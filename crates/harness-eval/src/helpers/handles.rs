use super::*;

fn id(value: &Value) -> Result<String> {
    let name = if value.is_object() {
        // Eval creates fresh native children. Their handle covers the initial execution only.
        if value["run_epoch"] != 0 {
            return Err(failure("stale_task_handle", "invalid task handle epoch"));
        }
        value["id"].as_str()
    } else {
        value.as_str()
    };
    let name = name
        .map(|name| name.strip_prefix("agent://").unwrap_or(name))
        .filter(|name| {
            !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_whitespace)
        })
        .ok_or("expected a task id or agent handle")?;
    Ok(if value.is_object() {
        format!("eval:{name}:0")
    } else {
        name.into()
    })
}

fn snapshots(result: &Value) -> Result<Vec<Value>> {
    if result["details"]["isError"] == true {
        return Err(result_text(result).into());
    }
    let details = &result["details"];
    let rows = if let Some(row) = details.get("Result") {
        vec![row.clone()]
    } else {
        details["MultiResult"]["results"]
            .as_array()
            .ok_or("host returned no task results")?
            .clone()
    };
    Ok(rows.into_iter().map(|row| {
        let terminal = !matches!(row["status"].as_str(), Some("running" | "initializing" | "queued"));
        json!({"id":row["task_id"],"run_epoch":0,"status":row["status"],"terminal":terminal,"output":row["output"],"exit_code":row["exit_code"]})
    }).collect())
}

pub(super) async fn wait(session: &Inner, cell: &Cell, args: &Value) -> Result<Value> {
    keys(args, &["handles", "mode", "timeout"])?;
    let handles = args["handles"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= 20)
        .ok_or("wait() requires 1-20 handles")?;
    let ids = handles.iter().map(id).collect::<Result<Vec<_>>>()?;
    let mode = args
        .get("mode")
        .map_or(Some("all"), Value::as_str)
        .ok_or("wait() mode must be text")?;
    if !matches!(mode, "all" | "any" | "settled") {
        return Err("wait() mode must be all, any or settled".into());
    }
    let timeout = args
        .get("timeout")
        .map_or(Some(60.0), Value::as_f64)
        .filter(|timeout| timeout.is_finite() && (0.0..=3600.0).contains(timeout))
        .ok_or("wait() timeout must be 0-3600 seconds")?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs_f64(timeout);
    let mut observed = std::collections::BTreeMap::new();
    let mut pending = ids.clone();
    let mut poll = true;
    loop {
        let timeout_ms = if poll {
            0
        } else {
            u64::try_from(
                deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .as_millis(),
            )?
        };
        poll = false;
        let result = tool(
            session,
            cell,
            "wait_commands_or_subagents",
            json!({"task_ids":pending,"mode":"wait_any","timeout_ms":timeout_ms}),
        )
        .await?;
        for row in snapshots(&result)? {
            let id = row["id"]
                .as_str()
                .ok_or("task result id missing")?
                .to_owned();
            let key = ids
                .iter()
                .find(|requested| {
                    requested.as_str() == id || requested.as_str() == format!("eval:{id}:0")
                })
                .ok_or("unexpected task result id")?;
            observed.insert(key.clone(), row);
        }
        let results = ids
            .iter()
            .filter_map(|id| observed.get(id).cloned())
            .collect::<Vec<_>>();
        let successful = |row: &Value| row["terminal"] == true && row["status"] == "completed";
        if mode == "all"
            && results
                .iter()
                .any(|row| row["terminal"] == true && !successful(row))
        {
            return Err(failure(
                "eval_wait_failed",
                "a waited task failed or was cancelled; use mode=settled to collect its outcome",
            ));
        }
        pending.retain(|id| observed.get(id).is_none_or(|row| row["terminal"] != true));
        let done = if mode == "any" {
            results.iter().any(successful)
        } else {
            pending.is_empty()
        };
        if mode == "any" && !done && pending.is_empty() {
            return Err(failure(
                "eval_wait_failed",
                "no waited task completed successfully",
            ));
        }
        if done || tokio::time::Instant::now() >= deadline {
            return Ok(json!({"done":done,"mode":mode,"results":results}));
        }
    }
}

pub(super) async fn control(session: &Inner, cell: &Cell, args: &Value) -> Result<Value> {
    keys(args, &["handle", "op", "text", "delivery"])?;
    let id = id(&args["handle"])?;
    let (name, parameters) = match args["op"].as_str() {
        Some("status" | "output") => (
            "get_command_or_subagent_output",
            json!({"task_ids":[id],"timeout_ms":0}),
        ),
        Some("cancel") => ("kill_command_or_subagent", json!({"task_id":id})),
        Some("send") => {
            let text = args["text"]
                .as_str()
                .filter(|text| !text.is_empty())
                .ok_or("handle.send() requires text")?;
            let delivery = args
                .get("delivery")
                .map_or(Some("queue"), Value::as_str)
                .ok_or("message delivery must be text")?;
            if !matches!(delivery, "queue" | "steer" | "interject") {
                return Err("invalid message delivery".into());
            }
            (
                "send_subagent_message",
                json!({"subagent_id":id,"text":text,"delivery":delivery}),
            )
        }
        _ => return Err("unknown handle operation".into()),
    };
    let result = tool(session, cell, name, parameters).await?;
    if matches!(args["op"].as_str(), Some("status" | "output")) {
        let snapshot = snapshots(&result)?
            .into_iter()
            .next()
            .ok_or("task result missing")?;
        return Ok(if args["op"] == "output" {
            snapshot["output"].clone()
        } else {
            snapshot
        });
    }
    Ok(marshal(&result))
}
