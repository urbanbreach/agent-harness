#[derive(Debug, Clone, Default)]
pub(crate) struct QuestionPromptState {
    pub(crate) permission_id: Option<String>,
    pub(crate) tab: usize,
    pub(crate) selection: usize,
    pub(crate) answers: Vec<Vec<String>>,
    pub(crate) custom: Vec<String>,
    pub(crate) editing: bool,
    pub(crate) answer_buffer: String,
    pub(crate) answer_cursor: usize,
    pub(crate) answer_error: Option<String>,
}
