use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::session::runtime::session_stream::FinishedRunOutput;
use bendclaw::kernel::task::execution::classify_task_run_output;

#[test]
fn marks_completed_runs_ok() {
    let (status, output, error) = classify_task_run_output(FinishedRunOutput {
        text: "done".to_string(),
        stop_reason: Reason::EndTurn,
    });
    assert_eq!(status, "ok");
    assert_eq!(output.as_deref(), Some("done"));
    assert!(error.is_none());
}

#[test]
fn marks_budget_runs_partial() {
    let (status, output, error) = classify_task_run_output(FinishedRunOutput {
        text: "partial summary".to_string(),
        stop_reason: Reason::MaxIterations,
    });
    assert_eq!(status, "partial");
    assert_eq!(output.as_deref(), Some("partial summary"));
    assert!(error
        .as_deref()
        .is_some_and(|value| value.contains("max_iterations")));
}

#[test]
fn marks_aborted_runs_cancelled() {
    let (status, _output, error) = classify_task_run_output(FinishedRunOutput {
        text: "".to_string(),
        stop_reason: Reason::Aborted,
    });
    assert_eq!(status, "cancelled");
    assert!(error.is_none());
}

#[test]
fn marks_error_runs_error() {
    let (status, _output, error) = classify_task_run_output(FinishedRunOutput {
        text: "".to_string(),
        stop_reason: Reason::Error,
    });
    assert_eq!(status, "error");
    assert!(error.is_some());
}
