use std::ffi::OsString;

use evot::auth;
use evot::conf::Config;

use crate::conf_load_test::env_lock;

fn restore_env_var(key: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn write_test_home(dir: &std::path::Path, auth: Option<&str>, cache: Option<&str>) {
    let root = dir.join(".evotai");
    std::fs::create_dir_all(&root).unwrap();
    if let Some(content) = auth {
        std::fs::write(root.join("auth.json"), content).unwrap();
    }
    if let Some(content) = cache {
        std::fs::write(root.join("models.cache.json"), content).unwrap();
    }
}

const AUTH_JSON: &str = r#"{
  "version": 1,
  "server_base_url": "http://localhost:8787",
  "user": {"id":"u1","name":"bo","email":"bo@test.dev"},
  "cli_token": "tok",
  "refresh_token": "ref",
  "models_synced_at": 0
}"#;

const CACHE_JSON: &str = r#"{
  "synced_at": 123,
  "response": {
    "version": 3,
    "providers": [
      {"name":"evot-free","protocol":"anthropic",
       "base_url":"http://localhost:8787/v1/llm","api_key":"evot.scoped.key",
       "default_model":"m-two","models":["m-one","m-two","m-three"]}
    ],
    "models": [
      {"id":"m-one","display_name":"One","protocol":"anthropic","tier":"base"},
      {"id":"m-two","display_name":"Two","protocol":"anthropic","tier":"base","thinking_level":"max"},
      {"id":"m-three","display_name":"Three","protocol":"anthropic","tier":"base"}
    ],
    "notices": []
  }
}"#;

#[test]
fn cloud_provider_registered_and_default_when_no_byok() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(CACHE_JSON));
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    let config = result.unwrap();
    let profile = config
        .providers
        .get("evot-free")
        .expect("evot-free provider registered");
    assert_eq!(profile.base_url, "http://localhost:8787/v1/llm");
    assert_eq!(profile.api_key, "evot.scoped.key");
    assert_eq!(profile.models.first().unwrap(), "m-two");
    assert_eq!(config.llm.provider, "evot-free");
    assert_eq!(
        config.cloud_thinking_levels.get("m-two").copied(),
        Some(evot_engine::ThinkingLevel::Max)
    );
    assert!(!config.cloud_thinking_levels.contains_key("m-one"));
    // Tiers land beside the levels so the console can group by them.
    assert_eq!(
        config.cloud_model_tiers.get("m-two").map(String::as_str),
        Some("base")
    );

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn cloud_provider_kept_optional_when_byok_active() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-byok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(CACHE_JSON));
    std::fs::write(
        env_home.join(".evotai/evot.toml"),
        "[providers.openrouter]\napi_key = \"sk-byok\"\nmodel = [\"byok/model\"]\n\n[llm]\nprovider = \"openrouter\"\n",
    )
    .unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    let config = result.unwrap();
    assert!(config.providers.contains_key("evot-free"));
    assert_eq!(config.llm.provider, "openrouter");

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn no_auth_file_means_no_cloud_provider() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, None, None);
    std::env::set_var("HOME", &env_home);

    let result = Config::load();

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    let config = result.unwrap();
    assert!(!config.providers.contains_key("evot-free"));

    let _ = std::fs::remove_dir_all(&env_home);
}

/// A newer server sends one provider per (tier, protocol) pair, and owns the
/// heading and ordering for each.
const MULTI_CACHE_JSON: &str = r#"{
  "synced_at": 123,
  "response": {
    "version": 7,
    "providers": [
      {"name":"evot-free","label":"Evot Free","sort_order":0,"protocol":"anthropic",
       "base_url":"http://localhost:8787/v1/llm","api_key":"evot.scoped.key",
       "default_model":"claude-free","models":["claude-free"]},
      {"name":"evot-free-openai","label":"Evot Free","sort_order":0,"protocol":"openai",
       "base_url":"http://localhost:8787/v1/llm","api_key":"evot.scoped.key",
       "default_model":"gpt-free","models":["gpt-free-mini","aardvark-openai","gpt-free"]},
      {"name":"evot-pro","label":"Evot Premium","sort_order":1,"protocol":"anthropic",
       "base_url":"http://localhost:8787/v1/llm","api_key":"evot.scoped.key",
       "default_model":"claude-pro","models":["aaa-pro","zzz-pro","claude-pro"]}
    ],
    "models": [
      {"id":"claude-free","protocol":"anthropic","tier":"base","provider":"evot-free"},
      {"id":"gpt-free","protocol":"openai","tier":"base","provider":"evot-free-openai","sort_order":2},
      {"id":"gpt-free-mini","protocol":"openai","tier":"base","provider":"evot-free-openai","sort_order":7},
      {"id":"aardvark-openai","protocol":"openai","tier":"base","provider":"evot-free-openai","sort_order":3},
      {"id":"claude-pro","protocol":"anthropic","tier":"special","provider":"evot-pro"},
      {"id":"aaa-pro","protocol":"anthropic","tier":"special","provider":"evot-pro","sort_order":5},
      {"id":"zzz-pro","protocol":"anthropic","tier":"special","provider":"evot-pro","sort_order":1}
    ],
    "notices": []
  }
}"#;

#[test]
fn each_cloud_protocol_becomes_its_own_provider() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(MULTI_CACHE_JSON));
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    restore_env_var("HOME", original_home);
    let config = result.unwrap();

    let free = config.providers.get("evot-free").expect("free registered");
    assert_eq!(free.protocol.to_string(), "anthropic");
    assert_eq!(free.models, vec!["claude-free".to_string()]);

    let compat = config
        .providers
        .get("evot-free-openai")
        .expect("openai group registered");
    assert_eq!(compat.protocol.to_string(), "openai");
    // The head is the server default; the rest preserves the catalog's own
    // rank-descending array. Ranking at merge points re-sorts by sort_order,
    // so the profile only has to stay faithful to the wire.
    assert_eq!(compat.models, vec![
        "gpt-free".to_string(),
        "gpt-free-mini".to_string(),
        "aardvark-openai".to_string()
    ]);

    let pro = config.providers.get("evot-pro").expect("pro registered");
    assert_eq!(pro.protocol.to_string(), "anthropic");
    assert_eq!(pro.models, vec![
        "claude-pro".to_string(),
        "aaa-pro".to_string(),
        "zzz-pro".to_string()
    ]);

    // Premium (`special`) wins the landing spot when the account has it.
    // Lowest sort_order is only the fallback when every group is Free.
    assert_eq!(config.llm.provider, "evot-pro");

    let _ = std::fs::remove_dir_all(&env_home);
}

/// A persisted Free selection from an old env file is not honored past a
/// Premium landing spot: the TUI footer and every new session derive from
/// this in-memory value at startup, before any session exists.
#[test]
fn persisted_free_selection_yields_to_premium_at_load() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-yield-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(MULTI_CACHE_JSON));
    std::fs::write(
        env_home.join(".evotai/evot.env"),
        "EVOT_LLM_PROVIDER=evot-free\n",
    )
    .unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    restore_env_var("HOME", original_home);
    let config = result.unwrap();

    assert_eq!(config.llm.provider, "evot-pro");

    let _ = std::fs::remove_dir_all(&env_home);
}

/// Renaming the tiers server-side must move the landing spot with them, since
/// the client ranks groups by `sort_order` and never by name.
const RENAMED_CACHE_JSON: &str = r#"{
  "synced_at": 123,
  "response": {
    "version": 9,
    "providers": [
      {"name":"tier-alpha","label":"Alpha","sort_order":7,"protocol":"anthropic",
       "base_url":"http://localhost:8787/v1/llm","api_key":"evot.scoped.key",
       "default_model":"m-alpha","models":["m-alpha"]},
      {"name":"tier-omega","label":"Omega","sort_order":2,"protocol":"openai",
       "base_url":"http://localhost:8787/v1/llm","api_key":"evot.scoped.key",
       "default_model":"m-omega","models":["m-omega"]}
    ],
    "models": [
      {"id":"m-alpha","protocol":"anthropic","tier":"base","provider":"tier-alpha"},
      {"id":"m-omega","protocol":"openai","tier":"base","provider":"tier-omega"}
    ],
    "notices": []
  }
}"#;

#[test]
fn the_landing_provider_follows_server_order_not_its_name() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-rank-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(RENAMED_CACHE_JSON));
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    restore_env_var("HOME", original_home);
    let config = result.unwrap();

    assert!(config.providers.contains_key("tier-alpha"));
    assert!(config.providers.contains_key("tier-omega"));
    // sort_order 2 wins over 7, even though neither is named like a tier.
    assert_eq!(config.llm.provider, "tier-omega");

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn cloud_group_labels_and_order_come_from_the_server() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-label-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(MULTI_CACHE_JSON));
    std::env::set_var("HOME", &env_home);

    let cache = auth::load_models_cache();
    restore_env_var("HOME", original_home);
    let cache = cache.unwrap().expect("cache present");

    let groups = cache.response.providers;
    // The server owns both the heading and the ordering.
    let free = groups
        .iter()
        .find(|g| g.name == "evot-free")
        .expect("free group");
    assert_eq!(free.label, "Evot Free");
    assert_eq!(free.sort_order, 0);

    let pro = groups
        .iter()
        .find(|g| g.name == "evot-pro")
        .expect("pro group");
    assert_eq!(pro.label, "Evot Premium");
    assert_eq!(pro.sort_order, 1);

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn cloud_models_keep_the_server_default_first() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(CACHE_JSON));
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    restore_env_var("HOME", original_home);
    let config = result.unwrap();

    let profile = config.providers.get("evot-free").expect("registered");
    assert_eq!(profile.protocol.to_string(), "anthropic");
    // default_model is m-two, so it is preselected ahead of m-one.
    assert_eq!(profile.models.first().unwrap(), "m-two");
    assert_eq!(profile.models.len(), 3);

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn auth_store_roundtrip() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, None, None);
    std::env::set_var("HOME", &env_home);

    let state: auth::AuthState = serde_json::from_str(AUTH_JSON).unwrap();
    auth::save_auth(&state).unwrap();
    let loaded = auth::load_auth().unwrap().expect("state persisted");

    assert_eq!(loaded.user.email, "bo@test.dev");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(env_home.join(".evotai/auth.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    auth::clear_auth().unwrap();
    assert!(auth::load_auth().unwrap().is_none());

    match original_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    let _ = std::fs::remove_dir_all(&env_home);
}

const STALE_ENV: &str = "\
EVOT_LLM_PROVIDER=evot-free
EVOT_LLM_EVOT_FREE_PROTOCOL=anthropic
EVOT_LLM_EVOT_FREE_BASE_URL=https://auto.evot.ai/v1/llm
EVOT_LLM_EVOT_FREE_API_KEY=evot.scoped.key
EVOT_LLM_ANTHROPIC_API_KEY=sk-byok
EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com
EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6
";

#[test]
fn load_cleans_cloud_keys_out_of_the_env_file_while_logged_in() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-reconcile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(CACHE_JSON));
    std::fs::write(env_home.join(".evotai/evot.env"), STALE_ENV).unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    let after = std::fs::read_to_string(env_home.join(".evotai/evot.env"));
    restore_env_var("HOME", original_home);

    let config = result.unwrap();
    let content = after.unwrap();

    assert!(!content.contains("evot.scoped.key"));
    assert!(!content.contains("EVOT_LLM_EVOT_FREE_BASE_URL"));
    assert!(!content.contains("EVOT_LLM_EVOT_FREE_PROTOCOL"));
    assert!(content.contains("EVOT_LLM_ANTHROPIC_API_KEY=sk-byok"));

    let free = config.providers.get("evot-free").expect("still registered");
    assert_eq!(free.base_url, "http://localhost:8787/v1/llm");
    assert_eq!(free.api_key, "evot.scoped.key");
    assert!(config.cloud_providers.contains("evot-free"));

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn load_drops_cloud_providers_left_behind_after_logout() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-orphan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, None, None);
    std::fs::write(env_home.join(".evotai/evot.env"), STALE_ENV).unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    let after = std::fs::read_to_string(env_home.join(".evotai/evot.env"));
    restore_env_var("HOME", original_home);

    let config = result.unwrap();
    assert!(!config.providers.contains_key("evot-free"));
    assert_eq!(config.llm.provider, "anthropic");
    assert!(config.active_llm().is_ok());

    let content = after.unwrap();
    assert!(!content.contains("evot.scoped.key"));
    assert!(content.contains("EVOT_LLM_ANTHROPIC_API_KEY=sk-byok"));

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn load_leaves_byok_providers_alone() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-byok-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, None, None);
    std::fs::write(
        env_home.join(".evotai/evot.env"),
        "\
EVOT_LLM_PROVIDER=evot-proxy
EVOT_LLM_EVOT_PROXY_PROTOCOL=anthropic
EVOT_LLM_EVOT_PROXY_BASE_URL=https://llm.internal.corp/anthropic
EVOT_LLM_EVOT_PROXY_API_KEY=sk-corp
EVOT_LLM_EVOT_PROXY_MODEL=corp-model
",
    )
    .unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    let after = std::fs::read_to_string(env_home.join(".evotai/evot.env"));
    restore_env_var("HOME", original_home);

    let config = result.unwrap();
    let proxy = config
        .providers
        .get("evot-proxy")
        .expect("BYOK provider kept despite its evot- prefix");
    assert_eq!(proxy.api_key, "sk-corp");
    assert_eq!(config.llm.provider, "evot-proxy");
    assert!(after
        .unwrap()
        .contains("EVOT_LLM_EVOT_PROXY_API_KEY=sk-corp"));

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn load_is_a_no_op_for_a_clean_env_file() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-noop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(CACHE_JSON));
    let env_path = env_home.join(".evotai/evot.env");
    let clean = "\
EVOT_LLM_ANTHROPIC_API_KEY=sk-byok
EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com
EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6
";
    std::fs::write(&env_path, clean).unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    let after = std::fs::read_to_string(&env_path);
    restore_env_var("HOME", original_home);

    result.unwrap();
    assert_eq!(after.unwrap(), clean);

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn logout_clears_credentials_and_cache() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-logout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, Some(AUTH_JSON), Some(CACHE_JSON));
    std::env::set_var("HOME", &env_home);

    let result = auth::logout();
    let auth_gone = auth::load_auth().map(|state| state.is_none());
    let cache_gone = auth::load_models_cache().map(|cache| cache.is_none());
    restore_env_var("HOME", original_home);

    result.expect("logout succeeds");
    assert!(auth_gone.unwrap());
    assert!(cache_gone.unwrap());

    let _ = std::fs::remove_dir_all(&env_home);
}

#[test]
fn selection_naming_a_missing_provider_falls_back() {
    let _guard = env_lock().lock().unwrap();
    let original_home = std::env::var_os("HOME");
    let env_home = std::env::temp_dir().join(format!("evot-auth-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&env_home);
    write_test_home(&env_home, None, None);
    std::fs::write(
        env_home.join(".evotai/evot.env"),
        "\
EVOT_LLM_PROVIDER=evot-free
EVOT_LLM_ANTHROPIC_API_KEY=sk-byok
EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com
EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6
",
    )
    .unwrap();
    std::env::set_var("HOME", &env_home);

    let result = Config::load();
    restore_env_var("HOME", original_home);

    let config = result.unwrap();
    assert_eq!(config.llm.provider, "anthropic");
    assert!(config.active_llm().is_ok());

    let _ = std::fs::remove_dir_all(&env_home);
}

mod wiremock_tests {
    use evot::auth;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;

    use super::*;

    #[tokio::test]
    async fn begin_login_parses_server_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/cli/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "code": "ABC123",
                "login_url": "http://x/login?code=ABC123",
                "expires_at": 123456,
                "expires_in_ms": 3600000,
                "interval_ms": 2000
            })))
            .mount(&server)
            .await;

        let resp = auth::begin_login(&server.uri(), "fp-1").await.unwrap();
        assert_eq!(resp.code, "ABC123");
        assert_eq!(resp.expires_in_ms, 3_600_000);
    }

    #[tokio::test]
    async fn poll_status_maps_success_to_auth_state() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/auth/cli/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "cli_token": "tok-1",
                "refresh_token": "ref-1",
                "user": {"id": "u1", "name": "bo", "email": "bo@test.dev"}
            })))
            .mount(&server)
            .await;

        match auth::poll_status(&server.uri(), "CODE", 42).await.unwrap() {
            auth::PollOutcome::Success { user } => {
                assert_eq!(user.cli_token, "tok-1");
                assert_eq!(user.server_base_url, server.uri());
                assert_eq!(user.user.email, "bo@test.dev");
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn poll_status_pending_202() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/auth/cli/status"))
            .respond_with(
                ResponseTemplate::new(202).set_body_json(serde_json::json!({"status": "pending"})),
            )
            .mount(&server)
            .await;

        match auth::poll_status(&server.uri(), "CODE", 42).await.unwrap() {
            auth::PollOutcome::Pending => {}
            other => panic!("expected pending, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sync_models_sends_bearer_token_and_parses_catalog() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/config/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "version": 7,
                "providers": [{
                    "name": "evot-free",
                    "protocol": "anthropic",
                    "base_url": "http://x/v1/llm",
                    "api_key": "evot.k",
                    "default_model": "m1",
                    "models": ["m1"]
                }],
                "models": [{"id": "m1", "display_name": "One",
                            "protocol": "anthropic", "tier": "base"}],
                "notices": [{"id": "n1", "kind": "notice", "title": "hi"}]
            })))
            .mount(&server)
            .await;

        let state: auth::AuthState = serde_json::from_str(
            AUTH_JSON
                .replace(
                    "\"server_base_url\": \"http://localhost:8787\"",
                    &format!("\"server_base_url\": \"{}\"", server.uri()),
                )
                .as_str(),
        )
        .unwrap();

        let response = auth::sync_models(&state).await.unwrap();
        assert_eq!(response.version, 7);
        assert_eq!(response.models.len(), 1);
        assert_eq!(response.providers[0].default_model, "m1");
        assert_eq!(response.providers[0].protocol, "anthropic");
        assert_eq!(response.notices[0].id, "n1");

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let headers = &requests[0].headers;
        assert_eq!(
            headers.get("authorization").map(|v| v.to_str().unwrap()),
            Some("Bearer tok")
        );
    }

    #[tokio::test]
    async fn sync_models_surfaces_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/config/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let state: auth::AuthState = serde_json::from_str(
            AUTH_JSON
                .replace(
                    "\"server_base_url\": \"http://localhost:8787\"",
                    &format!("\"server_base_url\": \"{}\"", server.uri()),
                )
                .as_str(),
        )
        .unwrap();
        assert!(auth::sync_models(&state).await.is_err());
    }
}
