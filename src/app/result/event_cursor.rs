/// Cursor-based event pagination for polling clients.
///
/// Tracks the last-seen sequence number so clients can resume
/// from where they left off using `after_seq` semantics.
#[derive(Debug, Clone)]
pub struct EventCursor {
    last_seq: u64,
}

impl EventCursor {
    pub fn new() -> Self {
        Self { last_seq: 0 }
    }

    /// Create a cursor starting after the given sequence number.
    pub fn after(seq: u64) -> Self {
        Self { last_seq: seq }
    }

    /// Returns the last-seen sequence number.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Advance the cursor to the given sequence number.
    pub fn advance(&mut self, seq: u64) {
        if seq > self.last_seq {
            self.last_seq = seq;
        }
    }

    /// Returns `true` if the given sequence should be included (i.e. is after the cursor).
    pub fn includes(&self, seq: u64) -> bool {
        seq > self.last_seq
    }

    /// Encode the cursor as an opaque string for client transport.
    pub fn encode(&self) -> String {
        format!("seq:{}", self.last_seq)
    }

    /// Decode a cursor from the opaque string representation.
    pub fn decode(s: &str) -> Option<Self> {
        let seq_str = s.strip_prefix("seq:")?;
        let seq = seq_str.parse::<u64>().ok()?;
        Some(Self { last_seq: seq })
    }
}

impl Default for EventCursor {
    fn default() -> Self {
        Self::new()
    }
}
