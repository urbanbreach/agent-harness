pub mod provider;
pub mod transport;

mod safety;
mod tests;
mod types;

pub use safety::{assert_cassette_is_safe, assert_http_cassette_is_safe, compact_json};
pub(crate) use types::CASSETTE_VERSION;
pub use types::{
    recorded_headers_to_header_map, CassetteError, CassetteInteraction, CassetteMode,
    OpenAiHttpCassette, OpenAiHttpInteraction, OpenAiHttpRecordedRequest,
    OpenAiHttpRecordedResponse, ProviderCassette,
};

pub use self::provider::RecordedProvider;
pub use self::transport::RecordedOpenAiHttpTransport;
