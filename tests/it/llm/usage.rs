use bendclaw::llm::usage::TokenUsage;
use serde_json::json;

// ── TokenUsage::new ──

#[test]
fn usage_new_computes_total() {
    let u = TokenUsage::new(100, 50);
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

// ── with_cache ──

#[test]
fn usage_with_cache() {
    let u = TokenUsage::new(200, 100).with_cache(50, 30);
    assert_eq!(u.cache_read_tokens, 50);
    assert_eq!(u.cache_write_tokens, 30);
    assert_eq!(u.total_tokens, 300);
}

// ── cache_hit_rate ──

#[test]
fn cache_hit_rate_with_reads() {
    let u = TokenUsage::new(100, 50).with_cache(75, 0);
    let rate = u.cache_hit_rate();
    assert!((rate - 0.75).abs() < f64::EPSILON);
}

#[test]
fn cache_hit_rate_zero_prompt() {
    let u = TokenUsage::new(0, 50);
    assert_eq!(u.cache_hit_rate(), 0.0);
}

#[test]
fn cache_hit_rate_no_cache() {
    let u = TokenUsage::new(100, 50);
    assert_eq!(u.cache_hit_rate(), 0.0);
}

// ── from_openai_json ──

#[test]
fn from_openai_json_full() {
    let j = json!({"prompt_tokens": 200, "completion_tokens": 80});
    let u = TokenUsage::from_openai_json(&j);
    assert_eq!(u.prompt_tokens, 200);
    assert_eq!(u.completion_tokens, 80);
    assert_eq!(u.total_tokens, 280);
}

#[test]
fn from_openai_json_missing_fields() {
    let j = json!({});
    let u = TokenUsage::from_openai_json(&j);
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
}

// ── from_anthropic_json ──

#[test]
fn from_anthropic_json_full() {
    let j = json!({
        "input_tokens": 150,
        "output_tokens": 60,
        "cache_read_input_tokens": 40,
        "cache_creation_input_tokens": 20
    });
    let u = TokenUsage::from_anthropic_json(&j);
    assert_eq!(u.prompt_tokens, 150);
    assert_eq!(u.completion_tokens, 60);
    assert_eq!(u.cache_read_tokens, 40);
    assert_eq!(u.cache_write_tokens, 20);
}

#[test]
fn from_anthropic_json_no_cache() {
    let j = json!({"input_tokens": 100, "output_tokens": 50});
    let u = TokenUsage::from_anthropic_json(&j);
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

// ── AddAssign ──

#[test]
fn add_assign_accumulates() {
    let mut total = TokenUsage::new(100, 50).with_cache(10, 5);
    let other = TokenUsage::new(200, 80).with_cache(20, 10);
    total += &other;

    assert_eq!(total.prompt_tokens, 300);
    assert_eq!(total.completion_tokens, 130);
    assert_eq!(total.total_tokens, 430);
    assert_eq!(total.cache_read_tokens, 30);
    assert_eq!(total.cache_write_tokens, 15);
}

// ── Serialization round-trip ──

#[test]
fn usage_serde_roundtrip() {
    let u = TokenUsage::new(100, 50).with_cache(25, 10);
    let json = serde_json::to_string(&u).unwrap();
    let u2: TokenUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u2.prompt_tokens, 100);
    assert_eq!(u2.completion_tokens, 50);
    assert_eq!(u2.cache_read_tokens, 25);
    assert_eq!(u2.cache_write_tokens, 10);
}
