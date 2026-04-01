use crate::kernel::run::result::Reason;
use crate::kernel::session::runtime::session_stream::FinishedRunOutput;

pub fn classify_task_run_output(
    finished: FinishedRunOutput,
) -> (String, Option<String>, Option<String>) {
    let text = if finished.text.trim().is_empty() {
        None
    } else {
        Some(finished.text)
    };

    match finished.stop_reason {
        Reason::EndTurn => ("ok".to_string(), text, None),
        Reason::MaxIterations | Reason::Timeout => (
            "partial".to_string(),
            text,
            Some(format!(
                "agent stopped before completing the task: {}",
                finished.stop_reason.as_str()
            )),
        ),
        Reason::Aborted => ("cancelled".to_string(), text, None),
        reason => (
            "error".to_string(),
            text,
            Some(format!("agent encountered an error: {}", reason.as_str())),
        ),
    }
}
