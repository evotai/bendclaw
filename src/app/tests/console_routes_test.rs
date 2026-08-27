//! Exercises the console's HTTP surface against the real router: the page
//! documents, their static assets, and the split models/channel APIs.
//!
//! These run through `Server::router`, so a route that is declared but not
//! reachable (or an asset served with the wrong content type, which a browser
//! silently refuses to execute as a module) fails here rather than in a browser.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use evot::agent::Agent;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::gateway::channels::http::Server;
use evot::storage::MemoryStorage;
use evot_engine::provider::ProviderError;
use evot_engine::provider::StreamConfig;
use evot_engine::provider::StreamEvent;
use evot_engine::provider::StreamOutcome;
use evot_engine::provider::StreamProvider;
use evot_engine::ThinkingLevel;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct BlockingChatProvider {
    started: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl StreamProvider for BlockingChatProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> std::result::Result<StreamOutcome, ProviderError> {
        self.started.store(true, Ordering::SeqCst);
        cancel.cancelled().await;
        self.cancelled.store(true, Ordering::SeqCst);
        Err(ProviderError::Cancelled)
    }
}

fn test_config() -> Config {
    test_config_at(std::env::temp_dir().join("console_routes_test.env"))
}

fn test_config_at(env_path: std::path::PathBuf) -> Config {
    let mut config = Config::new(std::env::temp_dir());
    config
        .providers
        .insert("anthropic".into(), ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "sk-test-key-abcd".into(),
            base_url: "https://api.anthropic.com".into(),
            models: vec!["claude-sonnet-4-6".into()],
            compat_caps: Default::default(),
            route_capabilities: Default::default(),
            thinking_level: None,
            context_window: None,
            max_tokens: None,
            supports_image: None,
        });
    config.providers.insert("deepseek".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "sk-deepseek-test".into(),
        base_url: "https://api.deepseek.com".into(),
        models: vec!["deepseek-chat".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "anthropic".into();
    config.env_file_path = env_path;
    config
}

/// A router whose saves land in their own env file, so POST tests do not
/// interfere with each other or leave shared state behind.
fn router_with_env(tag: &str) -> TestResult2<(axum::Router, std::path::PathBuf)> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("console_routes_{tag}_{nanos}.env"));
    let config = test_config_at(path.clone());
    let agent = Agent::new(&config, "/work")?;
    Ok((Server::new(agent, config).router(), path))
}

fn router() -> TestResult2<axum::Router> {
    let config = test_config();
    // `Agent::new` already hands back an Arc.
    let agent = Agent::new(&config, "/work")?;
    Ok(Server::new(agent, config).router())
}

type TestResult2<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn get(path: &str) -> TestResult2<(StatusCode, String, String)> {
    read(router()?, Request::builder().uri(path).body(Body::empty())?).await
}

async fn post(
    app: axum::Router,
    path: &str,
    body: serde_json::Value,
) -> TestResult2<(StatusCode, String, String)> {
    read(
        app,
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?,
    )
    .await
}

async fn read(
    app: axum::Router,
    request: Request<Body>,
) -> TestResult2<(StatusCode, String, String)> {
    let response = app.oneshot(request).await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024).await?;
    Ok((status, content_type, String::from_utf8_lossy(&bytes).into()))
}

#[tokio::test]
async fn console_pages_are_served() -> TestResult {
    for path in ["/models", "/feishu", "/chat"] {
        let (status, content_type, body) = get(path).await?;
        assert_eq!(status, StatusCode::OK, "{path} should be reachable");
        assert!(
            content_type.starts_with("text/html"),
            "{path} content-type was {content_type}"
        );
        // Each page is a shell host: it must pull in the shared stylesheet and
        // its own module, or it renders unstyled and inert.
        assert!(body.contains("/ui/app.css"), "{path} missing stylesheet");
        assert!(
            body.contains("type=\"module\""),
            "{path} missing module script"
        );
    }
    Ok(())
}

#[tokio::test]
async fn console_assets_carry_executable_content_types() -> TestResult {
    // A module served as text/plain is refused by the browser, so the header is
    // as load-bearing as the body here.
    let (status, content_type, body) = get("/ui/app.css").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type.starts_with("text/css"),
        "css was {content_type}"
    );
    assert!(body.contains("--page"));
    assert!(body.contains("--font"));
    assert!(body.contains(".load-bar"), "missing the top refresh bar");
    assert!(
        body.contains(".sk ") || body.contains(".sk{"),
        "missing skeleton shimmer"
    );

    for path in ["/ui/app.js", "/ui/models.js", "/ui/feishu.js"] {
        let (status, content_type, body) = get(path).await?;
        assert_eq!(status, StatusCode::OK, "{path} should be reachable");
        assert!(
            content_type.starts_with("text/javascript"),
            "{path} content-type was {content_type}"
        );
        assert!(!body.is_empty(), "{path} was empty");
    }
    Ok(())
}

#[tokio::test]
async fn page_modules_only_import_served_paths() -> TestResult {
    // The pages are unbundled ES modules, so every import must resolve to a
    // route. A typo would only surface as a blank page in a browser.
    for path in ["/ui/models.js", "/ui/feishu.js"] {
        let (_, _, body) = get(path).await?;
        assert!(
            body.contains("from \"./app.js\""),
            "{path} should import the shared helpers"
        );
    }
    Ok(())
}

#[tokio::test]
async fn settings_url_redirects_to_models() -> TestResult {
    let response = router()?
        .oneshot(Request::builder().uri("/settings").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok()),
        Some("/models")
    );
    Ok(())
}

#[test]
fn models_page_has_only_cloud_and_custom_sections() {
    let html = include_str!("../src/gateway/channels/http/static/ui/models.html");
    let js = include_str!("../src/gateway/channels/http/static/ui/models.js");

    // These are the only top-level concepts on the page. Global thinking is a
    // compact header control rather than a third configuration section.
    assert!(js.contains("<h2>Cloud</h2>"));
    assert!(js.contains("<h2>Custom</h2>"));
    assert_eq!(js.matches("<section class=\"model-section\">").count(), 2);
    assert!(html.contains(".cloud-list"));
    assert!(html.contains(".custom-editor"));
    assert!(js.contains("id=\"globalThinking\""));

    // Cloud entries are account-owned rows, never editor inputs or members of
    // the POST payload. Only custom providers pass through this filter.
    assert!(js.contains("const customProviders = () =>"));
    assert!(js.contains("!isCloud(provider)"));
    assert!(js.contains("providers: customProviders().map"));
    assert!(js.contains("const defaultName = () => state.active_provider || \"\";"));
    assert!(!js.contains("const defaultName = () => state.providers[0]"));
    assert!(js.contains("state.active_provider = nextName"));
    assert!(js.contains("state.active_provider === removed.name"));
    assert!(!js.contains("providerKey\" value=\""));

    // The Cloud directory groups by catalog tier through the shared label
    // helper; per-protocol provider names only ride the chip payload.
    assert!(js.contains("tierLabel(group.tier)"));
    assert!(js.contains("data-cloud-model"));
    assert!(js.contains("data-cloud-provider"));
    assert!(js.contains("active_model: pendingCloudModel"));
}

#[tokio::test]
async fn models_api_masks_secrets() -> TestResult {
    // The page renders from this payload, so a raw key here would put the secret
    // into the browser and into any HAR or devtools capture.
    let (status, content_type, body) = get("/api/models").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("application/json"));
    assert!(!body.contains("sk-test-key-abcd"), "raw api key leaked");

    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["active_provider"], "anthropic");
    let provider = &json["providers"][0];
    assert_eq!(provider["name"], "anthropic");
    assert_eq!(provider["api_key_set"], true);
    assert_eq!(provider["cloud"], false);
    assert!(
        provider["api_key_hint"]
            .as_str()
            .is_some_and(|hint| hint.starts_with("****")),
        "hint should be masked, got {:?}",
        provider["api_key_hint"]
    );
    // The page builds its selects from these, so an empty list would render a
    // form that cannot be filled in.
    assert!(json["protocols"].as_array().is_some_and(|a| !a.is_empty()));
    assert!(json["thinking_levels"]
        .as_array()
        .is_some_and(|a| !a.is_empty()));
    Ok(())
}

#[tokio::test]
async fn models_and_channel_apis_are_separate() -> TestResult {
    // The split is the point: each page fetches only its own section, so
    // neither payload should carry the other's fields.
    let (_, _, models) = get("/api/models").await?;
    let models: serde_json::Value = serde_json::from_str(&models)?;
    assert!(
        models.get("feishu").is_none(),
        "models leaked channel config"
    );

    let (status, _, channel) = get("/api/channels/feishu").await?;
    assert_eq!(status, StatusCode::OK);
    let channel: serde_json::Value = serde_json::from_str(&channel)?;
    assert!(
        channel.get("providers").is_none(),
        "channel leaked provider config"
    );
    // Unconfigured is a null channel rather than a missing key, so the page can
    // tell "not linked" from a malformed response.
    assert!(channel["feishu"].is_null());
    Ok(())
}

#[tokio::test]
async fn saving_models_leaves_the_channel_alone() -> TestResult {
    // The isolation the split exists for: the Models page sends no channel
    // fields, so a save must not clear a linked bot. Under the old combined
    // endpoint an absent `feishu` was indistinguishable from "unlink".
    let (app, env_path) = router_with_env("models_save")?;
    let (status, _, _) = post(
        app.clone(),
        "/api/channels/feishu",
        serde_json::json!({
            "app_id": "cli_app",
            "app_secret": "feishu-secret",
            "mention_only": true,
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) = post(
        app.clone(),
        "/api/models",
        serde_json::json!({
            "active_provider": "anthropic",
            "thinking_level": "high",
            "providers": [{
                "name": "anthropic",
                "protocol": "anthropic",
                "base_url": "https://api.anthropic.com",
                "models": ["claude-sonnet-4-6"],
                "api_key": serde_json::Value::Null,
                "thinking_level": serde_json::Value::Null,
            }],
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "models save failed: {body}");

    let (_, _, channel) = read(
        app,
        Request::builder()
            .uri("/api/channels/feishu")
            .body(Body::empty())?,
    )
    .await?;
    let channel: serde_json::Value = serde_json::from_str(&channel)?;
    assert_eq!(channel["feishu"]["app_id"], "cli_app");
    assert_eq!(channel["feishu"]["app_secret_set"], true);

    let written = std::fs::read_to_string(&env_path)?;
    assert!(written.contains("EVOT_CHANNEL_FEISHU_APP_ID=cli_app"));
    assert!(written.contains("EVOT_LLM_THINKING_LEVEL=high"));
    let _ = std::fs::remove_file(&env_path);
    Ok(())
}

#[tokio::test]
async fn saving_the_channel_leaves_models_alone() -> TestResult {
    // The mirror case: the Feishu page sends no provider list, which must not be
    // read as "delete every provider".
    let (app, env_path) = router_with_env("channel_save")?;
    let (status, _, body) = post(
        app.clone(),
        "/api/channels/feishu",
        serde_json::json!({
            "app_id": "cli_app",
            "app_secret": "feishu-secret",
            "mention_only": false,
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "channel save failed: {body}");
    // Persisting is not enough to run the bot: it is spawned at startup.
    let parsed: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(parsed["restart_required"], true);

    let (_, _, models) = read(
        app,
        Request::builder().uri("/api/models").body(Body::empty())?,
    )
    .await?;
    let models: serde_json::Value = serde_json::from_str(&models)?;
    assert_eq!(models["active_provider"], "anthropic");
    assert_eq!(models["providers"][0]["name"], "anthropic");
    assert_eq!(models["providers"][0]["api_key_set"], true);
    let _ = std::fs::remove_file(&env_path);
    Ok(())
}

#[tokio::test]
async fn blank_secrets_keep_the_stored_values() -> TestResult {
    // The pages send null when a secret field is left empty, since they only
    // ever receive a masked hint and could not echo the real value back.
    let (app, env_path) = router_with_env("blank_secret")?;
    post(
        app.clone(),
        "/api/channels/feishu",
        serde_json::json!({
            "app_id": "cli_app",
            "app_secret": "feishu-secret",
            "mention_only": true,
        }),
    )
    .await?;

    let (status, _, _) = post(
        app.clone(),
        "/api/channels/feishu",
        serde_json::json!({
            "app_id": "cli_app",
            "app_secret": serde_json::Value::Null,
            "mention_only": false,
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let written = std::fs::read_to_string(&env_path)?;
    assert!(
        written.contains("EVOT_CHANNEL_FEISHU_APP_SECRET=feishu-secret"),
        "blank secret cleared the stored one"
    );
    assert!(written.contains("EVOT_CHANNEL_FEISHU_MENTION_ONLY=false"));
    let _ = std::fs::remove_file(&env_path);
    Ok(())
}

#[tokio::test]
async fn an_invalid_save_is_rejected_with_a_reason() -> TestResult {
    // The page surfaces `error` verbatim, so it has to say what was wrong
    // rather than just failing the request.
    let (app, env_path) = router_with_env("invalid_save")?;
    let (status, _, body) = post(
        app,
        "/api/models",
        serde_json::json!({
            "active_provider": "ghost",
            "providers": [],
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let parsed: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(parsed["ok"], false);
    assert!(
        parsed["error"]
            .as_str()
            .is_some_and(|e| e.contains("ghost")),
        "error should name the bad provider, got {:?}",
        parsed["error"]
    );
    let _ = std::fs::remove_file(&env_path);
    Ok(())
}

/// The full catalog-default chain: cache default `max` → active_llm → the
/// options payload the composer renders. Guards against a clamp or startup
/// path silently downgrading the selection (e.g. Max → Xhigh → chip fallback).
// Holding env_lock across the requests IS the point: HOME must stay pinned
// while handlers reload config from disk.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn chat_options_current_thinking_honors_the_catalog_default() -> TestResult {
    let _guard = crate::conf_load_test::env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home =
        std::env::temp_dir().join(format!("evot-chat-options-thinking-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    let root = env_home.join(".evotai");
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("auth.json"),
        r#"{"version":1,"server_base_url":"http://localhost:8787",
            "user":{"id":"u1","name":"bo","email":"bo@test.dev"},
            "cli_token":"tok","refresh_token":"ref","models_synced_at":0}"#,
    )?;
    std::fs::write(
        root.join("models.cache.json"),
        r#"{"synced_at":1,"response":{"version":3,
          "providers":[{"name":"evot-free","protocol":"anthropic",
            "base_url":"http://localhost:8787/v1/llm","api_key":"evot.key",
            "default_model":"glm-5.3-flash","models":["glm-5.3-flash"]}],
          "models":[{"id":"glm-5.3-flash","display_name":"GLM 5.3 Flash",
            "protocol":"anthropic","tier":"base","thinking_level":"max"}],
          "notices":[]}}"#,
    )?;
    std::env::set_var("HOME", &env_home);
    let loaded = Config::load();
    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    let mut config = loaded?;
    config.env_file_path = std::env::temp_dir().join("chat-options-thinking.env");

    // Startup resolution itself must land on Max.
    assert_eq!(config.active_llm()?.thinking_level, ThinkingLevel::Max);

    let agent = Agent::new(&config, "/work")?;
    let app = Server::new(agent, config).router();
    let (status, _, body) = read(
        app,
        Request::builder()
            .uri("/api/chat/options")
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["current"]["thinking_level"], "max");
    assert_eq!(
        json["providers"][0]["models"][0]["thinking_levels"],
        serde_json::json!(["low", "high", "max"])
    );
    // The per-model default the composer preselects on a manual switch.
    assert_eq!(
        json["providers"][0]["models"][0]["default_thinking_level"],
        "max"
    );
    // The composer groups cloud providers by tier, same as the Models page.
    assert_eq!(json["providers"][0]["tier"], "base");
    let _ = std::fs::remove_dir_all(&env_home);
    Ok(())
}

#[tokio::test]
async fn chat_page_uses_conversation_layout_and_runtime_controls() -> TestResult {
    let (_, _, body) = get("/chat").await?;
    for id in [
        "id=\"chat\"",
        "id=\"input\"",
        "id=\"sendBtn\"",
        "id=\"commandBtn\"",
        "id=\"commandMenu\"",
        "id=\"recentSessions\"",
        "id=\"modelSelect\"",
        "id=\"modelMenu\"",
        "id=\"thinkingSelect\"",
        "id=\"thinkingMenu\"",
        "id=\"workspaceChip\"",
        "id=\"contextMeter\"",
        "id=\"contextPanel\"",
        "id=\"statsLine\"",
        "id=\"toBottom\"",
    ] {
        assert!(body.contains(id), "chat page lost {id}");
    }
    assert!(
        body.contains("/brand/icon.png"),
        "chat page missing the tab icon"
    );
    assert!(
        body.contains("IBM+Plex+Mono"),
        "chat page should load the admin typeface"
    );
    assert!(body.contains("class=\"chat-sidebar\""));
    assert!(body.contains("class=\"composer-card\""));
    assert!(body.contains("aria-haspopup=\"listbox\""));
    assert!(body.contains("data-command=\"/clear\""));
    assert!(!body.contains("Attachments are not available yet"));
    assert!(body.contains("/ui/chat.js"), "chat page missing its module");
    assert!(
        body.contains("/ui/chat.css"),
        "chat page missing its stylesheet"
    );
    assert!(
        !body.contains("<style>"),
        "chat should not carry inline styles"
    );
    Ok(())
}

#[tokio::test]
async fn chat_assets_are_served() -> TestResult {
    let (status, content_type, body) = get("/ui/chat.css").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/css"), "was {content_type}");
    assert!(body.contains(".chat-sidebar"));
    assert!(body.contains(".composer-card"));
    // Conversation chrome the harness layout depends on.
    assert!(body.contains(".turn-status"));
    // Spinner hugs the composer: no blank line before the input capsule.
    assert!(body.contains("margin: 6px 0 4px"));
    assert!(body.contains(".context-meter"));
    assert!(body.contains(".stats-line"));
    assert!(body.contains(".msg-action"));
    assert!(body.contains(".to-bottom"));
    // Tool rows are one collapsed line with an expandable IN/OUT body.
    assert!(body.contains(".tool-row"));
    assert!(body.contains(".tool-summary"));
    assert!(body.contains(".io-label"));
    // The hero welcome clears the floating composer band.
    assert!(body.contains(".conversation.hero .welcome { padding-bottom: 240px; }"));
    // Assistant markdown covers block structure, not just inline code.
    assert!(body.contains(".code-block"));
    assert!(body.contains(".table-wrap"));
    assert!(body.contains("blockquote"));
    // Queued steering has its own bubble state until the run admits it.
    assert!(body.contains("data-pending-steering"));
    // Search results show match context, not just titles.
    assert!(body.contains(".search-snippet"));
    // Scroll-triggered page loading shows an in-list indicator, not a freeze.
    assert!(body.contains(".recent-more"));
    assert!(body.contains(".seat-menu"));
    assert!(body.contains(".seat-group-title"));
    assert!(body.contains("mark {"));

    let (status, content_type, body) = get("/ui/chat.js").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type.starts_with("text/javascript"),
        "was {content_type}"
    );
    assert!(body.contains("/api/chat/options"));
    assert!(body.contains("chooseModel"));
    assert!(body.contains("paintModelMenu"));
    assert!(body.contains("chooseModel(meta.provider, meta.model)"));
    assert!(body.contains("/api/chat/abort"));
    assert!(body.contains("/api/chat/steer"));
    assert!(body.contains("/api/workspace"));
    assert!(body.contains("/api/sessions/"));
    // Sidebar sessions: full-text search plus armed two-step delete.
    assert!(body.contains("/api/sessions/delete"));
    assert!(body.contains("loadRecentPage"));
    assert!(
        body.contains("recentRows"),
        "paged refreshes must reconcile rows in place"
    );
    assert!(body.contains("/api/sessions?full=true"));
    assert!(body.contains("armDestructive"));
    assert!(body.contains("onSearchKeydown"));
    assert!(body.contains("snippetAround"));
    assert!(body.contains("Stop generating"));
    assert!(body.contains("new AbortController()"));
    assert!(body.contains("requestAnimationFrame"));
    assert!(body.contains("followOutput = isNearBottom()"));
    assert!(body.contains("thinking_level: thinkingLevel"));
    assert!(body.contains("node.type === \"session\""));
    assert!(body.contains("kind === \"thinking\""));
    assert!(body.contains("payload.cwd = workspace.cwd"));
    // Manual model switches preselect the model's catalog default, not tier 0.
    assert!(body.contains("entry?.default_thinking_level"));
    // Turn chrome: copy/clock actions, running status, and session readings.
    assert!(body.contains("navigator.clipboard.writeText"));
    assert!(body.contains("Ran for "));
    assert!(body.contains("tok/s"));
    assert!(body.contains("showTurnStatus()"));
    assert!(body.contains("renderContextMeter"));
    assert!(body.contains("detail.stats"));
    // Tool rows classify on the engine's lowercase wire names.
    assert!(body.contains("web_fetch: \"read\""));
    assert!(body.contains("toolRowModel"));
    assert!(body.contains("assistant.toolFacts"));
    // Markdown links are restricted to web/mail schemes.
    assert!(body.contains("function safeHref"));
    assert!(body.contains("mailto:"));
    // Mid-run Enter queues onto the active turn instead of dropping the text.
    assert!(body.contains("async function steerRun"));
    assert!(body.contains("appendPendingSteering"));
    assert!(body.contains("settleAllPendingSteering"));
    assert!(body.contains("abandonPendingSteering"));
    Ok(())
}

#[tokio::test]
async fn chat_abort_is_validated_and_idempotent() -> TestResult {
    let app = router()?;
    let (status, _, body) = post(
        app.clone(),
        "/api/chat/abort",
        serde_json::json!({ "session_id": "" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["ok"],
        false
    );

    let (status, _, body) = post(
        app,
        "/api/chat/abort",
        serde_json::json!({ "session_id": "not-running" }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["ok"], true);
    assert_eq!(json["active"], false);
    Ok(())
}

#[tokio::test]
async fn chat_steer_is_validated_and_reports_no_active_run() -> TestResult {
    let app = router()?;
    for payload in [
        serde_json::json!({ "session_id": "", "message": "hi" }),
        serde_json::json!({ "session_id": "sess-1", "message": "   " }),
    ] {
        let (status, _, body) = post(app.clone(), "/api/chat/steer", payload).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body)?["ok"],
            false
        );
    }

    // No run in flight: the browser is told so and sends a normal turn instead.
    let (status, _, body) = post(
        app,
        "/api/chat/steer",
        serde_json::json!({ "session_id": "not-running", "message": "hi" }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["ok"], true);
    assert_eq!(json["active"], false);
    Ok(())
}

#[tokio::test]
async fn chat_abort_stops_an_active_http_run() -> TestResult {
    let config = test_config();
    let started = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let agent = Agent::new_with_provider_for_test(
        &config,
        "/work",
        Arc::new(MemoryStorage::new()),
        BlockingChatProvider {
            started: Arc::clone(&started),
            cancelled: Arc::clone(&cancelled),
        },
    )?;
    let session = agent.create_session("http").await?;
    let app = Server::new(Arc::clone(&agent), config).router();

    // Keep the SSE response alive without consuming it. This ensures the run
    // can only be cancelled by the explicit abort route, not receiver drop.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "message": "wait until stopped",
                    "session_id": session.session_id,
                    "provider": "anthropic",
                    "model": "claude-sonnet-4-6"
                }))?))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    tokio::time::timeout(Duration::from_secs(3), async {
        while !started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(agent.has_active_run(&session.session_id));

    let (status, _, body) = post(
        app,
        "/api/chat/abort",
        serde_json::json!({ "session_id": session.session_id }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["active"], true);

    tokio::time::timeout(Duration::from_secs(3), async {
        while !cancelled.load(Ordering::SeqCst) || agent.has_active_run(&session.session_id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    drop(response);
    Ok(())
}

#[tokio::test]
async fn chat_options_expose_the_live_configured_model() -> TestResult {
    let (status, content_type, body) = get("/api/chat/options").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("application/json"));
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["current"]["provider"], "anthropic");
    assert_eq!(json["current"]["model"], "claude-sonnet-4-6");
    assert_eq!(json["providers"][0]["name"], "anthropic");
    assert_eq!(json["providers"][0]["models"][0]["id"], "claude-sonnet-4-6");
    assert!(
        json["providers"][0]["models"][0]["thinking_levels"]
            .as_array()
            .is_some_and(|levels| !levels.is_empty()),
        "configured reasoning model should expose its resolved thinking levels"
    );
    let deepseek = json["providers"]
        .as_array()
        .and_then(|providers| {
            providers
                .iter()
                .find(|provider| provider["name"] == "deepseek")
        })
        .ok_or("deepseek missing from chat options")?;
    assert_eq!(deepseek["models"][0]["id"], "deepseek-chat");
    assert_eq!(
        deepseek["models"][0]["thinking_levels"],
        serde_json::json!([])
    );
    Ok(())
}

#[tokio::test]
async fn chat_rejects_unconfigured_runtime_selections_before_starting() -> TestResult {
    let cases = [
        (
            serde_json::json!({
                "message": "hello",
                "provider": "ghost",
                "model": "claude-sonnet-4-6"
            }),
            "provider 'ghost' is not configured",
        ),
        (
            serde_json::json!({
                "message": "hello",
                "provider": "anthropic",
                "model": "ghost-model"
            }),
            "model 'ghost-model' is not configured",
        ),
        (
            serde_json::json!({
                "message": "hello",
                "provider": "anthropic",
                "model": "claude-sonnet-4-6",
                "thinking_level": "invalid"
            }),
            "unknown thinking level",
        ),
        (
            serde_json::json!({
                "message": "hello",
                "provider": "deepseek",
                "model": "deepseek-chat",
                "thinking_level": "high"
            }),
            "thinking level 'high' is not supported",
        ),
    ];

    for (payload, expected) in cases {
        let (status, content_type, body) = post(router()?, "/api/chat", payload).await?;
        // SSE endpoints keep HTTP 200 and carry run/setup failures as events.
        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/event-stream"));
        assert!(
            body.contains("\"type\":\"error\""),
            "missing SSE error: {body}"
        );
        assert!(body.contains(expected), "expected {expected:?} in {body}");
        assert!(
            body.contains("\"type\":\"done\""),
            "stream did not close: {body}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn root_redirects_to_chat_and_the_list_page_is_gone() -> TestResult {
    // Chat is the console's front door. Old bookmarks land there; the
    // standalone sessions list page is gone with its asset, while the
    // per-session trace viewer stays reachable.
    for path in ["/", "/sessions"] {
        let response = router()?
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{path}");
        assert_eq!(
            response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok()),
            Some("/chat"),
            "{path}"
        );
    }

    let (gone, _, _) = get("/ui/sessions.js").await?;
    assert_eq!(gone, StatusCode::NOT_FOUND);

    let (status, content_type, body) = get("/sessions/abc123/trace").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"), "was {content_type}");
    assert!(body.contains("Back to chat"), "trace lost its way home");
    Ok(())
}

#[tokio::test]
async fn the_spa_bundle_is_gone() -> TestResult {
    // The bundle and its live sockets were only consumed by the deleted SPA.
    // Leaving the routes behind would keep 1.8MB of assets in the binary.
    for path in ["/assets/index.js", "/assets/index.css", "/assets/logo.svg"] {
        let (status, _, _) = get(path).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} should be gone");
    }
    Ok(())
}

#[tokio::test]
async fn trace_page_joins_the_console_shell() -> TestResult {
    // Reachable at both paths the session cards and old links use.
    for path in ["/sessions/abc123", "/sessions/abc123/trace"] {
        let (status, content_type, body) = get(path).await?;
        assert_eq!(status, StatusCode::OK, "{path} should be reachable");
        assert!(
            content_type.starts_with("text/html"),
            "{path} was {content_type}"
        );
        assert!(
            body.contains("/ui/app.css"),
            "{path} missing the shell styles"
        );
        assert!(
            body.contains("/ui/chrome.js"),
            "{path} missing the console chrome"
        );
    }
    let (_, _, body) = get("/sessions/abc123/trace").await?;
    // The page scrolls normally rather than filling the viewport like chat.
    assert!(body.contains("data-chrome=\"page\""));
    // The iframe height protocol existed only for the SPA shell.
    assert!(
        !body.contains("postHeight"),
        "trace still reports iframe height"
    );
    assert!(
        !body.contains("postMessage"),
        "trace still posts to a parent frame"
    );
    Ok(())
}

/// The Models page renders the Cloud section from `cloud_tiers` (never the
/// per-protocol provider names), and picking a chip persists both halves of
/// the default: the serving provider and the model moved to its head.
// Same as above: the lock must survive every awaited request, since GET
/// /api/models re-reads the real home directory.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn models_api_groups_cloud_by_tier_and_chip_picks_the_default() -> TestResult {
    let _guard = crate::conf_load_test::env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home =
        std::env::temp_dir().join(format!("evot-models-tier-grouping-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    let root = env_home.join(".evotai");
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("auth.json"),
        r#"{"version":1,"server_base_url":"http://localhost:8787",
            "user":{"id":"u1","name":"bo","email":"bo@test.dev"},
            "cli_token":"tok","refresh_token":"ref","models_synced_at":0}"#,
    )?;
    // Two tiers worth of data: base hosts two models under one provider, and
    // the server default is NOT the model a chip would pick.
    std::fs::write(
        root.join("models.cache.json"),
        r#"{"synced_at":1,"response":{"version":3,
          "providers":[{"name":"evot-free","protocol":"anthropic",
            "base_url":"http://localhost:8787/v1/llm","api_key":"evot.key",
            "default_model":"gpt-5.6-luna",
            "models":["gpt-5.6-luna","glm-5.3-flash"]}],
          "models":[
            {"id":"gpt-5.6-luna","display_name":"Luna","protocol":"anthropic","tier":"base",
             "sort_order":1},
            {"id":"glm-5.3-flash","display_name":"GLM 5.3 Flash","protocol":"anthropic","tier":"base",
             "thinking_level":"max","sort_order":5}],
          "notices":[]}}"#,
    )?;
    std::env::set_var("HOME", &env_home);
    // HOME stays pointed at the hermetic home until every assertion lands:
    // GET /api/models reloads config from disk and would otherwise re-read
    // the real account cache and overwrite the cloud providers.
    let config = Config::load()?;
    let mut config = config;
    config.env_file_path = std::env::temp_dir().join("models-tier-grouping.env");

    let agent = Agent::new(&config, "/work")?;
    let app = Server::new(agent, config).router();

    let (status, _, body) = read(
        app.clone(),
        Request::builder().uri("/api/models").body(Body::empty())?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let snapshot: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(snapshot["active_provider"], "evot-free");
    assert_eq!(snapshot["cloud_tiers"].as_array().map(Vec::len), Some(1));
    let group = &snapshot["cloud_tiers"][0];
    assert_eq!(group["tier"], "base");
    // Overall rank wins inside the tier: glm's 5 shows before luna's 1, even
    // though luna is the serving default (its `active` flag is unaffected).
    assert_eq!(group["models"][0]["id"], "glm-5.3-flash");
    assert_eq!(group["models"][0]["provider"], "evot-free");
    assert_eq!(group["models"][0]["active"], false);
    assert_eq!(group["models"][1]["id"], "gpt-5.6-luna");
    assert_eq!(group["models"][1]["active"], true);

    // Picking the other chip: provider stays, model becomes the head.
    let (status, _, body) = post(
        app,
        "/api/models",
        serde_json::json!({
            "active_provider": "evot-free",
            "active_model": "glm-5.3-flash",
            "providers": []
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let updated: serde_json::Value = serde_json::from_str(&body)?;
    let group = &updated["models"]["cloud_tiers"][0];
    assert_eq!(group["models"][0]["id"], "glm-5.3-flash");
    assert_eq!(group["models"][0]["active"], true);
    assert_eq!(group["models"][1]["id"], "gpt-5.6-luna");
    assert_eq!(group["models"][1]["active"], false);

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    let _ = std::fs::remove_dir_all(&env_home);
    Ok(())
}

/// A Premium account's New-chat lands on a `special` model even if the
/// leftover live selection was Free. Existing sessions keep their own stamp.
#[tokio::test]
async fn chat_new_prefers_premium_when_the_account_has_it() -> TestResult {
    let mut config = test_config();
    config
        .providers
        .insert("evot-free".into(), ProviderProfile {
            protocol: Protocol::Anthropic,
            api_key: "evot.key".into(),
            base_url: "https://api.anthropic.com".into(),
            models: vec!["gpt-5.6-luna".into()],
            compat_caps: Default::default(),
            route_capabilities: Default::default(),
            thinking_level: None,
            context_window: None,
            max_tokens: None,
            supports_image: None,
        });
    config.providers.insert("evot-pro".into(), ProviderProfile {
        protocol: Protocol::Anthropic,
        api_key: "evot.key".into(),
        base_url: "https://api.anthropic.com".into(),
        models: vec!["claude-opus-5".into(), "glm-5.3-flash".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.cloud_providers.insert("evot-free".into());
    config.cloud_providers.insert("evot-pro".into());
    config
        .cloud_model_tiers
        .insert("gpt-5.6-luna".into(), "base".into());
    config
        .cloud_model_tiers
        .insert("claude-opus-5".into(), "special".into());
    config
        .cloud_model_tiers
        .insert("glm-5.3-flash".into(), "special".into());
    // Leftover Free selection from the previous session.
    config.llm.provider = "evot-free".into();

    let agent = Agent::new(&config, "/work")?;
    let app = Server::new(agent, config).router();
    let (status, _, body) = post(app, "/api/chat/new", serde_json::json!({})).await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["provider"], "evot-pro");
    assert_eq!(json["model"], "claude-opus-5");
    Ok(())
}

/// New-chat commits a blank session up front: the response carries the id the
/// composer then owns, an optional workspace binds at creation, and bad paths
/// are rejected before any session lands.
#[tokio::test]
async fn chat_new_creates_bound_blank_sessions() -> TestResult {
    let app = router()?;
    let (status, _, body) = post(app.clone(), "/api/chat/new", serde_json::json!({})).await?;
    assert_eq!(status, StatusCode::OK);
    let first: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(first["cwd"], "/work");
    let first_id = first["session_id"]
        .as_str()
        .ok_or("missing session_id")?
        .to_string();

    let (status, _, body) = post(app.clone(), "/api/chat/new", serde_json::json!({})).await?;
    assert_eq!(status, StatusCode::OK);
    let second: serde_json::Value = serde_json::from_str(&body)?;
    let second_id = second["session_id"].as_str().ok_or("missing session_id")?;
    assert_ne!(first_id, second_id);

    // An explicit workspace binds at creation and survives a reload.
    let dir = std::env::temp_dir().join(format!("evot-chat-new-ws-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let (status, _, body) = post(
        app.clone(),
        "/api/chat/new",
        serde_json::json!({ "cwd": dir.to_string_lossy() }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let bound: serde_json::Value = serde_json::from_str(&body)?;
    let expected = std::fs::canonicalize(&dir)?.to_string_lossy().into_owned();
    assert_eq!(bound["cwd"], expected);
    let _ = std::fs::remove_dir_all(&dir);

    // Unusable directories are refused before a session is created.
    let (status, _, body) = post(
        app,
        "/api/chat/new",
        serde_json::json!({ "cwd": "/definitely-not-a-real-evot-dir" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["ok"],
        false
    );
    Ok(())
}

/// The sidebar pages lightweight rows, and New-chat drafts (zero turns)
/// never appear in any listing — resume-by-id still reaches them.
#[tokio::test]
async fn sessions_api_pages_lightweight_rows() -> TestResult {
    let config = test_config();
    let storage: Arc<dyn evot::storage::Storage> = Arc::new(MemoryStorage::new());
    let agent = Agent::new_with_provider_for_test(
        &config,
        "/work",
        storage.clone(),
        BlockingChatProvider {
            started: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
        },
    )?;
    let app = Server::new(agent, config).router();

    // New-chat drafts are persisted for the composer but stay out of lists.
    for _ in 0..3 {
        let (status, _, _) = post(app.clone(), "/api/chat/new", serde_json::json!({})).await?;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _, body) = read(
        app.clone(),
        Request::builder()
            .uri("/api/sessions?limit=10&offset=0")
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let empty: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(empty["items"].as_array().map(Vec::len), Some(0));

    // A conversation with a real turn shows up without transcript text.
    let session = evot::agent::session::Session::new(
        "seed-visible".into(),
        "/work".into(),
        "claude-sonnet-4-6".into(),
        storage.clone(),
    )
    .await?;
    session
        .write_items(vec![evot::types::TranscriptItem::User {
            text: "seed the listing".into(),
            content: vec![],
        }])
        .await?;
    let mut meta = session.meta().await;
    meta.turns = 1;
    meta.title = Some("Seeded chat".into());
    storage.save_session(meta).await?;

    // A later New-chat click invalidates the snapshot cache, exactly what a
    // real session finishing does for the running console.
    let (status, _, _) = post(app.clone(), "/api/chat/new", serde_json::json!({})).await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) = read(
        app.clone(),
        Request::builder()
            .uri("/api/sessions?limit=10&offset=0")
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let page: serde_json::Value = serde_json::from_str(&body)?;
    let items = page["items"].as_array().ok_or("missing items")?;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["session_id"], "seed-visible");
    assert!(items[0].get("search_text").is_none());

    // The full index carries text for the visible row only.
    let (status, _, body) = read(
        app,
        Request::builder()
            .uri("/api/sessions?full=true")
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let full: serde_json::Value = serde_json::from_str(&body)?;
    let items = full["items"].as_array().ok_or("missing full items")?;
    assert_eq!(items.len(), 1);
    assert!(items[0].get("search_text").is_some());
    Ok(())
}

#[tokio::test]
async fn workspace_api_validates_directories() -> TestResult {
    let (status, _, body) = get("/api/workspace").await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["cwd"], "/work");
    assert_eq!(json["label"], "work");

    let (status, _, body) = post(
        router()?,
        "/api/workspace",
        serde_json::json!({ "path": "/definitely-not-a-real-evot-dir" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["ok"], false);
    Ok(())
}

#[tokio::test]
async fn session_detail_replays_transcript_nodes() -> TestResult {
    let config = test_config();
    let agent = Agent::new_with_provider_for_test(
        &config,
        "/work",
        Arc::new(MemoryStorage::new()),
        evot_engine::provider::MockProvider::text("ok"),
    )?;
    let session = agent.create_session("http").await?;
    let loaded = agent
        .load_session(&session.session_id)
        .await?
        .ok_or("missing session")?;
    loaded
        .write_items(vec![
            evot::types::TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            evot::types::TranscriptItem::Assistant {
                content: vec![
                    evot::types::AssistantBlock::Thinking {
                        text: "plan".into(),
                        metadata: None,
                    },
                    evot::types::AssistantBlock::Text {
                        text: "done".into(),
                    },
                ],
                stop_reason: "stop".into(),
                usage: evot::types::UsageSummary::default(),
                model: "claude-sonnet-4-6".into(),
                provider: "anthropic".into(),
                timestamp: 1_700_000_000_000,
                error_message: None,
            },
            // Stats never replay as nodes, so the readings must be folded
            // server-side from the raw entries instead.
            evot::types::TranscriptStats::LlmCallCompleted(evot::types::LlmCallCompletedStats {
                turn: 1,
                attempt: 1,
                usage: evot::types::UsageSummary {
                    input: 900,
                    output: 300,
                    cache_read: 100,
                    cache_write: 0,
                },
                metrics: Some(evot::types::LlmCallMetrics {
                    duration_ms: 4_000,
                    ttfb_ms: 200,
                    ttft_ms: 400,
                    streaming_ms: 3_000,
                    chunk_count: 42,
                }),
                error: None,
                context_window: 10_000,
                stop_reason: "stop".into(),
            })
            .to_item(),
            evot::types::TranscriptStats::ToolFinished(evot::types::ToolFinishedStats {
                tool_call_id: "tc-1".into(),
                tool_name: "read".into(),
                result_tokens: 12,
                duration_ms: 80,
                is_error: false,
            })
            .to_item(),
        ])
        .await?;
    let app = Server::new(agent, config).router();
    let (status, _, body) = read(
        app,
        Request::builder()
            .uri(format!("/api/sessions/{}", session.session_id))
            .body(Body::empty())?,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json["session"]["cwd"], "/work");
    assert_eq!(json["nodes"][0]["type"], "user");
    assert_eq!(json["nodes"][0]["text"], "hello");
    assert_eq!(json["nodes"][1]["type"], "assistant");
    assert_eq!(json["nodes"][1]["blocks"][0]["kind"], "thinking");
    assert_eq!(json["nodes"][1]["blocks"][1]["kind"], "text");
    // The message clock survives replay.
    assert_eq!(json["nodes"][1]["time"], 1_700_000_000_000u64);
    assert_eq!(json["nodes"][1]["model"], "claude-sonnet-4-6");
    // Session readings folded from the Stats entries.
    assert_eq!(json["stats"]["steps"], 1);
    assert_eq!(json["stats"]["turns"], 1);
    assert_eq!(json["stats"]["llm_ms"], 4_000);
    assert_eq!(json["stats"]["tool_ms"], 80);
    assert_eq!(json["stats"]["ttft_steps"], 1);
    assert_eq!(json["stats"]["decode_tokens"], 300);
    assert_eq!(json["stats"]["usage"]["input"], 900);
    assert_eq!(json["stats"]["context"]["percent"], 13);
    Ok(())
}

/// The account endpoints under a fake HOME: signed out by default, logout is
/// idempotent, and the notices slot stays empty without a synced catalog.
// Holding env_lock across the awaits IS the point: auth.json reads/writes
// and HOME must stay pinned for the whole request run.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn auth_endpoints_report_signed_out_state() -> TestResult {
    let _guard = crate::conf_load_test::env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-console-auth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    std::fs::create_dir_all(env_home.join(".evotai"))?;
    std::env::set_var("HOME", &env_home);
    // HOME must survive every awaited request: the endpoints read auth.json.
    let result = async {
        let (status, _, body) = get("/api/auth/session").await?;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(json["logged_in"], false);

        let (status, _, body) = get("/api/notices").await?;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(json["items"].as_array().map(Vec::len), Some(0));

        let (status, _, body) =
            post(app_for_auth()?, "/api/auth/logout", serde_json::json!({})).await?;
        assert_eq!(status, StatusCode::OK, "logout body: {body}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body)?["ok"],
            true
        );

        let (status, _, body) = get("/api/auth/session").await?;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body)?;
        assert_eq!(json["logged_in"], false);
        Ok(())
    }
    .await;
    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    let _ = std::fs::remove_dir_all(&env_home);
    result
}

/// A router whose env file stays out of the shared default path.
fn app_for_auth() -> TestResult2<axum::Router> {
    let path = std::env::temp_dir().join(format!("console_auth_{}.env", std::process::id()));
    let config = test_config_at(path);
    let agent = Agent::new(&config, "/work")?;
    Ok(Server::new(agent, config).router())
}
