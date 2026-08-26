//! Exercises the console's HTTP surface against the real router: the page
//! documents, their static assets, and the split models/channel APIs.
//!
//! These run through `Server::router`, so a route that is declared but not
//! reachable (or an asset served with the wrong content type, which a browser
//! silently refuses to execute as a module) fails here rather than in a browser.

use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use evot::agent::Agent;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::gateway::channels::http::Server;
use tower::ServiceExt;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

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

#[tokio::test]
async fn chat_page_keeps_its_own_markup_and_gains_the_shell() -> TestResult {
    // Chat's script is a classic script that captures #chat / #input at parse
    // time, so the chrome wraps the existing body rather than replacing it.
    // These ids and the chrome module must both survive for that to work.
    let (_, _, body) = get("/chat").await?;
    for id in [
        "id=\"chat\"",
        "id=\"input\"",
        "id=\"sendBtn\"",
        "id=\"sessionLabel\"",
    ] {
        assert!(body.contains(id), "chat page lost {id}");
    }
    assert!(
        body.contains("/ui/chrome.js"),
        "chat page missing console chrome"
    );
    assert!(
        body.contains("/ui/chat.css"),
        "chat page missing its stylesheet"
    );
    // The page used to ship a light theme inline; tokens now come from the
    // shared stylesheet, so no inline block should remain to fight it.
    assert!(
        !body.contains("<style>"),
        "chat page still carries an inline stylesheet"
    );
    Ok(())
}

#[tokio::test]
async fn chat_assets_are_served() -> TestResult {
    let (status, content_type, body) = get("/ui/chat.css").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/css"), "was {content_type}");
    assert!(body.contains(".chat-container"));

    let (status, content_type, body) = get("/ui/chrome.js").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type.starts_with("text/javascript"),
        "was {content_type}"
    );
    assert!(
        body.contains("from \"./app.js\""),
        "chrome should share the nav list"
    );
    Ok(())
}

#[tokio::test]
async fn root_serves_the_sessions_page() -> TestResult {
    // The React SPA used to own "/". Both it and /sessions now render the same
    // native page, so an old bookmark and the nav entry agree.
    for path in ["/", "/sessions"] {
        let (status, content_type, body) = get(path).await?;
        assert_eq!(status, StatusCode::OK, "{path} should be reachable");
        assert!(
            content_type.starts_with("text/html"),
            "{path} content-type was {content_type}"
        );
        assert!(
            body.contains("/ui/sessions.js"),
            "{path} missing its module"
        );
        assert!(
            body.contains("/ui/app.css"),
            "{path} missing the shell styles"
        );
    }

    let (status, content_type, body) = get("/ui/sessions.js").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type.starts_with("text/javascript"),
        "was {content_type}"
    );
    assert!(body.contains("from \"./app.js\""));
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
async fn vitals_reports_host_gauges() -> TestResult {
    // The Sessions header reads this once per load, replacing a 2s socket push.
    let (status, content_type, body) = get("/api/vitals").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("application/json"));
    let json: serde_json::Value = serde_json::from_str(&body)?;
    for key in [
        "cpu_percent",
        "ram_used",
        "ram_total",
        "disk_total",
        "disk_used",
    ] {
        assert!(json.get(key).is_some(), "vitals missing {key}");
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
