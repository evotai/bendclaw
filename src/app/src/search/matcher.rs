pub struct TextMatcher {
    query: String,
}

impl TextMatcher {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_lowercase(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub fn matches(&self, text: &str) -> bool {
        if self.query.is_empty() {
            return true;
        }
        let lower = text.to_lowercase();
        if lower.contains(&self.query) {
            return true;
        }
        self.is_subsequence(&lower)
    }

    pub fn is_substring(&self, text: &str) -> bool {
        text.to_lowercase().contains(&self.query)
    }

    fn is_subsequence(&self, text: &str) -> bool {
        let mut chars = self.query.chars();
        let mut target = match chars.next() {
            Some(c) => c,
            None => return true,
        };
        for c in text.chars() {
            if c == target {
                target = match chars.next() {
                    Some(c) => c,
                    None => return true,
                };
            }
        }
        false
    }
}
