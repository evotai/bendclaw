/// Lightweight SSE line parser.
///
/// Feed raw byte chunks from an HTTP response; get back parsed data payloads.
///
/// ```ignore
/// let mut parser = SseParser::new();
/// for chunk in byte_stream {
///     for data in parser.feed(&chunk) {
///         match data {
///             SseData::Json(v) => handle(v),
///             SseData::Done => break,
///         }
///     }
/// }
/// ```
pub struct SseParser {
    buffer: String,
}

/// A parsed SSE data payload.
pub enum SseData {
    /// Successfully parsed JSON object.
    Json(serde_json::Value),
    /// OpenAI-style `[DONE]` sentinel.
    Done,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Feed raw bytes into the parser, returning all complete data payloads.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseData> {
        self.buffer.push_str(&String::from_utf8_lossy(chunk));

        let mut results = Vec::new();

        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].trim().to_string();
            self.buffer = self.buffer[pos + 1..].to_string();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            let data = match line.strip_prefix("data: ") {
                Some(d) => d.trim(),
                None => continue,
            };

            if data == "[DONE]" {
                results.push(SseData::Done);
                continue;
            }

            if let Ok(v) = serde_json::from_str(data) {
                results.push(SseData::Json(v));
            }
        }

        results
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}
