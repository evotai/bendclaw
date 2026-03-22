//! Feishu WebSocket long-connection receiver.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::base::{ErrorCode, Result};
use crate::kernel::channel::delivery::backpressure::BackpressureResult;
use crate::kernel::channel::plugin::InboundEventSender;
use crate::observability::log::slog;

use super::config::FeishuConfig;
use super::message::{parse_event, MessageDedup};
use super::token::{get_token, TokenCache};

const FEISHU_API: &str = "https://open.feishu.cn/open-apis";
const FEISHU_DOMAIN: &str = "https://open.feishu.cn";
const DEFAULT_PING_INTERVAL_SECS: u64 = 120;
pub const RECONNECT_DELAY_SECS: u64 = 5;
pub const MAX_BACKOFF_SECS: u64 = 120;

// ── pbbp2 protobuf ────────────────────────────────────────────────────────────

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
        self.headers.iter().find(|h| h.key == key).map(|h| h.value.as_str())
    }
}

const METHOD_CONTROL: i32 = 0;
const METHOD_DATA: i32 = 1;

// ── WS endpoint ───────────────────────────────────────────────────────────────

async fn get_ws_endpoint(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64)> {
    let url = format!("{FEISHU_DOMAIN}/callback/ws/endpoint");
    let resp = client
        .post(&url)
        .header("locale", "zh")
        .json(&serde_json::json!({ "AppID": app_id, "AppSecret": app_secret }))
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws endpoint: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ErrorCode::internal(format!("feishu ws endpoint HTTP {status}: {body}")));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu ws endpoint response: {e}")))?;

    let code = json["code"].as_i64().unwrap_or(-1);
    if code != 0 {
        let msg = json["msg"].as_str().unwrap_or("unknown");
        return Err(ErrorCode::internal(format!(
            "feishu ws endpoint failed: code={code}, msg={msg}"
        )));
    }

    let ws_url = json["data"]["URL"]
        .as_str()
        .ok_or_else(|| ErrorCode::internal("feishu ws endpoint: missing URL"))?
        .to_string();

    let ping_interval = json["data"]["ClientConfig"]["PingInterval"]
        .as_u64()
        .unwrap_or(DEFAULT_PING_INTERVAL_SECS);

    slog!(info, "feishu_ws", "endpoint_obtained", ping_interval,);
    Ok((ws_url, ping_interval))
}

fn redact_ws_url(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|mut u| {
            let redacted: Vec<(String, String)> = u
                .query_pairs()
                .map(|(k, v)| {
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

fn build_ping_frame(service_id: i32) -> PbFrame {
    PbFrame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: METHOD_CONTROL,
        headers: vec![PbHeader { key: "type".into(), value: "ping".into() }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

// ── Frame decoding (sync, testable) ──────────────────────────────────────────

pub struct DecodedFrame {
    response: Option<PbFrame>,
    pub event_payload: Option<String>,
    pub updated_ping_interval: Option<Duration>,
}

/// Decode a raw binary WS frame. Pure function — no async, no side effects.
pub fn decode_frame(
    data: &[u8],
    msg_cache: &mut HashMap<String, Vec<Option<Vec<u8>>>>,
) -> DecodedFrame {
    let frame = match PbFrame::decode(data) {
        Ok(f) => f,
        Err(e) => {
            slog!(warn, "feishu_ws", "decode_failed", error = %e,);
            return DecodedFrame { response: None, event_payload: None, updated_ping_interval: None };
        }
    };

    // CONTROL frame (ping/pong)
    if frame.method == METHOD_CONTROL {
        let mut updated_ping_interval = None;
        if frame.get_header("type") == Some("pong") {
            if let Some(payload) = &frame.payload {
                if let Ok(conf) = serde_json::from_slice::<serde_json::Value>(payload) {
                    if let Some(pi) = conf.get("PingInterval").and_then(|v| v.as_u64()) {
                        if pi > 0 {
                            updated_ping_interval = Some(Duration::from_secs(pi));
                        }
                    }
                }
            }
        }
        return DecodedFrame { response: None, event_payload: None, updated_ping_interval };
    }

    if frame.method != METHOD_DATA {
        return DecodedFrame { response: None, event_payload: None, updated_ping_interval: None };
    }

    let msg_type = frame.get_header("type").unwrap_or("").to_string();
    let msg_id = frame.get_header("message_id").unwrap_or("").to_string();
    let sum: usize = frame.get_header("sum").and_then(|s| s.parse().ok()).unwrap_or(1);
    let seq: usize = frame.get_header("seq").and_then(|s| s.parse().ok()).unwrap_or(0);

    let payload_bytes = frame.payload.clone().unwrap_or_default();

    // Fragment reassembly
    let full_payload = if sum > 1 {
        let buf = msg_cache.entry(msg_id.clone()).or_insert_with(|| vec![None; sum]);
        if seq < buf.len() {
            buf[seq] = Some(payload_bytes);
        }
        if buf.iter().all(|p| p.is_some()) {
            let combined: Vec<u8> =
                buf.iter().flat_map(|p| p.as_ref().unwrap().clone()).collect();
            msg_cache.remove(&msg_id);
            combined
        } else {
            return DecodedFrame { response: None, event_payload: None, updated_ping_interval: None };
        }
    } else {
        payload_bytes
    };

    // ACK response
    let mut resp_frame = frame.clone();
    resp_frame.payload = Some(br#"{"code":200}"#.to_vec());

    let event_payload = if msg_type == "event" {
        Some(String::from_utf8_lossy(&full_payload).to_string())
    } else {
        None
    };

    DecodedFrame {
        response: Some(resp_frame),
        event_payload,
        updated_ping_interval: None,
    }
}

// ── Reaction ──────────────────────────────────────────────────────────────────

pub async fn add_reaction(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
    token_cache: &TokenCache,
    message_id: &str,
    emoji_type: &str,
) -> Result<()> {
    let token = get_token(client, FEISHU_API, app_id, app_secret, token_cache).await?;
    let url = format!("{FEISHU_API}/im/v1/messages/{message_id}/reactions");
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&serde_json::json!({ "reaction_type": { "emoji_type": emoji_type } }))
        .send()
        .await
        .map_err(|e| ErrorCode::internal(format!("feishu add reaction: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        slog!(warn, "feishu_outbound", "reaction_failed", http_status = %status, body, message_id, emoji_type,);
    }
    Ok(())
}

// ── Main receive loop ─────────────────────────────────────────────────────────

pub async fn receive_loop(
    client: &reqwest::Client,
    config: &FeishuConfig,
    event_tx: &InboundEventSender,
    cancel: &CancellationToken,
    token_cache: &TokenCache,
    dedup: &Arc<RwLock<MessageDedup>>,
    bot_open_id: Option<&str>,
) -> Result<()> {
    let (ws_url, ping_interval_secs) =
        get_ws_endpoint(client, &config.app_id, &config.app_secret).await?;

    slog!(info, "feishu_ws", "connecting", url = %redact_ws_url(&ws_url),);

    let connector = {
        let tls = native_tls::TlsConnector::new()
            .map_err(|e| ErrorCode::internal(format!("native tls: {e}")))?;
        tokio_tungstenite::Connector::NativeTls(tls)
    };

    let (ws_stream, ws_resp) =
        tokio_tungstenite::connect_async_tls_with_config(&ws_url, None, false, Some(connector))
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu ws connect: {e}")))?;

    let hs_status = ws_resp.headers().get("Handshake-Status")
        .and_then(|v| v.to_str().ok()).unwrap_or("");
    let hs_msg = ws_resp.headers().get("Handshake-Msg")
        .and_then(|v| v.to_str().ok()).unwrap_or("");
    slog!(info, "feishu_ws", "handshake", hs_status, hs_msg,);

    let service_id: i32 = reqwest::Url::parse(&ws_url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "service_id")
                .and_then(|(_, v)| v.parse().ok())
        })
        .unwrap_or(0);

    let (mut write, mut read) = ws_stream.split();

    let mut ping_interval_dur = Duration::from_secs(ping_interval_secs);
    let mut ping_ticker = tokio::time::interval(ping_interval_dur);
    ping_ticker.tick().await;
    let mut timeout_check = tokio::time::interval(Duration::from_secs(10));
    timeout_check.tick().await;
    let mut last_recv = tokio::time::Instant::now();

    // Send initial ping
    write
        .send(Message::Binary(build_ping_frame(service_id).encode_to_vec()))
        .await
        .map_err(|_| ErrorCode::internal("feishu ws: initial ping failed"))?;

    let mut msg_cache: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();

    let result = loop {
        let heartbeat_timeout = ping_interval_dur.max(Duration::from_secs(120)) * 3;

        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                let _ = write.send(Message::Close(None)).await;
                break Ok(());
            }

            _ = timeout_check.tick() => {
                if last_recv.elapsed() > heartbeat_timeout {
                    slog!(warn, "feishu_ws", "heartbeat_timeout",
                        elapsed_secs = last_recv.elapsed().as_secs(),);
                    break Err(ErrorCode::internal("feishu ws: heartbeat timeout"));
                }
            }

            _ = ping_ticker.tick() => {
                if write.send(Message::Binary(build_ping_frame(service_id).encode_to_vec())).await.is_err() {
                    break Err(ErrorCode::internal("feishu ws: ping failed"));
                }
            }

            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        last_recv = tokio::time::Instant::now();
                        let decoded = decode_frame(&data, &mut msg_cache);

                        if let Some(updated) = decoded.updated_ping_interval {
                            ping_interval_dur = updated;
                            ping_ticker = tokio::time::interval(ping_interval_dur);
                        }

                        if let Some(resp) = decoded.response {
                            if write.send(Message::Binary(resp.encode_to_vec())).await.is_err() {
                                break Err(ErrorCode::internal("feishu ws: write response failed"));
                            }
                        }

                        if let Some(payload) = decoded.event_payload {
                            handle_event(
                                &payload, config, event_tx, client,
                                token_cache, dedup, bot_open_id,
                            ).await;
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
                    _ => {}
                }
            }
        }
    };

    let _ = write.send(Message::Close(None)).await;
    result
}

async fn handle_event(
    payload: &str,
    config: &FeishuConfig,
    event_tx: &InboundEventSender,
    client: &reqwest::Client,
    token_cache: &TokenCache,
    dedup: &Arc<RwLock<MessageDedup>>,
    bot_open_id: Option<&str>,
) {
    let json: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            slog!(warn, "feishu_ws", "invalid_json", error = %e,);
            return;
        }
    };

    let event_type = json.pointer("/header/event_type").and_then(|v| v.as_str()).unwrap_or("");
    if event_type != "im.message.receive_v1" {
        slog!(info, "feishu_ws", "event_ignored", event_type,);
        return;
    }

    let Some(event_data) = json.get("event") else {
        return;
    };

    let sender_id = event_data
        .pointer("/sender/sender_id/open_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !config.is_sender_allowed(sender_id) {
        slog!(warn, "feishu_ws", "sender_denied", sender_id,);
        return;
    }

    // Dedup check
    let msg_id = event_data
        .pointer("/message/message_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !msg_id.is_empty() {
        let mut dedup_guard = dedup.write().await;
        if !dedup_guard.check_and_insert(msg_id) {
            slog!(info, "feishu_ws", "duplicate_message", msg_id,);
            return;
        }
    }

    // Reaction ack (fire-and-forget)
    if !msg_id.is_empty() {
        let client = client.clone();
        let app_id = config.app_id.clone();
        let app_secret = config.app_secret.clone();
        let token_cache = token_cache.clone();
        let msg_id_owned = msg_id.to_string();
        tokio::spawn(async move {
            let _ = add_reaction(&client, &app_id, &app_secret, &token_cache, &msg_id_owned, "THUMBSUP").await;
        });
    }

    if let Some(inbound) = parse_event(event_data, config.mention_only, bot_open_id) {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_ws_url_hides_sensitive_params() {
        let url = "wss://open.feishu.cn/ws?service_id=1&access_key=secret123&ticket=tok456";
        let redacted = redact_ws_url(url);
        assert!(!redacted.contains("secret123"));
        assert!(!redacted.contains("tok456"));
        assert!(redacted.contains("service_id=1"));
        assert!(redacted.contains("access_key=***"));
        assert!(redacted.contains("ticket=***"));
    }

    #[test]
    fn decode_frame_returns_empty_on_invalid_data() {
        let mut cache = HashMap::new();
        let result = decode_frame(b"not protobuf", &mut cache);
        assert!(result.response.is_none());
        assert!(result.event_payload.is_none());
    }

    #[test]
    fn build_ping_frame_is_control() {
        let frame = build_ping_frame(42);
        assert_eq!(frame.method, METHOD_CONTROL);
        assert_eq!(frame.service, 42);
        assert_eq!(frame.get_header("type"), Some("ping"));
    }
}
