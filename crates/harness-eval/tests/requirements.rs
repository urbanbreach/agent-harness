use harness_eval::{Session, SessionOptions, Settings};
use serde_json::json;
use std::{collections::BTreeMap, time::Duration};

#[tokio::test]
async fn missing_node_is_reported_when_javascript_is_used() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let session = Session::new(SessionOptions {
        environment: BTreeMap::from([("PATH".into(), root.path().as_os_str().to_owned())]),
        cwd: root.path().into(),
        artifacts: root.path().join("artifacts"),
        local_dir: root.path().join("local"),
        languages: vec!["js".into()],
        session_env: BTreeMap::new(),
        settings: Settings::default(),
    })
    .map_err(|error| error.to_string())?;
    let mut events = session
        .execute(
            "missing-node",
            json!({"language":"js","summary":"Check runtime availability","code":"1+1","on_timeout":"error"}),
            json!([]),
            false,
        )
        .await
        .map_err(|error| error.to_string())?;
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = events.recv().await {
            if event["type"] == "result" {
                return Ok::<_, Box<dyn std::error::Error>>(event["result"].clone());
            }
        }
        Err("eval ended without a result".into())
    })
    .await??;
    assert_eq!(result["details"]["isError"], true);
    assert!(
        result["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Node.js 24 or newer on PATH")),
        "{result}"
    );
    session.close().await.map_err(|error| error.to_string())?;
    Ok(())
}
