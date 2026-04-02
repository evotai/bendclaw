use bendclaw::observability::run_lifecycle::LifecycleEvent;

#[test]
fn lifecycle_run_started_serializes_with_tag() {
    let event = LifecycleEvent::RunStarted {
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "run.started");
    assert_eq!(json["run_id"], "r01");
}

#[test]
fn lifecycle_run_completed_serializes_with_tag() {
    let event = LifecycleEvent::RunCompleted {
        run_id: "r01".into(),
        session_id: "s01".into(),
        iterations: 5,
        stop_reason: "end_turn".into(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "run.completed");
    assert_eq!(json["iterations"], 5);
}

#[test]
fn lifecycle_cleanup_completed_serializes() {
    let event = LifecycleEvent::CleanupCompleted {
        user_id: "u01".into(),
        agent_id: "a01".into(),
        cleaned: 3,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "cleanup.completed");
    assert_eq!(json["cleaned"], 3);
}

#[test]
fn lifecycle_all_variants_serialize() {
    let events = vec![
        LifecycleEvent::RunStarted {
            run_id: "r".into(),
            session_id: "s".into(),
            agent_id: "a".into(),
            user_id: "u".into(),
        },
        LifecycleEvent::RunResumed {
            run_id: "r".into(),
            session_id: "s".into(),
            parent_run_id: "p".into(),
        },
        LifecycleEvent::RunCompleted {
            run_id: "r".into(),
            session_id: "s".into(),
            iterations: 1,
            stop_reason: "end".into(),
        },
        LifecycleEvent::RunFailed {
            run_id: "r".into(),
            session_id: "s".into(),
            error: "oops".into(),
        },
        LifecycleEvent::RunInterrupted {
            run_id: "r".into(),
            session_id: "s".into(),
            reason: "ctrl-c".into(),
        },
        LifecycleEvent::CleanupStarted {
            user_id: "u".into(),
            agent_id: "a".into(),
            policy: "Full".into(),
        },
        LifecycleEvent::CleanupCompleted {
            user_id: "u".into(),
            agent_id: "a".into(),
            cleaned: 0,
        },
    ];
    for event in &events {
        let json = serde_json::to_string(event);
        assert!(json.is_ok(), "All lifecycle variants must serialize");
    }
}
