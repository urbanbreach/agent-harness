use crate::text::collapse_inline_whitespace;

#[derive(Debug, Clone)]
enum OrderedToolInputValue {
    String(String),
    Number(String),
    Bool(bool),
    Null,
    Array(()),
    Object(Vec<(String, OrderedToolInputValue)>),
}

impl<'de> serde::Deserialize<'de> for OrderedToolInputValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OrderedToolInputValueVisitor;

        impl<'de> serde::de::Visitor<'de> for OrderedToolInputValueVisitor {
            type Value = OrderedToolInputValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("ordered JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Number(value.to_string()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Number(value.to_string()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Number(value.to_string()))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(OrderedToolInputValue::String(value.to_string()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::String(value))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Null)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(OrderedToolInputValue::Null)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                while seq.next_element::<OrderedToolInputValue>()?.is_some() {}
                Ok(OrderedToolInputValue::Array(()))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut fields = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, OrderedToolInputValue>()? {
                    fields.push((key, value));
                }
                Ok(OrderedToolInputValue::Object(fields))
            }
        }

        deserializer.deserialize_any(OrderedToolInputValueVisitor)
    }
}

fn compact_tool_input_part(key: &str, value: &OrderedToolInputValue) -> Option<String> {
    match value {
        OrderedToolInputValue::String(text) => {
            let rendered = collapse_inline_whitespace(text);
            (!rendered.is_empty()).then(|| format!("{key}={rendered}"))
        }
        OrderedToolInputValue::Number(number) => Some(format!("{key}={number}")),
        OrderedToolInputValue::Bool(flag) => Some(format!("{key}={flag}")),
        OrderedToolInputValue::Null => Some(format!("{key}=null")),
        OrderedToolInputValue::Array(_) | OrderedToolInputValue::Object(_) => None,
    }
}

pub(super) fn tool_input_label(args_summary: &str, unwrap_arguments: bool) -> Option<String> {
    ordered_tool_input_display_value(args_summary, unwrap_arguments)
        .as_ref()
        .and_then(tool_input_label_from_value)
}

pub(super) fn tool_input_args(
    args_summary: &str,
    unwrap_arguments: bool,
    omit_keys: &[&str],
) -> Vec<String> {
    ordered_tool_input_display_value(args_summary, unwrap_arguments)
        .as_ref()
        .map(|value| tool_input_args_from_value(value, omit_keys, 3))
        .unwrap_or_default()
}

fn ordered_tool_input_display_value(
    args_summary: &str,
    unwrap_arguments: bool,
) -> Option<OrderedToolInputValue> {
    let value = serde_json::from_str::<OrderedToolInputValue>(args_summary).ok()?;
    if !unwrap_arguments {
        return Some(value);
    }

    match value {
        OrderedToolInputValue::Object(fields) => fields
            .iter()
            .find(|(key, _)| key == "arguments")
            .map(|(_, value)| value.clone())
            .or(Some(OrderedToolInputValue::Object(fields))),
        _ => Some(value),
    }
}

fn tool_input_label_from_value(value: &OrderedToolInputValue) -> Option<String> {
    let OrderedToolInputValue::Object(fields) = value else {
        return None;
    };

    [
        "description",
        "query",
        "url",
        "filePath",
        "path",
        "pattern",
        "name",
    ]
    .iter()
    .find_map(|key| {
        fields
            .iter()
            .find(|(field, _)| field == key)
            .and_then(|(_, value)| match value {
                OrderedToolInputValue::String(text) => {
                    let rendered = collapse_inline_whitespace(text);
                    (!rendered.is_empty()).then_some(rendered)
                }
                _ => None,
            })
    })
}

fn tool_input_args_from_value(
    value: &OrderedToolInputValue,
    omit_keys: &[&str],
    max_parts: usize,
) -> Vec<String> {
    let OrderedToolInputValue::Object(fields) = value else {
        return Vec::new();
    };
    let skip = [
        "description",
        "query",
        "url",
        "filePath",
        "path",
        "pattern",
        "name",
    ];

    let mut parts = Vec::new();
    for (key, value) in fields.iter() {
        if skip.contains(&key.as_str()) || omit_keys.contains(&key.as_str()) {
            continue;
        }
        if let Some(part) = compact_tool_input_part(key, value) {
            parts.push(part);
        }
        if parts.len() >= max_parts {
            break;
        }
    }
    parts
}

pub(super) fn compact_tool_trigger_subtitle(
    label: Option<String>,
    args: Vec<String>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(label) = label {
        parts.push(label);
    }
    parts.extend(args.into_iter().map(|arg| format!("[{arg}]")));
    (!parts.is_empty()).then(|| parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::{compact_tool_trigger_subtitle, tool_input_args, tool_input_label};

    #[test]
    fn tool_input_label_prefers_descriptive_string_fields() {
        // arrange
        // act
        // assert
        assert_eq!(
            tool_input_label(
                r#"{"command":"ignored","query":"  find\n matches ","limit":5}"#,
                false,
            ),
            Some("find matches".to_string())
        );
    }

    #[test]
    fn tool_input_args_preserve_json_order_and_skip_label_fields() {
        // arrange
        // act
        // assert
        assert_eq!(
            tool_input_args(
                r#"{"query":"find","offset":2,"limit":5,"include_hidden":false,"nested":{}}"#,
                false,
                &[],
            ),
            vec![
                "offset=2".to_string(),
                "limit=5".to_string(),
                "include_hidden=false".to_string(),
            ]
        );
    }

    #[test]
    fn tool_input_args_can_unwrap_nested_arguments_and_omit_keys() {
        // arrange
        // act
        // assert
        assert_eq!(
            tool_input_args(
                r#"{"server":"ignored","arguments":{"name":"search","tool":"grep","limit":3,"exact":true}}"#,
                true,
                &["tool"],
            ),
            vec!["limit=3".to_string(), "exact=true".to_string()]
        );
    }

    #[test]
    fn compact_tool_trigger_subtitle_combines_label_and_args() {
        // arrange
        // act
        // assert
        assert_eq!(
            compact_tool_trigger_subtitle(
                Some("needle".to_string()),
                vec!["limit=3".to_string(), "exact=true".to_string()],
            ),
            Some("needle [limit=3] [exact=true]".to_string())
        );
    }
}
