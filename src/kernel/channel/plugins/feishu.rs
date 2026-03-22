use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::SinkExt;
use futures::StreamExt;
use prost::Message as ProstMessage;
use serde::Deserialize;
use serde::Serialize;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::capabilities::ChannelCapabilities;
use crate::kernel::channel::capabilities::ChannelKind;
use crate::kernel::channel::capabilities::InboundMode;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::message::InboundMessage;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::channel::plugin::ChannelPlugin;
use crate::kernel::channel::plugin::InboundEventSender;
use crate::kernel::channel::plugin::InboundKind;
use crate::kernel::channel::plugin::ReceiverFactory;
use crate::observability::log::slog;

pub const FEISHU_CHANNEL_TYPE: &str = "feishu";
const FEISHU_API: &str = "https://open.feishu.cn/open-apis";
const FEISHU_DOMAIN: &str = "https://open.feishu.cn";
const FEISHU_MAX_MESSAGE_LEN: usize = 30_000;

const DEFAULT_PING_INTERVAL_SECS: u64 = 120;
const RECONNECT_DELAY_SECS: u64 = 5;

// ── Protobuf (pbbp2) ──

#[derive(Clone, PartialEq, ProstMessage)]
struct PbHeader {
    #[prost(string, required, tag = "1")]
    key: String,
    #[prost(string, required, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, ProstMessage)]
struct PbFrame {
    #[prost(uint64, required, tag = "1")]
    seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    log_id: u64,
    #[prost(int32, required, tag = "3")]
    service: i32,
    #[prost(int32, required, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<PbHeader>,
    #[prost(string, optional, tag = "6")]
    payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    log_id_new: Option<String>,
}

impl PbFrame {
    fn get_header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
    }
}

const FRAME_METHOD_CONTROL: i32 = 0;
const FRAME_METHOD_DATA: i32 = 1;

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
}

// ── Plugin ──

pub struct FeishuChannel {
    client: reqwest::Client,
}

impl FeishuChannel {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for FeishuChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelPlugin for FeishuChannel {
    fn channel_type(&self) -> &str {
        FEISHU_CHANNEL_TYPE
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::WebSocket,
            supports_edit: true,
            supports_streaming: false,
            supports_markdown: true,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
        }
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let c: FeishuConfig = serde_json::from_value(config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))?;
        if c.app_id.is_empty() || c.app_secret.is_empty() {
            return Err(ErrorCode::config(
                "feishu app_id and app_secret are required",
            ));
        }
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(FeishuOutbound {
            client: self.client.clone(),
        })
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(FeishuReceiverFactory {
            client: self.client.clone(),
        }))
    }
}

// ── ReceiverFactory ──

struct FeishuReceiverFactory {
    client: reqwest::Client,
}

#[async_trait]
impl ReceiverFactory for FeishuReceiverFactory {
    async fn spawn(
        &self,
        account: &ChannelAccount,
        event_tx: InboundEventSender,
        cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let config: FeishuConfig = serde_json::from_value(account.config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))?;
        let client = self.client.clone();
        let account_id = account.channel_account_id.clone();

        let handle = tokio::spawn(async move {
            slog!(info, "feishu_ws", "receiver_started", account_id = %account_id,);
            let mut backoff_secs = RECONNECT_DELAY_SECS;
            const MAX_BACKOFF_SECS: u64 = 120;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        slog!(info, "feishu_ws", "receiver_cancelled", account_id = %account_id,);
                        return;
                    }
                    result = ws_receive_loop(&client, &config, &event_tx, &cancel) => {
                        match result {
                            Ok(()) => {
                                slog!(info, "feishu_ws", "closed_reconnecting", account_id = %account_id,);
                                backoff_secs = RECONNECT_DELAY_SECS;
                            }
                            Err(e) => {
                                slog!(error, "feishu_ws", "error_reconnecting", account_id = %account_id, backoff_secs, error = %e,);
                            }
                        }
                    }
                }
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                }
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF_SECS);
            }
        });

        Ok(handle)
    }
}

// ── Outbound ──

struct FeishuOutbound {
    client: reqwest::Client,
}

impl FeishuOutbound {
    fn extract_credentials(config: &serde_json::Value) -> Result<(String, String)> {
        let app_id = config
            .get("app_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ErrorCode::config("feishu config missing app_id"))?;
        let app_secret = config
            .get("app_secret")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ErrorCode::config("feishu config missing app_secret"))?;
        Ok((app_id.to_string(), app_secret.to_string()))
    }
}

#[async_trait]
impl ChannelOutbound for FeishuOutbound {
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let (app_id, app_secret) = Self::extract_credentials(config)?;
        let token = get_tenant_token(&self.client, &app_id, &app_secret).await?;

        let url = format!("{FEISHU_API}/im/v1/messages?receive_id_type=chat_id");
        let content = serde_json::json!({ "text": text }).to_string();
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content,
        });
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu send: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu send response: {e}")))?;
        let msg_id = json["data"]["message_id"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(msg_id)
    }

    async fn send_typing(&self, _config: &serde_json::Value, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(
        &self,
        config: &serde_json::Value,
        _chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        let (app_id, app_secret) = Self::extract_credentials(config)?;
        let token = get_tenant_token(&self.client, &app_id, &app_secret).await?;
        let url = format!("{FEISHU_API}/im/v1/messages/{msg_id}");
        let content = serde_json::json!({ "text": text }).to_string();
        let body = serde_json::json!({
            "msg_type": "text",
            "content": content,
        });
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu edit_message: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            slog!(warn, "feishu_outbound", "edit_failed", http_status = %status, body,);
            return Err(ErrorCode::channel_send(format!(
                "feishu edit_message failed: HTTP {status}: {body}"
            )));
        }
        Ok(())
    }

    async fn add_reaction(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "feishu channel does not support reactions",
        ))
    }

    async fn update_draft(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        self.edit_message(config, chat_id, msg_id, text).await
    }
}

// ── WebSocket long connection ──

async fn get_tenant_token(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<String> {
    let url = format!("{FEISHU_API}/auth/v3/tenant_access_token/internal");
    let body = serde_json::json!({
        "app_id": app_id,
        "app_secret": app_secret,
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu auth: {e}")))?;
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu auth response: {e}")))?;

    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["msg"].as_str().unwrap_or("unknown");
        return Err(ErrorCode::internal(format!(
            "feishu auth failed: code={code}, msg={msg}"
        )));
    }

    json["tenant_access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ErrorCode::internal(format!(
                "feishu: missing tenant_access_token in response: {json}"
            ))
        })
}

async fn get_ws_endpoint(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64)> {
    let url = format!("{FEISHU_DOMAIN}/callback/ws/endpoint");
    let resp = client
        .post(&url)
        .header("locale", "zh")
        .json(&serde_json::json!({
            "AppID": app_id,
            "AppSecret": app_secret,
        }))
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws endpoint: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ErrorCode::internal(format!(
            "feishu ws endpoint HTTP {status}: {body}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws endpoint response: {e}")))?;

    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["msg"].as_str().unwrap_or("unknown error");
        return Err(ErrorCode::internal(format!(
            "feishu ws endpoint failed: code={code}, msg={msg}"
        )));
    }

    let client_config = &json["data"]["ClientConfig"];
    slog!(
        info,
        "feishu_ws",
        "endpoint_response",
        code,
        reconnect_count = client_config["ReconnectCount"].as_i64().unwrap_or(0),
        reconnect_interval = client_config["ReconnectInterval"].as_i64().unwrap_or(0),
        ping_interval = client_config["PingInterval"].as_i64().unwrap_or(0),
    );

    let ws_url = json["data"]["URL"]
        .as_str()
        .ok_or_else(|| ErrorCode::internal("feishu ws endpoint: missing URL"))?
        .to_string();

    let ping_interval = json["data"]["ClientConfig"]["PingInterval"]
        .as_u64()
        .unwrap_or(DEFAULT_PING_INTERVAL_SECS);

    Ok((ws_url, ping_interval))
}

fn redact_ws_url(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|mut u| {
            let redacted: Vec<(String, String)> = u
                .query_pairs()
                .map(|(k, v): (std::borrow::Cow<str>, std::borrow::Cow<str>)| {
                    let val = if k == "access_key" || k == "ticket" {
                        "***".to_string()
                    } else {
                        v.to_string()
                    };
                    (k.to_string(), val)
                })
                .collect();
            u.query_pairs_mut().clear();
            for (k, v) in &redacted {
                u.query_pairs_mut().append_pair(k, v);
            }
            u.to_string()
        })
        .unwrap_or_else(|_| url.to_string())
}

fn build_native_tls_connector() -> Result<tokio_tungstenite::Connector> {
    let tls = native_tls::TlsConnector::new()
        .map_err(|e| ErrorCode::internal(format!("native tls: {e}")))?;
    Ok(tokio_tungstenite::Connector::NativeTls(tls))
}

async fn ws_receive_loop(
    client: &reqwest::Client,
    config: &FeishuConfig,
    event_tx: &InboundEventSender,
    cancel: &CancellationToken,
) -> Result<()> {
    let (ws_url, ping_interval_secs) =
        get_ws_endpoint(client, &config.app_id, &config.app_secret).await?;

    let redacted_url = redact_ws_url(&ws_url);
    slog!(info, "feishu_ws", "connecting", url = %redacted_url, ping_interval = ping_interval_secs,);

    let connector = build_native_tls_connector()?;
    let (ws_stream, ws_resp) =
        tokio_tungstenite::connect_async_tls_with_config(&ws_url, None, false, Some(connector))
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu ws connect: {e}")))?;

    // Log handshake response headers (official SDK checks Handshake-Status)
    let hs_status = ws_resp
        .headers()
        .get("Handshake-Status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let hs_msg = ws_resp
        .headers()
        .get("Handshake-Msg")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    slog!(
        info,
        "feishu_ws",
        "handshake",
        status = ws_resp.status().as_u16(),
        hs_status,
        hs_msg,
    );

    let (mut write, mut read) = ws_stream.split();

    let service_id: i32 = reqwest::Url::parse(&ws_url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "service_id")
                .and_then(|(_, v)| v.parse().ok())
        })
        .unwrap_or(0);

    let mut ping_interval_dur = Duration::from_secs(ping_interval_secs);
    let mut ping_interval = tokio::time::interval(ping_interval_dur);
    ping_interval.tick().await;
    let mut last_recv = tokio::time::Instant::now();
    let mut timeout_check = tokio::time::interval(Duration::from_secs(10));
    timeout_check.tick().await;

    slog!(info, "feishu_ws", "connected", service_id,);

    let initial_ping = build_ping_frame(service_id);
    if write
        .send(Message::Binary(initial_ping.encode_to_vec()))
        .await
        .is_err()
    {
        return Err(ErrorCode::internal("feishu ws: initial ping failed"));
    }

    let mut msg_cache: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();

    let result = loop {
        let heartbeat_timeout = ping_interval_dur.max(Duration::from_secs(120)) * 3;
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                slog!(info, "feishu_ws", "closing_gracefully",);
                let _ = write.send(Message::Close(None)).await;
                break Ok(());
            }
            _ = timeout_check.tick() => {
                if last_recv.elapsed() > heartbeat_timeout {
                    slog!(warn, "feishu_ws", "heartbeat_timeout",
                        elapsed_secs = last_recv.elapsed().as_secs(),
                        timeout_secs = heartbeat_timeout.as_secs(),
                    );
                    break Err(ErrorCode::internal("feishu ws: heartbeat timeout, reconnecting"));
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let (resp_frame, event_payload) = decode_frame(
                            &data, &mut msg_cache, &mut last_recv, &config.app_id,
                            &mut ping_interval_dur,
                        );
                        // Send response BEFORE processing event (match official SDK behavior)
                        if let Some(resp) = resp_frame {
                            if write.send(Message::Binary(resp.encode_to_vec())).await.is_err() {
                                break Err(ErrorCode::internal("feishu ws write response failed"));
                            }
                        }
                        if let Some(payload) = event_payload {
                            handle_event_payload(&payload, config, event_tx, client).await;
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        slog!(info, "feishu_ws", "closed_by_server",);
                        break Ok(());
                    }
                    Some(Err(e)) => {
                        break Err(ErrorCode::internal(format!("feishu ws read: {e}")));
                    }
                    Some(Ok(other)) => {
                        slog!(warn, "feishu_ws", "unexpected_ws_msg", msg_type = %format!("{:?}", std::mem::discriminant(&other)),);
                    }
                }
            }
            _ = ping_interval.tick() => {
                let encoded = build_ping_frame(service_id).encode_to_vec();
                if write.send(Message::Binary(encoded)).await.is_err() {
                    break Err(ErrorCode::internal("feishu ws ping failed"));
                }
                slog!(info, "feishu_ws", "ping_sent",);
            }
        }
    };

    // Always try to send close frame on exit
    let _ = write.send(Message::Close(None)).await;
    result
}

fn build_ping_frame(service_id: i32) -> PbFrame {
    PbFrame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: FRAME_METHOD_CONTROL,
        headers: vec![PbHeader {
            key: "type".into(),
            value: "ping".into(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

/// Decode a PB frame, return (optional response frame, optional event payload to process).
/// This is intentionally sync so the WS read loop is not blocked by event processing.
fn decode_frame(
    data: &[u8],
    msg_cache: &mut HashMap<String, Vec<Option<Vec<u8>>>>,
    last_recv: &mut tokio::time::Instant,
    app_id: &str,
    ping_interval_dur: &mut Duration,
) -> (Option<PbFrame>, Option<String>) {
    let frame = match PbFrame::decode(data) {
        Ok(f) => f,
        Err(e) => {
            slog!(warn, "feishu_ws", "decode_failed", error = %e,);
            return (None, None);
        }
    };

    if frame.method == FRAME_METHOD_CONTROL {
        let msg_type = frame.get_header("type").unwrap_or("");
        if msg_type == "pong" {
            // Parse ClientConfig from pong payload (like official SDK)
            if let Some(payload) = &frame.payload {
                if let Ok(conf) = serde_json::from_slice::<serde_json::Value>(payload) {
                    if let Some(pi) = conf.get("PingInterval").and_then(|v| v.as_u64()) {
                        if pi > 0 {
                            *ping_interval_dur = Duration::from_secs(pi);
                        }
                    }
                }
            }
        }
        slog!(info, "feishu_ws", "control_frame", msg_type,);
        return (None, None);
    }

    if frame.method != FRAME_METHOD_DATA {
        slog!(info, "feishu_ws", "unknown_method", method = frame.method,);
        return (None, None);
    }

    let msg_type = frame.get_header("type").unwrap_or("").to_string();
    let msg_id = frame.get_header("message_id").unwrap_or("").to_string();
    let trace_id = frame.get_header("trace_id").unwrap_or("").to_string();
    let sum: usize = frame
        .get_header("sum")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let seq: usize = frame
        .get_header("seq")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let payload_bytes = frame.payload.clone().unwrap_or_default();

    // Multi-part message combining
    let full_payload = if sum > 1 {
        let buf = msg_cache
            .entry(msg_id.clone())
            .or_insert_with(|| vec![None; sum]);
        if seq < buf.len() {
            buf[seq] = Some(payload_bytes);
        }
        if buf.iter().all(|p| p.is_some()) {
            let combined: Vec<u8> = buf
                .iter()
                .flat_map(|p| p.as_ref().unwrap().clone())
                .collect();
            msg_cache.remove(&msg_id);
            combined
        } else {
            return (None, None);
        }
    } else {
        payload_bytes
    };

    let payload_str = String::from_utf8_lossy(&full_payload).to_string();

    // Build response frame immediately
    let resp = serde_json::json!({"code": 200});
    let mut resp_frame = frame.clone();
    resp_frame.payload = Some(resp.to_string().into_bytes());

    let event_payload = if msg_type == "event" {
        *last_recv = tokio::time::Instant::now();
        slog!(
            info,
            "feishu_ws",
            "event_received",
            msg_id,
            trace_id,
            app_id,
        );
        Some(payload_str)
    } else {
        slog!(
            info,
            "feishu_ws",
            "data_frame_received",
            msg_type,
            msg_id,
            trace_id,
        );
        None
    };

    (Some(resp_frame), event_payload)
}

async fn handle_event_payload(
    payload: &str,
    config: &FeishuConfig,
    event_tx: &InboundEventSender,
    client: &reqwest::Client,
) {
    let json: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            slog!(warn, "feishu_ws", "invalid_json", error = %e,);
            return;
        }
    };

    let event_type = json
        .pointer("/header/event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if event_type != "im.message.receive_v1" {
        slog!(info, "feishu_ws", "event_ignored", event_type,);
        return;
    }

    let Some(event_data) = json.get("event") else {
        slog!(warn, "feishu_ws", "missing_event_field",);
        return;
    };

    let sender_id = event_data
        .pointer("/sender/sender_id/open_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !is_allowed(&config.allow_from, sender_id) {
        slog!(warn, "feishu_ws", "sender_denied", sender_id,);
        return;
    }

    // Add thumbsup reaction
    let feishu_msg_id = event_data
        .pointer("/message/message_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !feishu_msg_id.is_empty() {
        let _ = add_reaction(
            client,
            &config.app_id,
            &config.app_secret,
            feishu_msg_id,
            "THUMBSUP",
        )
        .await;
    }

    if let Some(inbound) = parse_feishu_message(event_data) {
        use crate::kernel::channel::delivery::backpressure::BackpressureResult;
        match event_tx.send(inbound) {
            BackpressureResult::Accepted => {
                slog!(info, "feishu_ws", "message_queued",);
            }
            BackpressureResult::Busy => {
                slog!(warn, "feishu_ws", "channel_busy",);
            }
            BackpressureResult::Rejected => {
                slog!(warn, "feishu_ws", "channel_full",);
            }
        }
    } else {
        slog!(info, "feishu_ws", "unsupported_type",);
    }
}

async fn add_reaction(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
    message_id: &str,
    emoji_type: &str,
) -> Result<()> {
    let token = get_tenant_token(client, app_id, app_secret).await?;
    let url = format!("{FEISHU_API}/im/v1/messages/{message_id}/reactions");
    let body = serde_json::json!({
        "reaction_type": { "emoji_type": emoji_type }
    });
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu add reaction: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        slog!(warn, "feishu_outbound", "reaction_failed", http_status = %status, body, message_id = message_id, emoji_type,);
    } else {
        slog!(
            info,
            "feishu_outbound",
            "reaction_sent",
            message_id = message_id,
            emoji_type,
        );
    }
    Ok(())
}

fn is_allowed(allow_from: &[String], sender_id: &str) -> bool {
    if allow_from.is_empty() {
        return true;
    }
    if allow_from.iter().any(|s| s == "*") {
        return true;
    }
    allow_from.iter().any(|s| s == sender_id)
}

fn parse_feishu_message(event: &serde_json::Value) -> Option<InboundEvent> {
    let message = event.get("message")?;
    let sender = event.get("sender")?.get("sender_id")?;

    let msg_id = message.get("message_id")?.as_str()?;
    let chat_id = message.get("chat_id")?.as_str()?;
    let sender_id = sender.get("open_id")?.as_str()?;
    let msg_type = message.get("message_type")?.as_str()?;

    if msg_type != "text" {
        slog!(warn, "feishu_ws", "unsupported_msg_type", msg_type, msg_id,);
        return None;
    }

    let content_str = message.get("content")?.as_str()?;
    let content: serde_json::Value = match serde_json::from_str(content_str) {
        Ok(v) => v,
        Err(e) => {
            slog!(warn, "feishu_ws", "content_parse_failed", msg_id, error = %e, content_str,);
            return None;
        }
    };
    let text = match content.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            slog!(warn, "feishu_ws", "no_text_field", msg_id, content = %content,);
            return None;
        }
    };

    let timestamp = message
        .get("create_time")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|ms| ms / 1000)
        .unwrap_or(0);

    Some(InboundEvent::Message(InboundMessage {
        message_id: msg_id.to_string(),
        chat_id: chat_id.to_string(),
        sender_id: sender_id.to_string(),
        sender_name: String::new(),
        text: text.to_string(),
        attachments: vec![],
        timestamp,
    }))
}
