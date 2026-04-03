use bendclaw::memory::decay::decay_score;

#[test]
fn fresh_high_access() {
    let score = decay_score(0.0, 100, 30.0);
    assert!(
        score > 0.8,
        "fresh + high access should score high: {score}"
    );
}

#[test]
fn fresh_zero_access() {
    let score = decay_score(0.0, 0, 30.0);
    assert!(
        score > 0.5,
        "fresh + zero access should still be decent: {score}"
    );
}

#[test]
fn old_zero_access() {
    let score = decay_score(90.0, 0, 30.0);
    assert!(
        score < 0.2,
        "90 days old + zero access should be low: {score}"
    );
}

#[test]
fn old_high_access() {
    let score = decay_score(90.0, 50, 30.0);
    assert!(
        score > 0.2,
        "old but high access should be moderate: {score}"
    );
}

#[test]
fn score_bounds() {
    assert!(decay_score(0.0, 0, 30.0) <= 1.0);
    assert!(decay_score(1000.0, 0, 30.0) >= 0.001);
}
