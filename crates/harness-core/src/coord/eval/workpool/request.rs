use super::*;

pub(super) fn operation(args: &Value) -> Result<&str, CoordinatorError> {
    let op = args["op"]
        .as_str()
        .ok_or_else(|| invalid("workpool op is required"))?;
    let allowed = match op {
        "create" => &["op", "agent", "name", "mode", "width", "tools"][..],
        "push" => &["op", "pool_id", "items"][..],
        "close" | "inspect" | "cancel" => &["op", "pool_id"][..],
        _ => return Err(invalid("unknown workpool operation")),
    };
    if args
        .as_object()
        .is_none_or(|o| o.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err(invalid("unexpected workpool argument"));
    }
    Ok(op)
}

pub(super) fn spec(args: &Value) -> Result<Value, CoordinatorError> {
    let mut spec = args["agent"].clone();
    if let Some(profile) = spec.as_str() {
        spec = json!({"subagent_type":profile});
    }
    let fields = spec
        .as_object_mut()
        .ok_or_else(|| invalid("workpool agent must be a profile name or object"))?;
    if fields.keys().any(|name| {
        ![
            "subagent_type",
            "agent",
            "category",
            "prompt",
            "model",
            "tools",
            "isolation",
        ]
        .contains(&name.as_str())
    }) {
        return Err(invalid("unexpected workpool agent field"));
    }
    for alias in ["agent", "category"] {
        if let Some(profile) = fields.remove(alias)
            && fields.insert("subagent_type".into(), profile).is_some()
        {
            return Err(invalid("use only one workpool agent profile field"));
        }
    }
    if let Some(tools) = args.get("tools") {
        fields.insert("tools".into(), tools.clone());
    }
    if fields.get("tools").is_some_and(|tools| {
        tools.as_array().is_none_or(|tools| {
            tools
                .iter()
                .any(|name| name.as_str().is_none_or(str::is_empty))
        })
    }) {
        return Err(invalid("workpool tools must be a list of names"));
    }
    for key in ["prompt", "subagent_type", "model"] {
        if fields.get(key).is_some_and(|value| !value.is_string()) {
            return Err(invalid("workpool agent fields must be text"));
        }
    }
    if args.get("mode").is_some_and(|mode| mode != "fresh") {
        return Err(invalid(
            "workpool mode must be fresh; each item gets its own native subagent",
        ));
    }
    Ok(spec)
}

pub(super) fn items(pool: &Pool, value: &Value) -> Result<Submissions, CoordinatorError> {
    let items = value
        .as_array()
        .ok_or_else(|| invalid("workpool items must be an array"))?;
    if items.is_empty() || pool.children.len() + items.len() > 128 {
        return Err(invalid("workpool accepts 1-128 total items"));
    }
    let mut submissions = Vec::new();
    for item in items {
        let input_text = item.get("input").map(|input| {
            input
                .as_str()
                .map_or_else(|| input.to_string(), str::to_owned)
        });
        let prompt = item
            .as_str()
            .or_else(|| item["prompt"].as_str())
            .or(input_text.as_deref())
            .filter(|s| !s.trim().is_empty() && s.len() <= 65536)
            .ok_or_else(|| invalid("each workpool item needs a prompt of 1-65536 bytes"))?;
        if item.as_object().is_some_and(|o| {
            o.keys()
                .any(|key| !["prompt", "label", "key", "input"].contains(&key.as_str()))
        }) {
            return Err(invalid("unexpected workpool item field"));
        }
        let label = match item.get("label") {
            Some(label) => label
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.len() <= 128)
                .ok_or_else(|| invalid("workpool label must contain 1-128 bytes"))?,
            None => &pool.name,
        };
        let child = uuid::Uuid::now_v7().to_string();
        let key = match item.get("key") {
            Some(key) => key
                .as_str()
                .filter(|key| !key.is_empty() && key.len() <= 128)
                .ok_or_else(|| invalid("workpool item key must contain 1-128 bytes"))?
                .to_owned(),
            None => child.clone(),
        };
        if pool.keys.values().any(|existing| existing == &key)
            || submissions.iter().any(|(_, existing, _)| existing == &key)
        {
            return Err(invalid("workpool item keys must be unique"));
        }
        let mut spec = pool.spec.clone();
        let prefix = spec["prompt"].as_str().unwrap_or_default();
        spec["prompt"] = json!(if prefix.is_empty() {
            prompt.to_owned()
        } else {
            format!("{prefix}\n\n{prompt}")
        });
        spec["task_id"] = json!(child);
        spec["description"] = json!(label);
        spec["background"] = json!(true);
        submissions.push((child, key, spec));
    }
    Ok(submissions)
}
