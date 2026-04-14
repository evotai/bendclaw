use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::config::FeishuChannelConfig;
use crate::agent::Agent;
use crate::channel::Channel;
use crate::error::Result;

pub struct FeishuChannel {
    #[allow(dead_code)]
    config: FeishuChannelConfig,
    #[allow(dead_code)]
    session_map: SessionMap,
}

impl FeishuChannel {
    pub fn new(config: FeishuChannelConfig) -> Self {
        Self {
            config,
            session_map: SessionMap::new(),
        }
    }

    pub fn spawn(
        conf: FeishuChannelConfig,
        agent: Arc<Agent>,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        let ch = Arc::new(Self::new(conf));
        tokio::spawn(async move {
            if let Err(e) = ch.run(agent, cancel).await {
                tracing::error!(channel = "feishu", error = %e, "channel exited");
            }
        })
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    async fn run(self: Arc<Self>, _agent: Arc<Agent>, cancel: CancellationToken) -> Result<()> {
        tracing::info!(channel = "feishu", "channel started");

        // TODO: implement feishu websocket connection loop
        // 1. Get WS endpoint via token
        // 2. Connect WebSocket
        // 3. Message loop:
        //    - decode protobuf frame
        //    - parse event → text, chat_id
        //    - session_map.resolve_or_create(chat_id)
        //    - Agent::query(QueryRequest::text(text).session_id(session_id))
        //    - collect reply from QueryStream
        //    - send_text back to feishu
        // 4. Reconnect on disconnect (exponential backoff)

        cancel.cancelled().await;
        tracing::info!(channel = "feishu", "channel stopped");
        Ok(())
    }
}

// ── channel-private session state ──

struct SessionMap {
    inner: Mutex<HashMap<String, String>>,
}

impl SessionMap {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    #[allow(dead_code)]
    async fn resolve_or_create(&self, chat_id: &str) -> String {
        let mut map = self.inner.lock().await;
        if let Some(id) = map.get(chat_id) {
            return id.clone();
        }
        let session_id = uuid::Uuid::new_v4().to_string();
        map.insert(chat_id.to_string(), session_id.clone());
        session_id
    }
}
