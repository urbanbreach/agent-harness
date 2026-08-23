#[derive(Debug, thiserror::Error)]
pub enum ModelLimitError {
    #[error("model `{identity}` must define context and output together, with optional input, or omit all limits for explicit unknown mode")]
    Partial { identity: String },
    #[error("model `{identity}` context window must be greater than zero")]
    ZeroContext { identity: String },
    #[error("model `{identity}` max input must be greater than zero")]
    ZeroInput { identity: String },
    #[error("model `{identity}` max output must be greater than zero")]
    ZeroOutput { identity: String },
    #[error("model `{identity}` max input {input} exceeds context window {context}")]
    InputAboveContext {
        identity: String,
        input: u32,
        context: u32,
    },
    #[error("model `{identity}` max output {output} exceeds context window {context}")]
    OutputAboveContext {
        identity: String,
        output: u32,
        context: u32,
    },
}
