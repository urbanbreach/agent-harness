use crate::cell::{epoch_ms, Cell, State};
use serde_json::{json, Map, Value};

pub(crate) fn capped(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

pub(crate) fn arguments(value: &Value) -> (Option<Value>, bool) {
    let bounded = bound(value, 0);
    let truncated = &bounded != value;
    if bounded.to_string().len() > 4096 {
        (None, true)
    } else {
        (Some(bounded), truncated)
    }
}

fn bound(value: &Value, depth: usize) -> Value {
    match value {
        Value::String(text) => capped(text, 512).into(),
        Value::Array(_) | Value::Object(_) if depth >= 6 => "…".into(),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(32)
                .map(|value| bound(value, depth + 1))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .take(32)
                .map(|(key, value)| (key.clone(), bound(value, depth + 1)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn aggregate() -> Value {
    json!({"count":0,"totalDurationMs":0,"okCount":0,"errorCount":0,"pendingCount":0})
}

#[derive(Default)]
pub(crate) struct ToolCalls {
    pub count: usize,
    pub rows: Vec<Value>,
    names: Vec<String>,
    aggregates: Map<String, Value>,
    overflow: Option<Value>,
}

impl ToolCalls {
    fn group(&mut self, name: &str) -> &mut Value {
        if self.aggregates.contains_key(name) || self.aggregates.len() < 64 {
            if !self.aggregates.contains_key(name) {
                self.names.push(name.into());
            }
            self.aggregates.entry(name).or_insert_with(aggregate)
        } else {
            self.overflow.get_or_insert_with(aggregate)
        }
    }

    pub fn begin(&mut self, row: &Value) -> usize {
        let index = self.count;
        self.count += 1;
        if index < 30 {
            self.rows.push(row.clone());
        }
        let item = self.group(row["name"].as_str().unwrap_or_default());
        for (key, value) in [
            ("count", 1),
            ("pendingCount", 1),
            (
                "pendingStarted",
                row["startedAt"].as_u64().unwrap_or_default(),
            ),
        ] {
            item[key] = item[key]
                .as_u64()
                .unwrap_or_default()
                .saturating_add(value)
                .into();
        }
        index
    }

    pub fn finish(&mut self, index: usize, row: Value) {
        let item = self.group(row["name"].as_str().unwrap_or_default());
        for (key, value) in [
            ("pendingCount", 1),
            (
                "pendingStarted",
                row["startedAt"].as_u64().unwrap_or_default(),
            ),
        ] {
            item[key] = item[key]
                .as_u64()
                .unwrap_or_default()
                .saturating_sub(value)
                .into();
        }
        for (key, value) in [
            (
                if row["ok"] == true {
                    "okCount"
                } else {
                    "errorCount"
                },
                1,
            ),
            (
                "totalDurationMs",
                row["durationMs"].as_u64().unwrap_or_default(),
            ),
        ] {
            item[key] = item[key]
                .as_u64()
                .unwrap_or_default()
                .saturating_add(value)
                .into();
        }
        if let Some(saved) = self.rows.get_mut(index) {
            *saved = row;
        }
    }
}

pub(crate) fn execution(cell: &Cell, state: &State) -> Value {
    let completed = epoch_ms();
    let calls = &state.tool_calls;
    let mut aggregates = calls.aggregates.clone();
    let mut overflow = calls.overflow.clone();
    let mut pending = 0;
    for item in aggregates.values_mut().chain(overflow.iter_mut()) {
        let count = item["pendingCount"].as_u64().unwrap_or_default();
        pending += count;
        let elapsed = u64::try_from(completed)
            .unwrap_or(u64::MAX)
            .saturating_mul(count)
            .saturating_sub(item["pendingStarted"].as_u64().unwrap_or_default());
        item["totalDurationMs"] = item["totalDurationMs"]
            .as_u64()
            .unwrap_or_default()
            .saturating_add(elapsed)
            .into();
        if let Some(fields) = item.as_object_mut() {
            fields.remove("pendingStarted");
        }
    }
    let mut result = json!({"version":1,"detailLevel":"full","cellId":capped(&cell.id,128),"language":cell.language,
        "ok":!matches!(state.status,"error"|"cancelled"),"startedAt":cell.submitted_at,"completedAt":completed,
        "durationMs":cell.created.elapsed().as_millis(),"kernelDurationMs":state.duration.as_millis(),
        "queued_ms":state.started_at.unwrap_or(completed).saturating_sub(cell.submitted_at),"detached":state.detached,
        "toolCallCount":calls.count,"pendingToolCallCount":pending,"distinctToolsCalled":calls.names,
        "toolAggregates":aggregates,"toolAggregatesTruncated":overflow.is_some(),"toolCalls":calls.rows});
    if let Some(overflow) = overflow {
        result["toolAggregateOverflow"] = overflow;
    }
    if result["ok"] == false {
        result["error"] = capped(&state.output.snapshot().0, 512).into();
    }
    result
}

pub(crate) fn rpc(mut full: Value) -> Value {
    if let Some(object) = full.as_object_mut() {
        object.remove("error");
    }
    for call in full["toolCalls"].as_array_mut().into_iter().flatten() {
        if let Some(call) = call.as_object_mut() {
            call.retain(|key, _| matches!(key.as_str(), "name" | "ok" | "durationMs"));
        }
    }
    full["detailLevel"] = "metadata".into();
    full["rpcTruncated"] = false.into();
    if full.to_string().len() > 32 * 1024 {
        let mut total = aggregate();
        for item in full["toolAggregates"]
            .as_object()
            .into_iter()
            .flat_map(|map| map.values())
            .chain(full.get("toolAggregateOverflow"))
        {
            for field in [
                "count",
                "totalDurationMs",
                "okCount",
                "errorCount",
                "pendingCount",
            ] {
                total[field] = total[field]
                    .as_u64()
                    .unwrap_or_default()
                    .saturating_add(item[field].as_u64().unwrap_or_default())
                    .into();
            }
        }
        full["toolAggregateOverflow"] = total;
        full["toolCalls"] = json!([]);
        full["distinctToolsCalled"] = json!([]);
        full["toolAggregates"] = json!({});
        full["toolAggregatesTruncated"] = true.into();
        full["rpcTruncated"] = true.into();
    }
    full
}
