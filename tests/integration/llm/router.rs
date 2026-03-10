use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use bendclaw::llm::config::LLMConfig;
use bendclaw::llm::config::ProviderEndpoint;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::router::LLMRouter;
use bendclaw::llm::stream::StreamEvent;
use tokio_stream::StreamExt;

// ── LLMRouter from empty config ──

#[tokio::test]
async fn router_empty_config_returns_error() -> Result<()> {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).context("router build")?;

    let result = router
        .chat("model", &[ChatMessage::user("hi")], &[], 0.7)
        .await;
    assert!(result.is_err());
    let Err(e) = result else {
        bail!("expected error");
    };
    assert!(e.message.contains("no LLM providers"));
    Ok(())
}

#[tokio::test]
async fn router_empty_config_stream_returns_error() -> Result<()> {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).context("router build")?;

    let mut stream = router.chat_stream("model", &[ChatMessage::user("hi")], &[], 0.7);
    let event = stream.next().await.context("expected stream event")?;
    match event {
        StreamEvent::Error(msg) => assert!(msg.contains("no LLM providers")),
        _ => bail!("expected Error event"),
    }
    Ok(())
}

// ── LLMRouter defaults ──

#[test]
fn router_empty_default_model() -> Result<()> {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).context("router build")?;
    assert_eq!(router.default_model(), "unknown");
    Ok(())
}

#[test]
fn router_empty_default_temperature() -> Result<()> {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).context("router build")?;
    assert!((router.default_temperature() - 0.7).abs() < f32::EPSILON);
    Ok(())
}

#[test]
fn router_empty_pricing() -> Result<()> {
    let config = LLMConfig::default();
    let router = LLMRouter::from_config(&config).context("router build")?;
    assert!(router.pricing("any").is_none());
    Ok(())
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
fn router_default_model_from_highest_weight() -> Result<()> {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).context("router build")?;
    assert_eq!(router.default_model(), "gpt-4");
    Ok(())
}

#[test]
fn router_default_temperature_from_highest_weight() -> Result<()> {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).context("router build")?;
    assert!((router.default_temperature() - 0.5).abs() < f32::EPSILON);
    Ok(())
}

#[test]
fn router_pricing_by_model() -> Result<()> {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).context("router build")?;

    let (input, output) = router.pricing("gpt-4").context("expected gpt-4 pricing")?;
    assert!((input - 30.0).abs() < f64::EPSILON);
    assert!((output - 60.0).abs() < f64::EPSILON);

    let (input, output) = router
        .pricing("gpt-3.5")
        .context("expected gpt-3.5 pricing")?;
    assert!((input - 0.5).abs() < f64::EPSILON);
    assert!((output - 1.5).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn router_pricing_unknown_model_falls_back_to_first() -> Result<()> {
    let config = make_config_with_providers();
    let router = LLMRouter::from_config(&config).context("router build")?;

    let (input, output) = router
        .pricing("unknown-model")
        .context("expected fallback pricing")?;
    assert!((input - 30.0).abs() < f64::EPSILON);
    assert!((output - 60.0).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn empty_config_creates_router() -> Result<()> {
    let cfg = LLMConfig::default();
    let router = LLMRouter::from_config(&cfg)?;
    assert_eq!(router.default_model(), "unknown");
    assert_eq!(router.default_temperature(), 0.7);
    Ok(())
}

#[test]
fn router_with_provider() -> Result<()> {
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
    let router = LLMRouter::from_config(&cfg)?;
    assert_eq!(router.default_model(), "gpt-4");
    assert_eq!(router.default_temperature(), 0.5);
    Ok(())
}

#[test]
fn router_pricing_returns_configured_prices() -> Result<()> {
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
    let router = LLMRouter::from_config(&cfg)?;
    let (input, output) = router.pricing("gpt-4").context("expected pricing")?;
    assert_eq!(input, 3.0);
    assert_eq!(output, 15.0);
    Ok(())
}

#[test]
fn router_pricing_none_when_zero() -> Result<()> {
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
    let router = LLMRouter::from_config(&cfg)?;
    assert!(router.pricing("gpt-4").is_none());
    Ok(())
}

#[test]
fn router_pricing_none_when_zero_prices() -> Result<()> {
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
    let router = LLMRouter::from_config(&config).context("router build")?;
    assert!(router.pricing("free-model").is_none());
    Ok(())
}
