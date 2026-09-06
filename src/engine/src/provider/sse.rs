//! Pull-based SSE decoding. The consumer drives HTTP reads directly: there is
//! no parser task or event queue to outlive it or accumulate behind it.
use futures::stream::BoxStream;
use futures::Stream;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

pub struct SseReader {
    stream: BoxStream<'static, Result<Vec<u8>, String>>,
    buffer: Vec<u8>,
    start: usize,
    scan: usize,
    ended: bool,
}

impl SseReader {
    pub fn new(response: reqwest::Response) -> Self {
        Self::from_stream(response.bytes_stream().map(|chunk| {
            chunk
                .map(|bytes| bytes.to_vec())
                .map_err(|error| format!("Stream read error: {error}"))
        }))
    }

    /// Stream injection for transport tests, without sockets or provider state.
    pub fn from_stream(
        stream: impl Stream<Item = Result<Vec<u8>, String>> + Send + 'static,
    ) -> Self {
        Self {
            stream: stream.boxed(),
            buffer: Vec::new(),
            start: 0,
            scan: 0,
            ended: false,
        }
    }

    pub async fn next(&mut self, cancel: &CancellationToken) -> Result<Option<SseEvent>, String> {
        loop {
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            while self.scan < self.buffer.len() {
                self.scan += 1;
                let frame = &self.buffer[self.start..self.scan];
                let separator = if frame.ends_with(b"\n\n") {
                    2
                } else if frame.ends_with(b"\r\n\r\n") {
                    4
                } else {
                    0
                };
                if frame.len().saturating_sub(separator) > MAX_FRAME_BYTES
                    && (separator > 0 || frame.len() > MAX_FRAME_BYTES + 3)
                {
                    return Err("SSE frame exceeds the 8 MiB limit".into());
                }
                if separator > 0 {
                    let event = parse_frame(&frame[..frame.len() - separator]);
                    self.start = self.scan;
                    if event.is_some() {
                        return Ok(event);
                    }
                }
            }
            if self.ended {
                let remaining = &self.buffer[self.start..];
                if remaining.len() > MAX_FRAME_BYTES {
                    return Err("SSE frame exceeds the 8 MiB limit".into());
                }
                let event = parse_frame(remaining);
                self.buffer.clear();
                self.start = 0;
                self.scan = 0;
                return Ok(event);
            }
            // Keep only the partial frame before requesting more network data.
            // UTF-8 is decoded after framing, not per HTTP chunk.
            if self.start > 0 {
                self.buffer.drain(..self.start);
                self.scan -= self.start;
                self.start = 0;
            }
            let chunk = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err("cancelled".into()),
                chunk = self.stream.next() => chunk,
            };
            match chunk {
                Some(Ok(bytes)) => self.buffer.extend_from_slice(&bytes),
                Some(Err(error)) => return Err(error),
                None => self.ended = true,
            }
        }
    }
}

fn parse_frame(frame: &[u8]) -> Option<SseEvent> {
    let frame = String::from_utf8_lossy(frame);
    let mut event = String::new();
    let mut data = Vec::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start_matches(' '));
        }
    }
    if data.is_empty() {
        return None;
    }
    if event.is_empty() {
        event = "message".into();
    }
    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}
