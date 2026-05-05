use serde::Serialize;

pub(crate) fn digest12(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().chars().take(12).collect()
}

pub(crate) fn digest12_json(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    digest12(&bytes)
}
