//! Schema shapes for the compaction settings whose loader grammar is wider than the field type.
use serde_json::{json, Value};

const PERCENT_TEXT: &str =
    r"^\s*(?:100(?:\.0+)?|[1-9][0-9]?(?:\.[0-9]+)?|0\.[0-9]*[1-9][0-9]*)\s*%\s*$";
const FIXED_TEXT: &str = r"^\s*[fF]0*[1-9][0-9]*\s*$";
const SELECTOR: &str = r"^[^\s:*]+[:/][^\s*]+\*?$|^[^\s:*]+[:/]\*$";

fn percent_number() -> Value {
    json!({"type":"number","exclusiveMinimum":0,"maximum":100})
}

fn positive_tokens() -> Value {
    json!({"type":"integer","minimum":1})
}

fn tokens_object(key: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(key.into(), positive_tokens());
    json!({"type":"object","additionalProperties":false,"required":[key],"properties":properties})
}

fn set(properties: &mut Value, canonical: &str, alias: &str, value: &Value) {
    for name in [canonical, alias] {
        properties[name] = value.clone();
    }
}

pub(super) fn overrides(defs: &mut Value) {
    let percent_text = json!({"type":"string","pattern":PERCENT_TEXT});
    let model_entry = json!({"anyOf":[
        percent_text, {"type":"string","pattern":FIXED_TEXT}, positive_tokens(),
        tokens_object("tokens"), tokens_object("base"), {"type":"null"}
    ]});
    let agent_entry = json!({"anyOf":[
        percent_text, positive_tokens(), tokens_object("tokens"), {"type":"null"}
    ]});
    let selector_names = json!({"pattern":SELECTOR});
    let properties = &mut defs["CompactionSettings"]["properties"];
    for (canonical, alias, entry, names) in [
        ("model_thresholds", "modelThresholds", &model_entry, true),
        ("agent_thresholds", "agentThresholds", &agent_entry, false),
        (
            "remote_models",
            "remoteModels",
            &json!({"anyOf":[{"$ref":"#/$defs/RemoteModelSettings"},{"type":"null"}]}),
            true,
        ),
        (
            "summary_models",
            "summaryModels",
            &json!({"anyOf":[{"type":"string","pattern":"\\S"},{"type":"null"}]}),
            true,
        ),
    ] {
        let mut map = properties[canonical].clone();
        map["type"] = json!(["object", "null"]);
        map["additionalProperties"] = entry.clone();
        if names {
            map["propertyNames"] = selector_names.clone();
        }
        set(properties, canonical, alias, &map);
    }
    let unset = json!({"const":-1});
    let null = json!({"type":"null"});
    for (canonical, alias, accepted) in [
        (
            "threshold_percent",
            "thresholdPercent",
            vec![
                percent_number(),
                json!({"type":"string","pattern":PERCENT_TEXT}),
                unset.clone(),
                null.clone(),
            ],
        ),
        (
            "threshold_tokens",
            "thresholdTokens",
            vec![positive_tokens(), unset.clone(), null.clone()],
        ),
        (
            "base_window_tokens",
            "baseWindowTokens",
            vec![positive_tokens(), unset.clone(), null.clone()],
        ),
        (
            "reserve_tokens",
            "reserveTokens",
            vec![
                json!({"type":"integer","minimum":0}),
                unset.clone(),
                null.clone(),
            ],
        ),
    ] {
        set(
            properties,
            canonical,
            alias,
            &json!({"anyOf":accepted,"default":null}),
        );
    }
    // These floor a finite number and clamp it into range, so negatives are accepted.
    for (canonical, alias, default) in [
        (
            "v2_retained_message_budget",
            "v2RetainedMessageBudget",
            64_000,
        ),
        ("idle_threshold_tokens", "idleThresholdTokens", 200_000),
        ("idle_timeout_seconds", "idleTimeoutSeconds", 300),
    ] {
        set(
            properties,
            canonical,
            alias,
            &json!({"type":"number","default":default}),
        );
    }
    let archive = &mut defs["ArchiveSettings"]["properties"];
    set(
        archive,
        "system_prompt",
        "systemPrompt",
        &json!({"type":"string","enum":["none","agents_md","all"],"default":"none"}),
    );
    if let Some(defs) = defs.as_object_mut() {
        for unused in [
            "CompactionThreshold",
            "CompactionThresholdPercent",
            "ArchiveSystemPrompt",
        ] {
            defs.remove(unused);
        }
    }
}
