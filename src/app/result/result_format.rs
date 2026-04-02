/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultFormat {
    Text,
    Json,
    StreamJson,
    Sse,
}
