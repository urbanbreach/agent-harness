use harness_eval::{Session, SessionOptions, Settings};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Instant};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn run(session: &Session, id: &str, language: &str, code: &str) -> Result<Value> {
    let mut events = session
        .execute(
            id,
            json!({"language":language,"code":code,"summary":"Measure eval","on_timeout":"error","reset":id.starts_with("cold-") && !id.ends_with("-0")}),
            json!([
                "perf_echo",
                "spawn_subagent",
                "get_command_or_subagent_output",
                "workpool"
            ]
            .map(|name| json!({"name":name,"description":"","parameters":{"type":"object"}}))),
            false,
        )
        .await?;
    while let Some(event) = events.recv().await {
        match event["type"].as_str() {
            Some("call") => {
                let result = if event["operation"] == "completion" {
                    json!({"text":"{\"answer\":42}","value":{"answer":42}})
                } else {
                    json!({"content":[{"type":"text","text":if event["args"]["name"] == "spawn_subagent" {"{\"answer\":42}"} else {"ok"}}],"details": if event["args"]["name"] == "workpool" {json!({"pool_id":"wp_0123456789abcdef0123456789abcdef"})} else {json!({})}})
                };
                session
                    .send(json!({"type":"reply","id":event["id"],"result":result}))
                    .await?;
            }
            Some("result") => return Ok(event["result"].clone()),
            _ => {}
        }
    }
    Err("benchmark cell disconnected".into())
}

#[tokio::test]
#[ignore = "release performance contract; scripts/qa/benchmark-eval.mjs"]
async fn perf_native_eval() -> Result {
    if cfg!(debug_assertions) {
        return Err("eval performance requires --release".into());
    }
    let destination = std::env::var_os("HARNESS_EVAL_PERF_OUTPUT")
        .ok_or("HARNESS_EVAL_PERF_OUTPUT is required")?;
    let directory = tempfile::tempdir()?;
    let session = Session::new(SessionOptions {
        environment: std::env::vars_os().collect(),
        cwd: directory.path().into(),
        artifacts: directory.path().join("artifacts"),
        local_dir: directory.path().join("local"),
        languages: vec!["js".into(), "py".into(), "rb".into(), "jl".into()],
        session_env: BTreeMap::new(),
        settings: Settings::default(),
    })?;
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../scripts/qa/fixtures/eval-performance.json"
    ))?;
    let mut results = Vec::new();
    let mut id = 0;
    for language in ["js", "py"] {
        let mut samples = Vec::new();
        for index in 0..5 {
            let start = Instant::now();
            let result = run(
                &session,
                &format!("cold-{language}-{index}"),
                language,
                "1+1",
            )
            .await?;
            assert_ne!(result["details"]["isError"], true, "{result}");
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        results.push(json!({"name":format!("cold-{language}"),"ms":samples}));
    }
    for case in cases {
        let mut samples = Vec::new();
        for iteration in 0..34 {
            let start = Instant::now();
            let result = run(
                &session,
                &id.to_string(),
                case["language"].as_str().ok_or("missing language")?,
                case["code"].as_str().ok_or("missing code")?,
            )
            .await?;
            assert_ne!(result["details"]["isError"], true, "{result}");
            if iteration >= 3 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            id += 1;
        }
        results.push(json!({"name":case["name"],"ms":samples}));
    }
    std::fs::write(&destination, serde_json::to_vec_pretty(&results)?)?;
    let corpus: Vec<Value> = serde_json::from_str(include_str!(
        "../../../scripts/qa/fixtures/eval-conformance.json"
    ))?;
    let mut checks = Vec::new();
    for entry in corpus {
        let result = run(
            &session,
            entry["name"].as_str().ok_or("missing name")?,
            entry["language"].as_str().ok_or("missing language")?,
            entry["code"].as_str().ok_or("missing code")?,
        )
        .await?;
        let text = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        checks.push(json!({"name":entry["name"],"error":result["details"]["isError"] == true,"text":text.trim()}));
    }
    let mut conformance = destination;
    conformance.push(".conformance.json");
    std::fs::write(conformance, serde_json::to_vec_pretty(&checks)?)?;
    session.close().await?;
    Ok(())
}
