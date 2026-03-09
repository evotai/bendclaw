use bendclaw::llm::config::LLMConfig;
use bendclaw::llm::config::ProviderEndpoint;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::router::LLMRouter;
use bendclaw::llm::stream::StreamEvent;
use tokio_stream::StreamExt;

// ── LLMRouter from empty config ──

#[tokio::test]
async fn router_empty_config_returns_error() {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).expect("router build");

    let result = router
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("no LLM providers"));
}

#[tokio::test]
async fn router_empty_config_stream_returns_error() {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).expect("router build");

    let mut stream = router.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);
    let event = stream.next().await.unwrap();
    match event {
        StreamEvent::Error(msg) => assert!(msg.contains("no LLM providers")),
        _ => panic!("expected Error event"),
    }
}

// ── LLMRouter defaults ──

#[test]
fn router_empty_default_model() {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).expect("router build");
    assert_eq!(router.default_model(), "unknown");
}

#[test]
fn router_empty_default_temperature() {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).expect("router build");
    assert!((router.default_temperature() - 0.7).abs() < f32::EPSILON);
}

#[test]
fn router_empty_pricing() {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).expect("router build");
    assert!(router.pricing("any").is_none());
}

// ── LLMRouter with providers ──

fn make_config_with_providers() -> LLMConfig {
    LLMConfig {
        providers: vec![
            ProviderEndpoint {
                name: "primary".into(),
                provider: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: "test-key".into(),
                model: "gpt-4".into(),
                weight: 100,
                temperature: 0.5,
                input_price: 30.0,
                output_price: 60.0,
            },
            ProviderEndpoint {
                name: "fallback".into(),
                provider: "openai".into(),
                base_url: "https://api.openai.com/v1".into(),
                api_key: "test-key-2".into(),
                model: "gpt-3.5".into(),
                weight: 50,
                temperature: 0.7,
                input_price: 0.5,
                output_price: 1.5,
            },
        ],
        max_retries: 1,
        base_backoff_ms: 50,
        circuit_breaker_threshold: 3,
        circuit_breaker_cooldown_secs: 60,
    }
}

#[test]
fn router_default_model_from_highest_weight() {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).expect("router build");
    assert_eq!(router.default_model(), "gpt-4");
}

#[test]
fn router_default_temperature_from_highest_weight() {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).expect("router build");
    assert!((router.default_temperature() - 0.5).abs() < f32::EPSILON);
}

#[test]
fn router_pricing_by_model() {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).expect("router build");

    let (input, output) = router.pricing("gpt-4").unwrap();
    assert!((input - 30.0).abs() < f64::EPSILON);
    assert!((output - 60.0).abs() < f64::EPSILON);

    let (input, output) = router.pricing("gpt-3.5").unwrap();
    assert!((input - 0.5).abs() < f64::EPSILON);
    assert!((output - 1.5).abs() < f64::EPSILON);
}

#[test]
fn router_pricing_unknown_model_falls_back_to_first() {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).expect("router build");

    let (input, output) = router.pricing("unknown-model").unwrap();
    assert!((input - 30.0).abs() < f64::EPSILON);
    assert!((output - 60.0).abs() < f64::EPSILON);
}

#[test]
fn empty_config_creates_router() {
    let cfg = LLMConfig::default();
    let router = LLMRouter::from_config(&cfg).unwrap();
    assert_eq!(router.default_model(), "unknown");
    assert_eq!(router.default_temperature(), 0.7);
}

#[test]
fn router_with_provider() {
    let cfg = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "test".into(),
            provider: "openai".into(),
            base_url: "http://localhost:1234".into(),
            api_key: "sk-test".into(),
            model: "gpt-4".into(),
            weight: 100,
            temperature: 0.5,
            input_price: 2.5,
            output_price: 10.0,
        }],
        ..Default::default()
    };
    let router = LLMRouter::from_config(&cfg).unwrap();
    assert_eq!(router.default_model(), "gpt-4");
    assert_eq!(router.default_temperature(), 0.5);
}

#[test]
fn router_pricing_returns_configured_prices() {
    let cfg = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "test".into(),
            provider: "openai".into(),
            base_url: "http://localhost:1234".into(),
            api_key: "sk-test".into(),
            model: "gpt-4".into(),
            weight: 100,
            temperature: 0.7,
            input_price: 3.0,
            output_price: 15.0,
        }],
        ..Default::default()
    };
    let router = LLMRouter::from_config(&cfg).unwrap();
    let pricing = router.pricing("gpt-4");
    assert!(pricing.is_some());
    let (input, output) = pricing.unwrap();
    assert_eq!(input, 3.0);
    assert_eq!(output, 15.0);
}

#[test]
fn router_pricing_none_when_zero() {
    let cfg = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "test".into(),
            provider: "openai".into(),
            base_url: "http://localhost:1234".into(),
            api_key: "sk-test".into(),
            model: "gpt-4".into(),
            weight: 100,
            temperature: 0.7,
            input_price: 0.0,
            output_price: 0.0,
        }],
        ..Default::default()
    };
    let router = LLMRouter::from_config(&cfg).unwrap();
    assert!(router.pricing("gpt-4").is_none());
}

#[test]
fn router_pricing_none_when_zero_prices() {
    let config = LLMConfig {
        providers: vec![ProviderEndpoint {
            name: "free".into(),
            provider: "openai".into(),
            base_url: "https://api.example.com".into(),
            api_key: "key".into(),
            model: "free-model".into(),
            weight: 100,
            temperature: 0.7,
            input_price: 0.0,
            output_price: 0.0,
        }],
        ..Default::default()
    };
    let router = LLMRouter::from_config(&config).expect("router build");
    assert!(router.pricing("free-model").is_none());
}
