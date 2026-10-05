use super::*;

struct Echo;
#[async_trait::async_trait]
impl Tool for Echo {
    fn id(&self) -> &str {
        "perf_echo"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(
        &self,
        _ctx: ToolContext,
        _args: Value,
    ) -> std::result::Result<ToolResult, ToolError> {
        Ok(ToolResult::text("ok"))
    }
}

#[tokio::test]
#[ignore = "release-mode local runtimes; scripts/qa/benchmark-eval.mjs"]
async fn perf_eval() -> Result {
    if cfg!(debug_assertions) {
        return Err("eval performance evidence requires --release".into());
    }
    let destination = std::env::var_os("HARNESS_EVAL_PERF_OUTPUT")
        .ok_or("HARNESS_EVAL_PERF_OUTPUT is required")?;
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../scripts/qa/fixtures/eval-performance.json"
    ))?;
    let session = Session::with_tools(
        settings(),
        MockProvider::default(),
        vec![Arc::new(Echo)],
        false,
    )
    .await?;
    let mut reports = Vec::new();
    for language in ["js", "py"] {
        let start = std::time::Instant::now();
        session.good(language, "1 + 1").await?;
        reports.push(
            json!({"name":format!("cold-{language}"), "ms":[start.elapsed().as_secs_f64()*1000.0]}),
        );
    }
    for case in cases {
        let language = case["language"].as_str().ok_or("missing language")?;
        let code = case["code"].as_str().ok_or("missing code")?;
        let mut samples = Vec::new();
        for iteration in 0..34 {
            let start = std::time::Instant::now();
            session.good(language, code).await?;
            if iteration >= 3 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        reports.push(json!({"name":case["name"],"ms":samples}));
    }
    session.handle.stop_run().await?;
    std::fs::write(destination, serde_json::to_vec_pretty(&reports)?)?;
    Ok(())
}
