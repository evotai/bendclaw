/// Per-dispatch owned context — passed into each `dispatch()` call.
///
/// No setters, no stored state on ToolOrchestrator.
#[derive(Clone, Debug)]
pub struct TurnContext {
    pub turn: u32,
    pub loop_span_id: String,
}
