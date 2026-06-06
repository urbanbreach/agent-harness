use reqwest::header::HeaderMap;

use super::non_empty_string;

use crate::{
    ProviderErrorCategory, ProviderStreamEvent, ProviderStreamFinishedMetadata,
    ProviderStreamStartMetadata,
};

pub(super) fn provider_stream_start_metadata_from_headers(
    headers: &HeaderMap,
) -> Option<ProviderStreamStartMetadata> {
    let metadata = ProviderStreamStartMetadata {
        provider_session_id: first_header_value(
            headers,
            &[
                "x-provider-session-id",
                "x-session-id",
                "openai-session-id",
                "session-id",
            ],
        ),
        provider_cache_id: first_header_value(
            headers,
            &[
                "x-provider-cache-id",
                "x-cache-id",
                "openai-cache-id",
                "cache-id",
            ],
        ),
    };

    (metadata.provider_session_id.is_some() || metadata.provider_cache_id.is_some())
        .then_some(metadata)
}

fn first_header_value(headers: &HeaderMap, names: &[&'static str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .and_then(non_empty_string)
            .map(str::to_string)
    })
}

pub(super) fn provider_stream_finished_metadata_from_start(
    start_metadata: Option<ProviderStreamStartMetadata>,
) -> ProviderStreamFinishedMetadata {
    let Some(start_metadata) = start_metadata else {
        return ProviderStreamFinishedMetadata::default();
    };

    ProviderStreamFinishedMetadata {
        provider_session_id: start_metadata.provider_session_id,
        provider_cache_id: start_metadata.provider_cache_id,
        ..ProviderStreamFinishedMetadata::default()
    }
}

pub(super) fn non_empty_finished_metadata(
    metadata: ProviderStreamFinishedMetadata,
) -> Option<ProviderStreamFinishedMetadata> {
    (metadata != ProviderStreamFinishedMetadata::default()).then_some(metadata)
}

pub(super) fn malformed_stream_error(message: impl Into<String>) -> ProviderStreamEvent {
    ProviderStreamEvent::categorized_error(message, ProviderErrorCategory::MalformedStream)
}

pub(super) fn transport_failure_error(message: impl Into<String>) -> ProviderStreamEvent {
    ProviderStreamEvent::categorized_error(message, ProviderErrorCategory::TransportFailure)
}

pub(super) fn unsupported_tool_call_error(message: impl Into<String>) -> ProviderStreamEvent {
    ProviderStreamEvent::categorized_error(message, ProviderErrorCategory::UnsupportedToolCall)
}
