//! Identifier wrappers required by the unchanged TUI and V1 event contracts.
macro_rules! identifiers {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl From<String> for $name { fn from(value: String) -> Self { Self(value) } }
        impl From<&str> for $name { fn from(value: &str) -> Self { Self(value.to_owned()) } }
        impl AsRef<str> for $name { fn as_ref(&self) -> &str { self.as_str() } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.as_str()) }
        }
    )+ };
}
identifiers!(
    RunId,
    RunName,
    SessionId,
    EntryId,
    TurnId,
    TaskId,
    RequestId,
    ProviderRequestId,
    ToolCallId
);
