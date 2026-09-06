use evot::contracts::ConfigInfo;
use serde::Deserialize;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn native_result_fixtures_match_rust_readers_and_writers() -> TestResult {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../cli/tests/fixtures/contracts/native-results-legacy.json"
    ))?;
    let session: evot::types::SessionMeta = serde_json::from_value(fixtures["session"].clone())?;
    let written = serde_json::to_value(&session)?;
    // Current writers retain every historical field and its meaning. Added
    // defaulted fields are allowed by the historical persistent reader.
    for (key, value) in fixtures["session"].as_object().ok_or("session object")? {
        assert_eq!(&written[key], value);
    }
    let reread: evot::types::SessionMeta = serde_json::from_value(written)?;
    assert_eq!(reread.session_id, session.session_id);
    let queue: evot_engine::PromptQueueEntry = serde_json::from_value(fixtures["queue"].clone())?;
    assert_eq!(serde_json::to_value(queue)?, fixtures["queue"]);
    let user: evot::auth::CloudUser = serde_json::from_value(fixtures["user"].clone())?;
    assert_eq!(serde_json::to_value(user)?, fixtures["user"]);
    let login: evot::auth::LoginCodeResponse = serde_json::from_value(fixtures["login"].clone())?;
    assert_eq!(serde_json::to_value(login)?, fixtures["login"]);
    let compacted = evot::compact::orchestrator::ManualCompactionOutcome::Compacted {
        summary: "fixture".into(),
        tokens_before: 100,
        tokens_after: 20,
        messages_before: 5,
        messages_after: 2,
        context_window: 1000,
        messages_evicted: 3,
        current_run_reclaimed: 0,
        compaction_level: 3,
        used_fallback: true,
        method: None,
        remote_blob_bytes: None,
        fallback_reason: None,
    };
    assert_eq!(serde_json::to_value(compacted)?, fixtures["compaction"]);
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyConfigInfo {
    provider: String,
    protocol: String,
    env_path: String,
    has_api_key: bool,
    base_url: Option<String>,
    available_models: Vec<serde_json::Value>,
    thinking_level: String,
}

#[test]
fn config_info_contract_round_trips_shared_fixtures() -> TestResult {
    for fixture in [
        include_str!("../../../cli/tests/fixtures/contracts/config-info-legacy.json"),
        include_str!("../../../cli/tests/fixtures/contracts/config-info-current.json"),
    ] {
        let original: serde_json::Value = serde_json::from_str(fixture)?;
        let current: ConfigInfo = serde_json::from_str(fixture)?;
        let written = serde_json::to_string(&current)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&written)?,
            original
        );
        let legacy: LegacyConfigInfo = serde_json::from_str(&written)?;
        assert_eq!(legacy.provider, current.provider);
        assert_eq!(legacy.protocol, current.protocol);
        assert_eq!(legacy.env_path, current.env_path);
        assert_eq!(legacy.has_api_key, current.has_api_key);
        assert_eq!(legacy.base_url, current.base_url);
        assert_eq!(legacy.available_models, current.available_models);
        assert_eq!(legacy.thinking_level, current.thinking_level);
    }
    Ok(())
}

#[test]
fn query_event_envelope_round_trips_shared_fixtures() -> TestResult {
    let fixtures: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../cli/tests/fixtures/contracts/query-events.json"
    ))?;
    for value in fixtures.iter().take(2) {
        let event: evot::agent::RunEvent = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(event)?, *value);
    }
    // Host messages are a separate published envelope, not incomplete runs.
    assert!(serde_json::from_value::<evot::agent::RunEvent>(fixtures[2].clone()).is_err());
    assert_eq!(fixtures[2]["kind"], "host_tool_call");
    let host: evot::contracts::HostEvent = serde_json::from_value(fixtures[2].clone())?;
    assert_eq!(serde_json::to_value(host)?, fixtures[2]);
    let source = include_str!("../../../cli/addon/src/host.rs");
    assert!(source.contains("evot::contracts::HostEvent::ToolCall"));
    Ok(())
}

#[test]
fn run_payload_fixtures_match_current_rust_writer() -> TestResult {
    let historical: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../cli/tests/fixtures/contracts/run-payloads.json"
    ))?;
    let current: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../cli/tests/fixtures/contracts/run-payloads-current.json"
    ))?;
    assert_eq!(historical.len(), current.len());
    let event_source = include_str!("../src/agent/run/event.rs");
    let kinds = regex::Regex::new(r#"Self::\w+\s*\{\s*\.\.\s*\}\s*=>\s*"([a-z_]+)""#)?;
    let emitted: std::collections::BTreeSet<_> = kinds
        .captures_iter(event_source)
        .filter_map(|capture| capture.get(1).map(|kind| kind.as_str().to_string()))
        .collect();
    let covered: std::collections::BTreeSet<_> = historical
        .iter()
        .filter_map(|value| value["kind"].as_str().map(str::to_string))
        .collect();
    assert!(!emitted.is_empty());
    assert_eq!(
        emitted, covered,
        "each emitted kind needs a shared payload fixture"
    );
    for (old, expected) in historical.iter().zip(&current) {
        let event = serde_json::json!({
            "event_id": "fixture", "run_id": "run", "session_id": "session", "turn": 1,
            "created_at": "2026-01-01T00:00:00Z", "kind": old["kind"], "payload": old["payload"],
        });
        let parsed: evot::agent::RunEvent = serde_json::from_value(event)?;
        let written = serde_json::to_value(&parsed)?;
        assert_eq!(written["kind"], expected["kind"]);
        assert_eq!(
            written["payload"], expected["payload"],
            "writer drift for {}",
            old["kind"]
        );
        let reread: evot::agent::RunEvent = serde_json::from_value(written.clone())?;
        assert_eq!(serde_json::to_value(reread)?, written);
    }
    let rich: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../cli/tests/fixtures/contracts/run-payloads-rich.json"
    ))?;
    for example in rich {
        let event = serde_json::json!({
            "event_id": "fixture", "run_id": "run", "session_id": "session", "turn": 1,
            "created_at": "2026-01-01T00:00:00Z", "kind": example["kind"], "payload": example["payload"],
        });
        let parsed: evot::agent::RunEvent = serde_json::from_value(event)?;
        let written = serde_json::to_value(parsed)?;
        let reread: evot::agent::RunEvent = serde_json::from_value(written.clone())?;
        assert_eq!(serde_json::to_value(reread)?, written);
    }
    Ok(())
}

#[test]
fn config_info_writer_and_reader_use_the_contract_boundary() {
    for source in [
        include_str!("../../../cli/addon/src/run.rs"),
        include_str!("../../../cli/addon/src/fork.rs"),
    ] {
        assert!(!source.contains("abort_notify"));
        assert!(source.contains("CancellationToken"));
    }
    let addon = include_str!("../../../cli/addon/src/agent.rs");
    assert!(addon.contains("let info = evot::contracts::ConfigInfo"));
    let native = include_str!("../../../cli/src/native/index.ts");
    assert!(native.contains("decodeConfigInfo(this.raw.configInfo())"));
    assert!(!native.contains("as ConfigInfo"));
}
