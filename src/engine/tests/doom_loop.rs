use evotengine::doom_loop::DoomLoopDetector;
use serde_json::json;

fn call(name: &str, args: serde_json::Value) -> (String, String, serde_json::Value) {
    ("id".into(), name.into(), args)
}

#[test]
fn no_trigger_below_threshold() {
    let mut d = DoomLoopDetector::new(3);
    let batch = vec![call("read_file", json!({"path": "/a.rs"}))];
    assert!(d.check(&batch).is_none());
    assert!(d.check(&batch).is_none());
}

#[test]
fn triggers_at_threshold() {
    let mut d = DoomLoopDetector::new(3);
    let batch = vec![call("read_file", json!({"path": "/a.rs"}))];
    assert!(d.check(&batch).is_none());
    assert!(d.check(&batch).is_none());
    let intervention = d.check(&batch);
    assert!(intervention.is_some(), "expected doom loop at threshold 3");
    // Steering message is a user message
    if let Some(i) = intervention {
        assert_eq!(i.steering_message.role(), "user");
    }
}

#[test]
fn different_args_no_trigger() {
    let mut d = DoomLoopDetector::new(3);
    assert!(d
        .check(&[call("read_file", json!({"path": "/a.rs"}))])
        .is_none());
    assert!(d
        .check(&[call("read_file", json!({"path": "/b.rs"}))])
        .is_none());
    assert!(d
        .check(&[call("read_file", json!({"path": "/c.rs"}))])
        .is_none());
}

#[test]
fn different_tool_breaks_streak() {
    let mut d = DoomLoopDetector::new(3);
    let batch_a = vec![call("read_file", json!({"path": "/a.rs"}))];
    let batch_b = vec![call("search", json!({"query": "foo"}))];
    assert!(d.check(&batch_a).is_none());
    assert!(d.check(&batch_a).is_none());
    assert!(d.check(&batch_b).is_none()); // resets
    assert!(d.check(&batch_a).is_none()); // 1 again
}

#[test]
fn multi_tool_batch() {
    let mut d = DoomLoopDetector::new(3);
    let batch = vec![
        call("search", json!({"query": "foo"})),
        call("read_file", json!({"path": "/a.rs"})),
    ];
    assert!(d.check(&batch).is_none());
    assert!(d.check(&batch).is_none());
    assert!(
        d.check(&batch).is_some(),
        "expected doom loop for multi-tool batch"
    );
}

#[test]
fn multi_tool_batch_different_order_no_trigger() {
    let mut d = DoomLoopDetector::new(3);
    let batch_a = vec![
        call("search", json!({"query": "foo"})),
        call("read_file", json!({"path": "/a.rs"})),
    ];
    let batch_b = vec![
        call("read_file", json!({"path": "/a.rs"})),
        call("search", json!({"query": "foo"})),
    ];
    assert!(d.check(&batch_a).is_none());
    assert!(d.check(&batch_b).is_none());
    assert!(d.check(&batch_a).is_none());
}

#[test]
fn canonical_json_key_order() {
    let mut d = DoomLoopDetector::new(3);
    let batch_a = vec![call("bash", json!({"command": "ls", "timeout": 30}))];
    let batch_b = vec![call("bash", json!({"timeout": 30, "command": "ls"}))];
    assert!(d.check(&batch_a).is_none());
    assert!(d.check(&batch_b).is_none());
    assert!(d.check(&batch_a).is_some());
}

#[test]
fn blocked_batch_not_recorded() {
    let mut d = DoomLoopDetector::new(3);
    let batch = vec![call("read_file", json!({"path": "/a.rs"}))];
    assert!(d.check(&batch).is_none());
    assert!(d.check(&batch).is_none());
    assert!(d.check(&batch).is_some()); // blocked, NOT recorded
                                        // A different batch should work fine after blocking.
    let other = vec![call("search", json!({"query": "bar"}))];
    assert!(d.check(&other).is_none());
}

#[test]
fn steering_message_contains_count() {
    let mut d = DoomLoopDetector::new(3);
    let batch = vec![call("read_file", json!({"path": "/a.rs"}))];
    d.check(&batch);
    d.check(&batch);
    if let Some(intervention) = d.check(&batch) {
        if let evotengine::types::AgentMessage::Llm(evotengine::types::Message::User {
            content,
            ..
        }) = &intervention.steering_message
        {
            let text = content
                .iter()
                .filter_map(|c| {
                    if let evotengine::types::Content::Text { text } = c {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            assert!(text.contains("3 times"));
            assert!(text.contains("different tool"));
        } else {
            panic!("expected user message");
        }
    } else {
        panic!("expected doom loop intervention at threshold 3");
    }
}

// ---------------------------------------------------------------------------
// Doom loop scenario tests — simulate realistic agent turn sequences
// ---------------------------------------------------------------------------

/// Helper: simulate N turns of the same tool call batch, return how many
/// succeeded before intervention.
fn turns_before_intervention(
    detector: &mut DoomLoopDetector,
    batch: &[(String, String, serde_json::Value)],
    max_turns: usize,
) -> usize {
    for i in 0..max_turns {
        if detector.check(batch).is_some() {
            return i;
        }
    }
    max_turns
}

#[test]
fn scenario_stuck_bash_loop() {
    // Agent keeps running the same bash command
    let mut d = DoomLoopDetector::new(3);
    let batch = vec![call("bash", json!({"command": "cd /tmp && make build"}))];
    assert_eq!(turns_before_intervention(&mut d, &batch, 10), 2);
}

#[test]
fn scenario_progressive_exploration_no_trigger() {
    // Agent explores different files — should never trigger
    let mut d = DoomLoopDetector::new(3);
    let files = ["/a.rs", "/b.rs", "/c.rs", "/d.rs", "/e.rs"];
    for f in &files {
        let batch = vec![call("read_file", json!({"path": f}))];
        assert!(
            d.check(&batch).is_none(),
            "should not trigger for different files"
        );
    }
}

#[test]
fn scenario_search_then_read_varying_args() {
    // Agent does search→read with different args each time
    let mut d = DoomLoopDetector::new(3);
    for i in 0..5 {
        let batch = vec![
            call("search", json!({"query": format!("pattern_{i}")})),
            call("read_file", json!({"path": format!("/file_{i}.rs")})),
        ];
        assert!(
            d.check(&batch).is_none(),
            "should not trigger for varying args"
        );
    }
}

#[test]
fn scenario_intervention_then_recovery() {
    // Agent gets stuck, intervention fires, then agent tries different approach
    let mut d = DoomLoopDetector::new(3);
    let stuck = vec![call("bash", json!({"command": "failing_cmd"}))];
    assert!(d.check(&stuck).is_none());
    assert!(d.check(&stuck).is_none());
    assert!(d.check(&stuck).is_some()); // intervention

    // Agent changes approach
    let new_approach = vec![call("read_file", json!({"path": "/error.log"}))];
    assert!(
        d.check(&new_approach).is_none(),
        "different batch after intervention should pass"
    );
}

#[test]
fn scenario_relapse_after_recovery() {
    // Agent recovers but then falls back into the same loop
    let mut d = DoomLoopDetector::new(3);
    let stuck = vec![call("bash", json!({"command": "make test"}))];

    // First loop
    assert!(d.check(&stuck).is_none());
    assert!(d.check(&stuck).is_none());
    assert!(d.check(&stuck).is_some());

    // Recovery
    let other = vec![call("read_file", json!({"path": "/Makefile"}))];
    assert!(d.check(&other).is_none());

    // Relapse — counter resets, needs 3 again
    assert!(d.check(&stuck).is_none());
    assert!(d.check(&stuck).is_none());
    assert!(d.check(&stuck).is_some());
}
