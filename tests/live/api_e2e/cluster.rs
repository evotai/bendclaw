use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::base::ErrorCode;
use bendclaw::base::Role;
use bendclaw::client::NodeEntry;
use bendclaw::config::ClusterConfig;
use bendclaw::kernel::cluster::ClusterOptions;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::message::ToolCall;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use serde_json::Value;

use crate::common::fake_cluster::FakeClusterRegistry;
use crate::common::setup::require_api_config;
use crate::common::setup::spawn_test_node;
use crate::common::setup::uid;
use crate::common::setup::TestContext;
use crate::common::setup::TestNodeOptions;
use crate::common::tracing;
use crate::mocks::llm::MockLLMProvider;
use crate::mocks::reactive_llm::ReactiveMockLLMProvider;

fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    }
}

fn text_response(text: impl Into<String>) -> bendclaw::base::Result<LLMResponse> {
    Ok(LLMResponse {
        content: Some(text.into()),
        tool_calls: Vec::new(),
        finish_reason: Some("stop".into()),
        usage: None,
        model: Some("mock".into()),
    })
}

fn tool_response(tool_calls: Vec<ToolCall>) -> bendclaw::base::Result<LLMResponse> {
    Ok(LLMResponse {
        content: None,
        tool_calls,
        finish_reason: Some("tool_calls".into()),
        usage: None,
        model: Some("mock".into()),
    })
}

fn parse_tool_json_values(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .filter_map(|message| serde_json::from_str::<Value>(&message.text()).ok())
        .collect()
}

fn latest_peer_nodes(messages: &[ChatMessage]) -> bendclaw::base::Result<Vec<NodeEntry>> {
    parse_tool_json_values(messages)
        .into_iter()
        .rev()
        .find_map(|value| serde_json::from_value::<Vec<NodeEntry>>(value).ok())
        .ok_or_else(|| ErrorCode::llm_request("cluster_nodes output missing from tool history"))
}

fn latest_dispatch_ids(messages: &[ChatMessage]) -> bendclaw::base::Result<Vec<String>> {
    let dispatch_ids: Vec<String> = parse_tool_json_values(messages)
        .into_iter()
        .filter_map(|value| {
            value
                .get("dispatch_id")
                .and_then(|dispatch_id| dispatch_id.as_str())
                .map(ToOwned::to_owned)
        })
        .collect();

    if dispatch_ids.is_empty() {
        return Err(ErrorCode::llm_request(
            "cluster_dispatch output missing from tool history",
        ));
    }
    Ok(dispatch_ids)
}

fn latest_collect_entries(messages: &[ChatMessage]) -> bendclaw::base::Result<Vec<Value>> {
    parse_tool_json_values(messages)
        .into_iter()
        .rev()
        .find_map(|value| value.as_array().cloned())
        .ok_or_else(|| ErrorCode::llm_request("cluster_collect output missing from tool history"))
}

fn collect_entries_from_events(events: &[Value]) -> Result<Vec<Value>> {
    let output = events
        .iter()
        .find(|event| {
            event["event"] == "ToolEnd" && event["payload"]["data"]["name"] == "cluster_collect"
        })
        .and_then(|event| event["payload"]["data"]["output"].as_str())
        .context("cluster_collect ToolEnd output missing")?;
    let value: Value = serde_json::from_str(output)?;
    value
        .as_array()
        .cloned()
        .context("cluster_collect output should be an array")
}

fn coordinator_llm(worker_agents: Vec<String>) -> Arc<dyn LLMProvider> {
    Arc::new(ReactiveMockLLMProvider::new(
        move |call_index, messages, _tools, _temperature| match call_index {
            0 => tool_response(vec![tool_call(
                "tc_nodes",
                "cluster_nodes",
                serde_json::json!({}),
            )]),
            1 => {
                let mut peers = latest_peer_nodes(messages)?;
                peers.sort_by(|a, b| a.node_id.cmp(&b.node_id));

                if peers.len() < worker_agents.len() {
                    return Err(ErrorCode::llm_request(format!(
                        "expected {} peer nodes, got {}",
                        worker_agents.len(),
                        peers.len()
                    )));
                }

                let calls = peers
                    .into_iter()
                    .zip(worker_agents.iter())
                    .enumerate()
                    .map(|(index, (peer, agent_id))| {
                        tool_call(
                            &format!("tc_dispatch_{index}"),
                            "cluster_dispatch",
                            serde_json::json!({
                                "node_id": peer.node_id,
                                "agent_id": agent_id,
                                "task": format!("solve subtask for {agent_id}")
                            }),
                        )
                    })
                    .collect();
                tool_response(calls)
            }
            2 => {
                let dispatch_ids = latest_dispatch_ids(messages)?;
                tool_response(vec![tool_call(
                    "tc_collect",
                    "cluster_collect",
                    serde_json::json!({
                        "dispatch_ids": dispatch_ids,
                        "timeout_secs": 5
                    }),
                )])
            }
            3 => {
                let mut entries = latest_collect_entries(messages)?;
                entries.sort_by(|a, b| a["agent_id"].as_str().cmp(&b["agent_id"].as_str()));
                let summary = entries
                    .into_iter()
                    .map(|entry| {
                        format!(
                            "{}={}",
                            entry["agent_id"].as_str().unwrap_or("unknown"),
                            entry["output"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                text_response(format!("cluster summary: {summary}"))
            }
            _ => text_response("cluster summary complete"),
        },
    ))
}

async fn setup_agent_http(
    base_url: &str,
    auth_key: &str,
    agent_id: &str,
    user_id: &str,
) -> Result<()> {
    let response = reqwest::Client::new()
        .post(format!("{base_url}/v1/agents/{agent_id}/setup"))
        .bearer_auth(auth_key)
        .header("x-user-id", user_id)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    anyhow::ensure!(status.is_success(), "setup_agent failed: {status} {body}");
    Ok(())
}

async fn create_run_http(
    base_url: &str,
    auth_key: &str,
    agent_id: &str,
    user_id: &str,
    session_id: &str,
    input: &str,
) -> Result<Value> {
    let response = reqwest::Client::new()
        .post(format!("{base_url}/v1/agents/{agent_id}/runs"))
        .bearer_auth(auth_key)
        .header("x-user-id", user_id)
        .json(&serde_json::json!({
            "session_id": session_id,
            "input": input,
            "stream": false
        }))
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    anyhow::ensure!(status.is_success(), "create_run failed: {status} {body}");
    Ok(serde_json::from_str(&body)?)
}

async fn get_run_http(
    base_url: &str,
    auth_key: &str,
    agent_id: &str,
    user_id: &str,
    run_id: &str,
) -> Result<Value> {
    let response = reqwest::Client::new()
        .get(format!("{base_url}/v1/agents/{agent_id}/runs/{run_id}"))
        .bearer_auth(auth_key)
        .header("x-user-id", user_id)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    anyhow::ensure!(status.is_success(), "get_run failed: {status} {body}");
    Ok(serde_json::from_str(&body)?)
}

async fn wait_for_registry_size(
    registry: &FakeClusterRegistry,
    expected: usize,
    timeout: Duration,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if registry.snapshot().len() == expected {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "registry size did not reach {expected}; current={}",
                registry.snapshot().len()
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn cluster_multi_agent_collaboration_e2e() -> Result<()> {
    tracing::init();
    let ctx = TestContext::setup().await?;
    let (api_base_url, api_token, warehouse) = require_api_config()?;
    let registry_token = "cluster-registry-token";
    let auth_key = "cluster-node-auth";
    let registry = FakeClusterRegistry::start(registry_token).await?;
    let cluster_options = ClusterOptions {
        heartbeat_interval: Duration::from_secs(1),
        dispatch_poll_interval: Duration::from_millis(50),
    };

    let coordinator_agent = uid("coord");
    let worker_agent_a = uid("wka");
    let user_id = uid("usr");
    let session_id = uid("sess");

    let cluster_config = || {
        Some(ClusterConfig {
            registry_url: registry.base_url().to_string(),
            registry_token: registry_token.to_string(),
            advertise_url: String::new(),
        })
    };

    let worker_llm_a: Arc<dyn LLMProvider> =
        Arc::new(MockLLMProvider::with_text("result-from-worker-a"));
    let coordinator = spawn_test_node(TestNodeOptions {
        root_pool: ctx.root_pool(),
        api_base_url: api_base_url.clone(),
        api_token: api_token.clone(),
        warehouse: warehouse.clone(),
        db_prefix: ctx.prefix().to_string(),
        node_id: "node-coordinator".to_string(),
        auth_key: auth_key.to_string(),
        llm: coordinator_llm(vec![worker_agent_a.clone()]),
        cluster: cluster_config(),
        cluster_options,
    })
    .await?;
    let worker_a = spawn_test_node(TestNodeOptions {
        root_pool: ctx.root_pool(),
        api_base_url: api_base_url.clone(),
        api_token: api_token.clone(),
        warehouse: warehouse.clone(),
        db_prefix: ctx.prefix().to_string(),
        node_id: "node-worker-a".to_string(),
        auth_key: auth_key.to_string(),
        llm: worker_llm_a,
        cluster: cluster_config(),
        cluster_options,
    })
    .await?;
    wait_for_registry_size(&registry, 2, Duration::from_secs(5)).await?;

    let result = async {
        tokio::try_join!(
            setup_agent_http(
                &coordinator.base_url,
                auth_key,
                &coordinator_agent,
                &user_id,
            ),
            setup_agent_http(&worker_a.base_url, auth_key, &worker_agent_a, &user_id),
        )?;
        let coordinator_run = create_run_http(
            &coordinator.base_url,
            auth_key,
            &coordinator_agent,
            &user_id,
            &session_id,
            "coordinate across workers",
        )
        .await?;

        let coordinator_run_id = coordinator_run["id"]
            .as_str()
            .context("coordinator run id missing")?
            .to_string();
        assert_eq!(coordinator_run["status"], "COMPLETED");
        let output = coordinator_run["output"]
            .as_str()
            .context("coordinator output missing")?;
        assert!(output.contains(&format!("{worker_agent_a}=result-from-worker-a")));

        let coordinator_detail = get_run_http(
            &coordinator.base_url,
            auth_key,
            &coordinator_agent,
            &user_id,
            &coordinator_run_id,
        )
        .await?;
        let events = coordinator_detail["events"]
            .as_array()
            .context("coordinator events missing")?;
        assert!(events.iter().any(|event| {
            event["event"] == "ToolEnd" && event["payload"]["data"]["name"] == "cluster_nodes"
        }));
        assert_eq!(
            events
                .iter()
                .filter(|event| {
                    event["event"] == "ToolEnd"
                        && event["payload"]["data"]["name"] == "cluster_dispatch"
                })
                .count(),
            1
        );
        assert!(events.iter().any(|event| {
            event["event"] == "ToolEnd" && event["payload"]["data"]["name"] == "cluster_collect"
        }));

        let collect_entries = collect_entries_from_events(events)?;
        assert_eq!(collect_entries.len(), 1);
        let worker_run_id = collect_entries[0]["run_id"]
            .as_str()
            .context("worker run_id missing from cluster_collect output")?;
        let worker_run = get_run_http(
            &worker_a.base_url,
            auth_key,
            &worker_agent_a,
            &user_id,
            worker_run_id,
        )
        .await?;
        assert_eq!(
            worker_run["parent_run_id"].as_str(),
            Some(coordinator_run_id.as_str())
        );
        assert_eq!(worker_run["output"].as_str(), Some("result-from-worker-a"));
        Ok(())
    }
    .await;

    let _ = worker_a.shutdown().await;
    let _ = coordinator.shutdown().await;
    registry.shutdown().await;
    result
}

#[tokio::test]
async fn cluster_shutdown_deregisters_nodes() -> Result<()> {
    tracing::init();
    let ctx = TestContext::setup().await?;
    let (api_base_url, api_token, warehouse) = require_api_config()?;
    let registry_token = "cluster-registry-token";
    let auth_key = "cluster-node-auth";
    let registry = FakeClusterRegistry::start(registry_token).await?;

    let node = spawn_test_node(TestNodeOptions {
        root_pool: ctx.root_pool(),
        api_base_url,
        api_token,
        warehouse,
        db_prefix: ctx.prefix().to_string(),
        node_id: "node-shutdown".to_string(),
        auth_key: auth_key.to_string(),
        llm: Arc::new(MockLLMProvider::with_text("ok")),
        cluster: Some(ClusterConfig {
            registry_url: registry.base_url().to_string(),
            registry_token: registry_token.to_string(),
            advertise_url: String::new(),
        }),
        cluster_options: ClusterOptions {
            heartbeat_interval: Duration::from_millis(100),
            dispatch_poll_interval: Duration::from_millis(50),
        },
    })
    .await?;

    wait_for_registry_size(&registry, 1, Duration::from_secs(5)).await?;
    node.shutdown().await?;
    wait_for_registry_size(&registry, 0, Duration::from_secs(5)).await?;
    registry.shutdown().await;
    Ok(())
}
