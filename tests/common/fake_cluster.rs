#![allow(dead_code)]

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context as _;
use anyhow::Result;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::delete;
use axum::routing::get;
use axum::routing::post;
use axum::routing::put;
use axum::Json;
use axum::Router;
use bendclaw::client::NodeEntry;
use bendclaw::client::RemoteRunResponse;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone)]
struct RegistryState {
    auth_token: String,
    nodes: Arc<Mutex<BTreeMap<String, NodeEntry>>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RegisterRequest {
    node_id: String,
    endpoint: String,
    #[serde(default)]
    cluster_id: String,
    #[serde(default)]
    data: serde_json::Value,
}

pub struct FakeClusterRegistry {
    base_url: String,
    nodes: Arc<Mutex<BTreeMap<String, NodeEntry>>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl FakeClusterRegistry {
    pub async fn start(auth_token: &str) -> Result<Self> {
        let nodes = Arc::new(Mutex::new(BTreeMap::new()));
        let state = RegistryState {
            auth_token: auth_token.to_string(),
            nodes: nodes.clone(),
        };
        let app = Router::new()
            .route("/v1/cluster/nodes", post(register_node).get(list_nodes))
            .route("/v1/cluster/nodes/{node_id}/heartbeat", put(heartbeat_node))
            .route("/v1/cluster/nodes/{node_id}", delete(delete_node))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind fake cluster registry")?;
        let addr = listener.local_addr().context("fake registry local addr")?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let server_handle = tokio::spawn(async move {
            let shutdown = async {
                let _ = shutdown_rx.await;
            };
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await;
        });

        Ok(Self {
            base_url: format!("http://{addr}"),
            nodes,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn snapshot(&self) -> Vec<NodeEntry> {
        self.nodes
            .lock()
            .expect("fake registry nodes lock")
            .values()
            .cloned()
            .collect()
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.await;
        }
    }
}

#[derive(Debug, Clone)]
pub struct FakeRunRequest {
    pub run_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub input: String,
    pub parent_run_id: Option<String>,
    pub origin_node_id: String,
}

#[derive(Debug, Clone)]
pub struct FakeRunState {
    pub status: String,
    pub output: String,
    pub error: String,
}

impl FakeRunState {
    pub fn running() -> Self {
        Self {
            status: "RUNNING".to_string(),
            output: String::new(),
            error: String::new(),
        }
    }

    pub fn completed(output: impl Into<String>) -> Self {
        Self {
            status: "COMPLETED".to_string(),
            output: output.into(),
            error: String::new(),
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            status: "ERROR".to_string(),
            output: String::new(),
            error: error.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FakeRunPlan {
    pub create: FakeRunState,
    pub polls: Vec<FakeRunState>,
}

impl FakeRunPlan {
    pub fn immediate(output: impl Into<String>) -> Self {
        Self {
            create: FakeRunState::completed(output.into()),
            polls: vec![FakeRunState::completed("")],
        }
    }

    pub fn running_then_complete(output: impl Into<String>) -> Self {
        let output = output.into();
        Self {
            create: FakeRunState::running(),
            polls: vec![FakeRunState::running(), FakeRunState::completed(output)],
        }
    }

    pub fn running_then_error(error: impl Into<String>) -> Self {
        let error = error.into();
        Self {
            create: FakeRunState::running(),
            polls: vec![FakeRunState::running(), FakeRunState::failed(error)],
        }
    }

    /// Always returns RUNNING — never completes. Useful for timeout tests.
    pub fn stuck_running() -> Self {
        Self {
            create: FakeRunState::running(),
            polls: vec![FakeRunState::running()],
        }
    }
}

type RunPlanHandler = dyn Fn(&FakeRunRequest) -> FakeRunPlan + Send + Sync;

#[derive(Clone)]
struct FakePeerState {
    auth_token: String,
    handler: Arc<RunPlanHandler>,
    requests: Arc<Mutex<Vec<FakeRunRequest>>>,
    runs: Arc<Mutex<BTreeMap<String, VecDeque<RemoteRunResponse>>>>,
}

#[derive(Deserialize)]
struct CreateRunBody {
    input: String,
    #[allow(dead_code)]
    stream: bool,
}

pub struct FakePeerNode {
    base_url: String,
    requests: Arc<Mutex<Vec<FakeRunRequest>>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

impl FakePeerNode {
    pub async fn start(
        auth_token: &str,
        handler: impl Fn(&FakeRunRequest) -> FakeRunPlan + Send + Sync + 'static,
    ) -> Result<Self> {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = FakePeerState {
            auth_token: auth_token.to_string(),
            handler: Arc::new(handler),
            requests: requests.clone(),
            runs: Arc::new(Mutex::new(BTreeMap::new())),
        };
        let app = Router::new()
            .route("/v1/agents/{agent_id}/runs", post(create_run))
            .route("/v1/agents/{agent_id}/runs/{run_id}", get(get_run))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind fake peer node")?;
        let addr = listener.local_addr().context("fake peer local addr")?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let server_handle = tokio::spawn(async move {
            let shutdown = async {
                let _ = shutdown_rx.await;
            };
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await;
        });

        Ok(Self {
            base_url: format!("http://{addr}"),
            requests,
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn requests(&self) -> Vec<FakeRunRequest> {
        self.requests
            .lock()
            .expect("fake peer requests lock")
            .clone()
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.server_handle.take() {
            let _ = handle.await;
        }
    }
}

fn is_authorized(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value == expected)
}

fn run_response(run_id: &str, state: FakeRunState) -> RemoteRunResponse {
    RemoteRunResponse {
        id: run_id.to_string(),
        session_id: format!("session-{run_id}"),
        status: state.status,
        output: state.output,
        error: state.error,
    }
}

async fn register_node(
    State(state): State<RegistryState>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> std::result::Result<Json<NodeEntry>, StatusCode> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let node = NodeEntry {
        node_id: body.node_id.clone(),
        endpoint: body.endpoint,
        cluster_id: body.cluster_id.clone(),
        data: body.data,
    };
    state
        .nodes
        .lock()
        .expect("fake registry nodes lock")
        .insert(body.node_id, node.clone());
    Ok(Json(node))
}

#[derive(Deserialize, Default)]
struct ListNodesQuery {
    cluster_id: Option<String>,
}

async fn list_nodes(
    State(state): State<RegistryState>,
    headers: HeaderMap,
    Query(q): Query<ListNodesQuery>,
) -> std::result::Result<Json<Vec<NodeEntry>>, StatusCode> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let nodes = state
        .nodes
        .lock()
        .expect("fake registry nodes lock")
        .values()
        .filter(|n| match &q.cluster_id {
            Some(cid) => n.cluster_id == *cid,
            None => true,
        })
        .cloned()
        .collect();
    Ok(Json(nodes))
}

async fn heartbeat_node(
    State(state): State<RegistryState>,
    headers: HeaderMap,
    Path(node_id): Path<String>,
) -> std::result::Result<StatusCode, StatusCode> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let nodes = state.nodes.lock().expect("fake registry nodes lock");
    if nodes.contains_key(&node_id) {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn delete_node(
    State(state): State<RegistryState>,
    headers: HeaderMap,
    Path(node_id): Path<String>,
) -> std::result::Result<StatusCode, StatusCode> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    state
        .nodes
        .lock()
        .expect("fake registry nodes lock")
        .remove(&node_id);
    Ok(StatusCode::OK)
}

async fn create_run(
    State(state): State<FakePeerState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
    Json(body): Json<CreateRunBody>,
) -> std::result::Result<Json<RemoteRunResponse>, StatusCode> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let user_id = headers
        .get("x-user-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let parent_run_id = headers
        .get("x-parent-run-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let origin_node_id = headers
        .get("x-origin-node-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let run_id = ulid::Ulid::new().to_string();
    let request = FakeRunRequest {
        run_id: run_id.clone(),
        agent_id,
        user_id,
        input: body.input,
        parent_run_id,
        origin_node_id,
    };
    let plan = (state.handler)(&request);
    let create_response = run_response(&run_id, plan.create);
    let polls: VecDeque<_> = if plan.polls.is_empty() {
        vec![create_response.clone()].into()
    } else {
        plan.polls
            .into_iter()
            .map(|poll| run_response(&run_id, poll))
            .collect()
    };

    state
        .requests
        .lock()
        .expect("fake peer requests lock")
        .push(request);
    state
        .runs
        .lock()
        .expect("fake peer runs lock")
        .insert(run_id, polls);

    Ok(Json(create_response))
}

async fn get_run(
    State(state): State<FakePeerState>,
    headers: HeaderMap,
    Path((_agent_id, run_id)): Path<(String, String)>,
) -> std::result::Result<Json<RemoteRunResponse>, StatusCode> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let mut runs = state.runs.lock().expect("fake peer runs lock");
    let Some(queue) = runs.get_mut(&run_id) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let response = if queue.len() > 1 {
        queue.pop_front().expect("queue len checked")
    } else {
        queue.front().cloned().expect("queue has at least one item")
    };
    Ok(Json(response))
}
