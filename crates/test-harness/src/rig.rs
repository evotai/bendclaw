use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use crate::mocks::context::test_session;
use crate::mocks::llm::MockTurn;
use crate::replay::TraceLlm;

// ── TraceMetrics ──────────────────────────────────────────────────────────────

/// Aggregate metrics collected from a single TestRig run.
pub struct TraceMetrics {
    pub llm_calls: usize,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    /// How many times each tool was invoked.
    pub tool_invocations: HashMap<String, u32>,
}

// ── TestRig ───────────────────────────────────────────────────────────────────

/// Wires TraceLlm + Session together for integration tests.
///
/// ```rust,ignore
/// let rig = TestRig::builder()
///     .with_turns(vec![MockTurn::Text("done".into())])
///     .build().await?;
///
/// let reply = rig.chat("hello").await?;
/// rig.verify()?;
/// let m = rig.metrics();
/// assert_eq!(m.llm_calls, 1);
/// ```
pub struct TestRig {
    session: bendclaw::sessions::Session,
    trace: Arc<TraceLlm>,
}

impl TestRig {
    pub fn builder() -> TestRigBuilder {
        TestRigBuilder::default()
    }

    /// Send a message and collect the final text response.
    pub async fn chat(&self, message: &str) -> Result<String> {
        Ok(self.session.chat(message, "").await?.finish().await?)
    }

    /// Assert all declarative expectations declared in the replay fixture.
    pub fn verify(&self) -> Result<()> {
        self.trace.verify()
    }

    /// Return aggregate metrics from the recorded call log.
    pub fn metrics(&self) -> TraceMetrics {
        let log = self.trace.call_log();
        let llm_calls = log.len();
        let total_prompt_tokens = log.iter().map(|r| r.usage.prompt_tokens).sum();
        let total_completion_tokens = log.iter().map(|r| r.usage.completion_tokens).sum();

        let mut tool_invocations: HashMap<String, u32> = HashMap::new();
        for record in &log {
            match &record.turn {
                MockTurn::ToolCall { name, .. } => {
                    *tool_invocations.entry(name.clone()).or_insert(0) += 1;
                }
                MockTurn::ToolCalls(calls) => {
                    for (name, _) in calls {
                        *tool_invocations.entry(name.clone()).or_insert(0) += 1;
                    }
                }
                MockTurn::Text(_) => {}
            }
        }

        TraceMetrics {
            llm_calls,
            total_prompt_tokens,
            total_completion_tokens,
            tool_invocations,
        }
    }
}

// ── TestRigBuilder ────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct TestRigBuilder {
    turns: Option<Vec<MockTurn>>,
    replay_name: Option<String>,
}

impl TestRigBuilder {
    /// Use a sequence of mock turns (no fixture file needed).
    pub fn with_turns(mut self, turns: Vec<MockTurn>) -> Self {
        self.turns = Some(turns);
        self
    }

    /// Load exchanges from `tests/fixtures/replays/{name}.json`.
    pub fn with_replay(mut self, name: &str) -> Self {
        self.replay_name = Some(name.to_string());
        self
    }

    pub async fn build(self) -> Result<TestRig> {
        let trace = Arc::new(match (self.replay_name, self.turns) {
            (Some(name), _) => TraceLlm::from_replay(&name)?,
            (None, Some(turns)) => TraceLlm::from_turns(turns),
            (None, None) => TraceLlm::from_turns(vec![]),
        });
        let session = test_session(trace.clone()).await?;
        Ok(TestRig { session, trace })
    }
}
