use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::client::BendclawClient;
use bendclaw::client::ClusterClient;
use bendclaw::kernel::cluster::ClusterOptions;
use bendclaw::kernel::cluster::ClusterService;
use bendclaw::kernel::tools::builtins::cluster::ClusterCollectTool;
use bendclaw::kernel::tools::builtins::cluster::ClusterDispatchTool;
use bendclaw::kernel::tools::builtins::cluster::ClusterNodesTool;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::tools::ToolContext;
use serde_json::Value;

use crate::common::fake_cluster::FakeClusterRegistry;
use crate::common::fake_cluster::FakePeerNode;
use crate::common::fake_cluster::FakeRunPlan;
use crate::common::tracing;
use crate::mocks::context::test_tool_context;

fn make_service(registry_url: &str, auth_token: &str, node_id: &str) -> Arc<ClusterService> {
    let cluster_client = Arc::new(ClusterClient::new(
        registry_url,
        auth_token,
        node_id,
        format!("http://{node_id}.local"),
        "test-cluster",
    ));
    let bendclaw_client = Arc::new(BendclawClient::new(auth_token, Duration::from_secs(5)));
    Arc::new(ClusterService::with_options(
        cluster_client,
        bendclaw_client,
        ClusterOptions {
            heartbeat_interval: Duration::from_millis(100),
            dispatch_poll_interval: Duration::from_millis(25),
        },
    ))
}

#[tokio::test]
async fn cluster_nodes_tool_discovers_registered_peer() -> Result<()> {
    tracing::init();
    let registry = FakeClusterRegistry::start("cluster-test-token").await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        "cluster-test-token",
        "node-peer",
        "http://peer.local",
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), "cluster-test-token", "node-self");
    let tool = ClusterNodesTool::new(service);
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    assert!(result.success);

    let nodes: Vec<Value> = serde_json::from_str(&result.output)?;
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["node_id"], "node-peer");
    assert_eq!(nodes[0]["endpoint"], "http://peer.local");

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_and_collect_tools_complete_remote_run() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-test-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_complete("worker completed")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table.clone());
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let nodes_result = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    assert!(nodes_result.success);

    let dispatch_result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "do work"
            }),
            &ctx,
        )
        .await?;
    assert!(dispatch_result.success);
    let dispatch_json: Value = serde_json::from_str(&dispatch_result.output)?;
    let dispatch_id = dispatch_json["dispatch_id"]
        .as_str()
        .context("dispatch_id missing")?
        .to_string();

    let collect_result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": [dispatch_id],
                "timeout_secs": 2
            }),
            &ctx,
        )
        .await?;
    assert!(collect_result.success);

    let entries: Vec<Value> = serde_json::from_str(&collect_result.output)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "COMPLETED");
    assert_eq!(entries[0]["output"], "worker completed");

    let requests = peer.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].agent_id, "worker-agent");
    assert_eq!(requests[0].input, "do work");
    assert_eq!(requests[0].user_id.as_str(), ctx.user_id.as_ref());
    assert_eq!(
        requests[0].parent_run_id.as_deref(),
        Some(ctx.run_id.as_ref())
    );

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_collect_tool_reports_remote_error() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-test-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_error("remote boom")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table.clone());
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    let dispatch_result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "explode"
            }),
            &ctx,
        )
        .await?;
    let dispatch_json: Value = serde_json::from_str(&dispatch_result.output)?;
    let dispatch_id = dispatch_json["dispatch_id"]
        .as_str()
        .context("dispatch_id missing")?
        .to_string();

    let collect_result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": [dispatch_id],
                "timeout_secs": 2
            }),
            &ctx,
        )
        .await?;
    assert!(collect_result.success);

    let entries: Vec<Value> = serde_json::from_str(&collect_result.output)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "ERROR");
    assert!(entries[0]["error"]
        .as_str()
        .is_some_and(|error| error.contains("remote boom")));

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_nodes_tool_isolates_by_cluster_id() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-iso-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;

    // Register two peers in different clusters
    let peer_a = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-cluster-a",
        "http://a.local",
        "cluster-alpha",
    );
    peer_a.register().await?;

    let peer_b = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-cluster-b",
        "http://b.local",
        "cluster-beta",
    );
    peer_b.register().await?;

    // Service in cluster-alpha with a different node_id
    let cluster_client_alpha = Arc::new(ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-self-alpha",
        "http://self-alpha.local",
        "cluster-alpha",
    ));
    let bendclaw_client = Arc::new(BendclawClient::new(auth_token, Duration::from_secs(5)));
    let service_alpha = Arc::new(ClusterService::with_options(
        cluster_client_alpha,
        bendclaw_client,
        ClusterOptions {
            heartbeat_interval: Duration::from_millis(100),
            dispatch_poll_interval: Duration::from_millis(25),
        },
    ));

    let tool = ClusterNodesTool::new(service_alpha);
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    assert!(result.success);

    let nodes: Vec<Value> = serde_json::from_str(&result.output)?;
    // Should only see node-cluster-a (same cluster), not node-cluster-b
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["node_id"], "node-cluster-a");

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_rejects_nested_fanout() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-fanout-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_complete("should not reach")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let ctx = test_tool_context();
    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;

    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table);

    // Simulate a dispatched context (is_dispatched = true)
    let dispatched_ctx = ToolContext {
        is_dispatched: true,
        ..test_tool_context()
    };

    let result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "nested work"
            }),
            &dispatched_ctx,
        )
        .await?;

    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("nested dispatch is not allowed")));

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_tool_rejects_empty_node_id() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-empty-node";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let dispatch_tool = ClusterDispatchTool::new(service, dispatch_table);
    let ctx = test_tool_context();

    let result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "",
                "agent_id": "worker",
                "task": "do stuff"
            }),
            &ctx,
        )
        .await?;
    assert!(!result.success);

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_tool_rejects_empty_task() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-empty-task";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let dispatch_tool = ClusterDispatchTool::new(service, dispatch_table);
    let ctx = test_tool_context();

    let result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker",
                "task": ""
            }),
            &ctx,
        )
        .await?;
    assert!(!result.success);

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_tool_rejects_unknown_node() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-unknown-node";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let dispatch_tool = ClusterDispatchTool::new(service, dispatch_table);
    let ctx = test_tool_context();

    let result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "ghost-node",
                "agent_id": "worker",
                "task": "do stuff"
            }),
            &ctx,
        )
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("ghost-node")));

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_collect_tool_rejects_empty_dispatch_ids() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-empty-ids";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": [],
                "timeout_secs": 1
            }),
            &ctx,
        )
        .await?;
    assert!(!result.success);

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_collect_tool_rejects_unknown_dispatch_id() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-unknown-did";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": ["nonexistent-id"],
                "timeout_secs": 1
            }),
            &ctx,
        )
        .await?;
    assert!(!result.success);

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_collect_tool_returns_partial_on_timeout() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-timeout";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| FakeRunPlan::stuck_running()).await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table.clone());
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    let dispatch_result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker",
                "task": "hang forever"
            }),
            &ctx,
        )
        .await?;
    assert!(dispatch_result.success);
    let dispatch_json: Value = serde_json::from_str(&dispatch_result.output)?;
    let dispatch_id = dispatch_json["dispatch_id"]
        .as_str()
        .context("dispatch_id missing")?
        .to_string();

    let collect_result = collect_tool
        .execute_with_context(
            serde_json::json!({
                "dispatch_ids": [dispatch_id],
                "timeout_secs": 1
            }),
            &ctx,
        )
        .await?;
    assert!(collect_result.success);

    let entries: Vec<Value> = serde_json::from_str(&collect_result.output)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "RUNNING");

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_to_multiple_nodes_and_collect_all() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-multi";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer_a = FakePeerNode::start(auth_token, |_req| {
        FakeRunPlan::running_then_complete("result-a")
    })
    .await?;
    let peer_b = FakePeerNode::start(auth_token, |_req| {
        FakeRunPlan::running_then_complete("result-b")
    })
    .await?;

    let client_a = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-a",
        peer_a.base_url(),
        "test-cluster",
    );
    client_a.register().await?;
    let client_b = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-b",
        peer_b.base_url(),
        "test-cluster",
    );
    client_b.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table.clone());
    let collect_tool = ClusterCollectTool::new(dispatch_table);
    let ctx = test_tool_context();

    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;

    let d1 = dispatch_tool
        .execute_with_context(
            serde_json::json!({"node_id": "node-a", "agent_id": "w1", "task": "task-a"}),
            &ctx,
        )
        .await?;
    let d2 = dispatch_tool
        .execute_with_context(
            serde_json::json!({"node_id": "node-b", "agent_id": "w2", "task": "task-b"}),
            &ctx,
        )
        .await?;
    let id1: Value = serde_json::from_str(&d1.output)?;
    let id2: Value = serde_json::from_str(&d2.output)?;
    let ids = vec![
        id1["dispatch_id"].as_str().unwrap().to_string(),
        id2["dispatch_id"].as_str().unwrap().to_string(),
    ];

    let collect_result = collect_tool
        .execute_with_context(
            serde_json::json!({"dispatch_ids": ids, "timeout_secs": 3}),
            &ctx,
        )
        .await?;
    assert!(collect_result.success);

    let entries: Vec<Value> = serde_json::from_str(&collect_result.output)?;
    assert_eq!(entries.len(), 2);
    let mut outputs: Vec<&str> = entries
        .iter()
        .filter_map(|e| e["output"].as_str())
        .collect();
    outputs.sort();
    assert_eq!(outputs, vec!["result-a", "result-b"]);

    peer_a.shutdown().await;
    peer_b.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_nodes_tool_returns_empty_when_no_peers() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-empty-peers";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let service = make_service(registry.base_url(), auth_token, "node-self");
    let tool = ClusterNodesTool::new(service);
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;
    assert!(result.success);

    let nodes: Vec<Value> = serde_json::from_str(&result.output)?;
    assert!(nodes.is_empty());

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_tool_allows_continue_run_context() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-continue-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_complete("continue ok")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table);
    let ctx = test_tool_context();

    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;

    // Simulate a continue_run context: is_dispatched = false even though
    // the caller would have set parent_run_id (lineage only).
    let continue_ctx = ToolContext {
        is_dispatched: false,
        ..test_tool_context()
    };

    let result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "continue work"
            }),
            &continue_ctx,
        )
        .await?;

    assert!(
        result.success,
        "dispatch should succeed for continue_run context"
    );

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn cluster_dispatch_tool_propagates_origin_node_id() -> Result<()> {
    tracing::init();
    let auth_token = "cluster-origin-token";
    let registry = FakeClusterRegistry::start(auth_token).await?;
    let peer = FakePeerNode::start(auth_token, |_request| {
        FakeRunPlan::running_then_complete("origin ok")
    })
    .await?;
    let peer_client = ClusterClient::new(
        registry.base_url(),
        auth_token,
        "node-peer",
        peer.base_url(),
        "test-cluster",
    );
    peer_client.register().await?;

    let service = make_service(registry.base_url(), auth_token, "node-self");
    let dispatch_table = service.create_dispatch_table();
    let nodes_tool = ClusterNodesTool::new(service.clone());
    let dispatch_tool = ClusterDispatchTool::new(service.clone(), dispatch_table);
    let ctx = test_tool_context();

    let _ = nodes_tool
        .execute_with_context(serde_json::json!({}), &ctx)
        .await?;

    let result = dispatch_tool
        .execute_with_context(
            serde_json::json!({
                "node_id": "node-peer",
                "agent_id": "worker-agent",
                "task": "check origin"
            }),
            &ctx,
        )
        .await?;
    assert!(result.success);

    let requests = peer.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].origin_node_id, "node-self",
        "origin_node_id should be the dispatching node's ID"
    );

    peer.shutdown().await;
    registry.shutdown().await;
    Ok(())
}
