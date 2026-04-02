pub struct BlockCoalescer {
    min_chars: usize,
    max_chars: usize,
    buf: String,
}

impl BlockCoalescer {
    pub fn new(min_chars: usize, max_chars: usize) -> Self {
        Self {
            min_chars,
            max_chars,
            buf: String::new(),
        }
    }

    /// Returns a block to send if max_chars exceeded.
    pub fn push(&mut self, text: &str) -> Option<String> {
        self.buf.push_str(text);
        if self.buf.len() >= self.max_chars {
            Some(self.take())
        } else {
            None
        }
    }

    /// Call at natural break points (ToolStart, ReasonEnd).
    pub fn flush_if_ready(&mut self) -> Option<String> {
        if self.buf.len() >= self.min_chars {
            Some(self.take())
        } else {
            None
        }
    }

    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.buf)
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}
