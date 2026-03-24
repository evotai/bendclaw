use bendclaw::kernel::runtime::pending_decision::clarification_template;
use bendclaw::kernel::runtime::pending_decision::resolve_decision;
use bendclaw::kernel::runtime::pending_decision::DecisionResolution;
use bendclaw::kernel::runtime::turn_coordinator_state::TurnCoordinatorState;
use bendclaw::kernel::runtime::turn_relation::RunRisk;
use bendclaw::kernel::runtime::turn_relation::RunSnapshot;
use bendclaw::kernel::runtime::turn_relation::StubClassifier;
use bendclaw::kernel::runtime::turn_relation::TurnRelation;
use bendclaw::kernel::runtime::turn_relation::TurnRelationClassifier;

// ── RunSnapshot ───────────────────────────────────────────────────────────────

#[test]
fn snapshot_from_input_truncates_long_summary() {
    let long_input = "x".repeat(500);
    let snap = RunSnapshot::from_input("s1", "r1", &long_input);
    assert_eq!(snap.summary.len(), 200);
    assert_eq!(snap.session_id, "s1");
    assert_eq!(snap.run_id, "r1");
    assert_eq!(snap.risk, RunRisk::ReadOnly);
    assert!(snap.target_scope.is_none());
}

#[test]
fn snapshot_from_input_short_input() {
    let snap = RunSnapshot::from_input("s1", "r1", "hello");
    assert_eq!(snap.summary, "hello");
}

// ── StubClassifier ────────────────────────────────────────────────────────────

#[test]
fn stub_classifier_always_fork_or_ask() {
    let snap = RunSnapshot::from_input("s1", "r1", "clean test_ databases");
    assert_eq!(
        StubClassifier.classify(&snap, "also clean xx_ databases"),
        TurnRelation::ForkOrAsk
    );
    assert_eq!(
        StubClassifier.classify(&snap, "stop"),
        TurnRelation::ForkOrAsk
    );
}

// ── TurnCoordinatorState ──────────────────────────────────────────────────────

#[test]
fn coordinator_state_snapshot_roundtrip() {
    let state = TurnCoordinatorState::default();
    assert!(state.get_snapshot("s1").is_none());

    let snap = RunSnapshot::from_input("s1", "r1", "do something");
    state.store_snapshot("s1", snap);

    let got = state.get_snapshot("s1").unwrap();
    assert_eq!(got.run_id, "r1");
    assert_eq!(got.summary, "do something");

    state.remove_snapshot("s1");
    assert!(state.get_snapshot("s1").is_none());
}

#[test]
fn coordinator_state_decision_roundtrip() {
    use bendclaw::kernel::runtime::pending_decision::DecisionOption;
    use bendclaw::kernel::runtime::pending_decision::PendingDecision;

    let state = TurnCoordinatorState::default();
    assert!(state.get_decision("s1").is_none());

    let decision = PendingDecision {
        session_id: "s1".to_string(),
        active_run_id: "r1".to_string(),
        question_id: "q1".to_string(),
        question_text: "What do you want?".to_string(),
        candidate_input: "new task".to_string(),
        options: vec![
            DecisionOption::ContinueCurrent,
            DecisionOption::CancelAndSwitch,
        ],
        created_at: std::time::Instant::now(),
    };
    state.store_decision(decision);

    let got = state.get_decision("s1").unwrap();
    assert_eq!(got.active_run_id, "r1");
    assert_eq!(got.candidate_input, "new task");

    state.remove_decision("s1");
    assert!(state.get_decision("s1").is_none());
}

// ── resolve_decision ──────────────────────────────────────────────────────────

#[test]
fn resolve_decision_switch_keywords() {
    assert_eq!(
        resolve_decision("switch"),
        DecisionResolution::CancelAndSwitch
    );
    assert_eq!(
        resolve_decision("cancel it"),
        DecisionResolution::CancelAndSwitch
    );
    assert_eq!(
        resolve_decision("replace with new"),
        DecisionResolution::CancelAndSwitch
    );
    assert_eq!(
        resolve_decision("restart"),
        DecisionResolution::CancelAndSwitch
    );
}

#[test]
fn resolve_decision_append_keywords() {
    assert_eq!(
        resolve_decision("append"),
        DecisionResolution::AppendAsFollowup
    );
    assert_eq!(
        resolve_decision("do it after"),
        DecisionResolution::AppendAsFollowup
    );
    assert_eq!(
        resolve_decision("queue it"),
        DecisionResolution::AppendAsFollowup
    );
    assert_eq!(
        resolve_decision("handle it later"),
        DecisionResolution::AppendAsFollowup
    );
}

#[test]
fn resolve_decision_default_continue() {
    assert_eq!(
        resolve_decision("continue"),
        DecisionResolution::ContinueCurrent
    );
    assert_eq!(
        resolve_decision("keep going"),
        DecisionResolution::ContinueCurrent
    );
    assert_eq!(resolve_decision("ok"), DecisionResolution::ContinueCurrent);
    assert_eq!(resolve_decision(""), DecisionResolution::ContinueCurrent);
}

// ── clarification_template ────────────────────────────────────────────────────

#[test]
fn clarification_template_contains_summary() {
    let text = clarification_template("clean test_ databases");
    assert!(text.contains("clean test_ databases"));
    assert!(text.contains("continue"));
    assert!(text.contains("switch"));
    assert!(text.contains("append"));
}
