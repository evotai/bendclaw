#[derive(Debug, Clone)]
pub struct RunRequest {
    pub prompt: String,
    pub session_id: Option<String>,
}

impl RunRequest {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            session_id: None,
        }
    }
}
