use std::collections::HashMap;
use std::time::Duration;

use prost::Message as ProstMessage;

type MultiPartCache = HashMap<String, (tokio::time::Instant, Vec<Option<Vec<u8>>>)>;

use super::config::is_client_error;
use super::config::FeishuConfig;
use super::config::ReconnectConfig;
use super::config::DEFAULT_PING_INTERVAL_SECS;
use super::config::ENDPOINT_OK;
use super::config::FEISHU_DOMAIN;
use super::message::parse_event;
use super::message::MessageDedup;
use super::outbound::add_reaction;
use super::token::TokenCache;
use crate::channels::runtime::channel_trait::InboundEventSender;
use crate::channels::runtime::diagnostics;
use crate::types::spawn_fire_and_forget;
use crate::types::ErrorCode;
use crate::types::Result;

// ── Protobuf (pbbp2) ──

#[derive(Clone, PartialEq, ProstMessage)]
pub struct PbHeader {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, ProstMessage)]
pub struct PbFrame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<PbHeader>,
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
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

// ── DecodedFrame ──

pub struct DecodedFrame {
    pub response: Option<PbFrame>,
    pub event_payload: Option<String>,
    pub updated_ping_interval: Option<Duration>,
    pub updated_reconnect_config: Option<ReconnectConfig>,
}

/// Decode a PB frame — pure function, no side effects.
pub fn decode_frame(data: &[u8], msg_cache: &mut MultiPartCache) -> DecodedFrame {
    let frame = match PbFrame::decode(data) {
        Ok(f) => f,
        Err(e) => {
            diagnostics::log_feishu_decode_failed(&e);
            return DecodedFrame {
                response: None,
                event_payload: None,
                updated_ping_interval: None,
                updated_reconnect_config: None,
            };
        }
    };

    if frame.method == FRAME_METHOD_CONTROL {
        let msg_type = frame.get_header("type").unwrap_or("");
        let mut updated_ping_interval = None;
        let mut updated_reconnect_config = None;

        if msg_type == "pong" {
            if let Some(payload) = &frame.payload {
                if let Ok(conf) = serde_json::from_slice::<serde_json::Value>(payload) {
                    if let Some(pi) = conf.get("PingInterval").and_then(|v| v.as_u64()) {
                        if pi > 0 {
                            updated_ping_interval = Some(Duration::from_secs(pi));
                        }
                    }
                    let mut rc = ReconnectConfig::default();
                    rc.update_from_pong(&conf);
                    updated_reconnect_config = Some(rc);
                }
            }
        }
        return DecodedFrame {
            response: None,
            event_payload: None,
            updated_ping_interval,
            updated_reconnect_config,
        };
    }

    if frame.method != FRAME_METHOD_DATA {
        return DecodedFrame {
            response: None,
            event_payload: None,
            updated_ping_interval: None,
            updated_reconnect_config: None,
        };
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
        let (_, buf) = msg_cache
            .entry(msg_id.clone())
            .or_insert_with(|| (tokio::time::Instant::now(), vec![None; sum]));
        if seq < buf.len() {
            buf[seq] = Some(payload_bytes);
        }
        if buf.iter().all(|p| p.is_some()) {
            let combined: Vec<u8> = buf
                .iter()
                .filter_map(|p| p.as_ref())
                .flat_map(|p| p.clone())
                .collect();
            msg_cache.remove(&msg_id);
            combined
        } else {
            return DecodedFrame {
                response: None,
                event_payload: None,
                updated_ping_interval: None,
                updated_reconnect_config: None,
            };
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
        let event_type = serde_json::from_str::<serde_json::Value>(&payload_str)
            .ok()
            .and_then(|v| {
                v.get("header")
                    .and_then(|h| h.get("event_type"))
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        diagnostics::log_feishu_event_received(&msg_id, &trace_id, &event_type);
        Some(payload_str)
    } else {
        None
    };

    DecodedFrame {
        response: Some(resp_frame),
        event_payload,
        updated_ping_interval: None,
        updated_reconnect_config: None,
    }
}

// ── Helper functions ──

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

pub fn redact_ws_url(url: &str) -> String {
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

// ── Endpoint ──

/// Get WS endpoint URL and server config. Returns (url, ping_interval, ReconnectConfig).
pub async fn get_ws_endpoint(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64, ReconnectConfig)> {
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
    if code != ENDPOINT_OK {
        let msg = json["msg"].as_str().unwrap_or("unknown error");
        if is_client_error(code) {
            return Err(ErrorCode::config(format!(
                "feishu ws endpoint client error: code={code}, msg={msg}"
            )));
        }
        return Err(ErrorCode::internal(format!(
            "feishu ws endpoint failed: code={code}, msg={msg}"
        )));
    }

    let client_config = &json["data"]["ClientConfig"];
    diagnostics::log_feishu_endpoint_response(
        code,
        client_config["ReconnectCount"].as_i64().unwrap_or(0),
        client_config["ReconnectInterval"].as_i64().unwrap_or(0),
        client_config["PingInterval"].as_i64().unwrap_or(0),
    );

    let ws_url = json["data"]["URL"]
        .as_str()
        .ok_or_else(|| ErrorCode::internal("feishu ws endpoint: missing URL"))?
        .to_string();

    let ping_interval = client_config["PingInterval"]
        .as_u64()
        .unwrap_or(DEFAULT_PING_INTERVAL_SECS);

    let reconnect_config = ReconnectConfig::from_client_config(client_config);

    Ok((ws_url, ping_interval, reconnect_config))
}

// ── Receive loop ──

pub async fn ws_receive_loop(
    client: &reqwest::Client,
    config: &FeishuConfig,
    token_cache: &TokenCache,
    event_tx: &InboundEventSender,
    cancel: &tokio_util::sync::CancellationToken,
    reconnect_config: &mut ReconnectConfig,
) -> Result<()> {
    let (ws_url, ping_interval_secs, endpoint_rc) =
        get_ws_endpoint(client, &config.app_id, &config.app_secret).await?;

    // Merge endpoint reconnect config
    *reconnect_config = endpoint_rc;

    let redacted_url = redact_ws_url(&ws_url);
    diagnostics::log_feishu_connecting(&redacted_url, ping_interval_secs);

    let connector = build_native_tls_connector()?;
    let (ws_stream, ws_resp) =
        tokio_tungstenite::connect_async_tls_with_config(&ws_url, None, false, Some(connector))
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu ws connect: {e}")))?;

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
    diagnostics::log_feishu_handshake(ws_resp.status().as_u16(), hs_status, hs_msg);

    let (mut write, mut read) = futures::StreamExt::split(ws_stream);

    let service_id: i32 = reqwest::Url::parse(&ws_url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "service_id")
                .and_then(|(_, v)| v.parse().ok())
        })
        .unwrap_or(0);

    let mut ping_interval_dur = Duration::from_secs(ping_interval_secs);
    let mut ping_timer = tokio::time::interval(ping_interval_dur);
    ping_timer.tick().await;
    let mut last_recv = tokio::time::Instant::now();
    let mut timeout_check = tokio::time::interval(Duration::from_secs(10));
    timeout_check.tick().await;

    diagnostics::log_feishu_connected(service_id);
    event_tx.set_connected(true);

    let initial_ping = build_ping_frame(service_id);
    if futures::SinkExt::send(
        &mut write,
        tokio_tungstenite::tungstenite::Message::Binary(initial_ping.encode_to_vec()),
    )
    .await
    .is_err()
    {
        return Err(ErrorCode::internal("feishu ws: initial ping failed"));
    }

    let mut msg_cache: MultiPartCache = HashMap::new();
    let mut dedup = MessageDedup::new(Duration::from_secs(30 * 60));
    let mut msg_cache_last_cleanup = tokio::time::Instant::now();

    let result = loop {
        let heartbeat_timeout = ping_interval_dur.max(Duration::from_secs(120)) * 3;
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {

                let _ = futures::SinkExt::send(&mut write, tokio_tungstenite::tungstenite::Message::Close(None)).await;
                break Ok(());
            }
            _ = timeout_check.tick() => {
                if last_recv.elapsed() > heartbeat_timeout {
                    diagnostics::log_feishu_heartbeat_timeout(
                        last_recv.elapsed().as_secs(),
                        heartbeat_timeout.as_secs(),
                    );
                    break Err(ErrorCode::internal("feishu ws: heartbeat timeout, reconnecting"));
                }
                // Cleanup stale msg_cache entries (>5min old)
                if msg_cache_last_cleanup.elapsed() > Duration::from_secs(10) {
                    msg_cache.retain(|_, (inserted_at, _)| inserted_at.elapsed() < Duration::from_secs(300));
                    msg_cache_last_cleanup = tokio::time::Instant::now();
                }
            }
            msg = futures::StreamExt::next(&mut read) => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(data))) => {
                        // FIX #3: update last_recv on ALL binary frames, not just events
                        last_recv = tokio::time::Instant::now();

                        let decoded = decode_frame(&data, &mut msg_cache);

                        // FIX #4: rebuild timer if ping interval changed
                        if let Some(new_dur) = decoded.updated_ping_interval {
                            if new_dur != ping_interval_dur {

                                ping_interval_dur = new_dur;
                                ping_timer = tokio::time::interval(ping_interval_dur);
                                ping_timer.tick().await;
                            }
                        }

                        // Update reconnect config from pong
                        if let Some(rc) = decoded.updated_reconnect_config {
                            *reconnect_config = rc;
                        }

                        // Send response BEFORE processing event
                        if let Some(resp) = decoded.response {
                            if futures::SinkExt::send(&mut write, tokio_tungstenite::tungstenite::Message::Binary(resp.encode_to_vec())).await.is_err() {
                                break Err(ErrorCode::internal("feishu ws write response failed"));
                            }
                        }

                        if let Some(payload) = decoded.event_payload {
                            handle_event_payload(&payload, config, token_cache, event_tx, client, &mut dedup).await;
                        }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(data))) => {
                        let _ = futures::SinkExt::send(&mut write, tokio_tungstenite::tungstenite::Message::Pong(data)).await;
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {

                        break Ok(());
                    }
                    Some(Err(e)) => {
                        break Err(ErrorCode::internal(format!("feishu ws read: {e}")));
                    }
                    Some(Ok(other)) => {
                        diagnostics::log_feishu_unexpected_ws_msg(&format!("{:?}", std::mem::discriminant(&other)));
                    }
                }
            }
            _ = ping_timer.tick() => {
                let encoded = build_ping_frame(service_id).encode_to_vec();
                if futures::SinkExt::send(&mut write, tokio_tungstenite::tungstenite::Message::Binary(encoded)).await.is_err() {
                    break Err(ErrorCode::internal("feishu ws ping failed"));
                }
            }
        }
    };

    let _ = futures::SinkExt::send(
        &mut write,
        tokio_tungstenite::tungstenite::Message::Close(None),
    )
    .await;
    result
}

async fn handle_event_payload(
    payload: &str,
    config: &FeishuConfig,
    token_cache: &TokenCache,
    event_tx: &InboundEventSender,
    client: &reqwest::Client,
    dedup: &mut MessageDedup,
) {
    let json: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            diagnostics::log_feishu_invalid_json(&e);
            return;
        }
    };

    let event_type = json
        .pointer("/header/event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let feishu_msg_id = json
        .pointer("/event/message/message_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !feishu_msg_id.is_empty() {
        let client = client.clone();
        let tc = token_cache.clone();
        let app_id = config.app_id.clone();
        let app_secret = config.app_secret.clone();
        let msg_id_for_reaction = feishu_msg_id.clone();
        spawn_fire_and_forget("feishu_reaction", async move {
            add_reaction(
                &client,
                &tc,
                &app_id,
                &app_secret,
                &msg_id_for_reaction,
                "THUMBSUP",
            )
            .await;
        });
    }

    if let Some(inbound) = parse_event(&json, config, dedup) {
        use crate::channels::egress::backpressure::BackpressureResult;
        match event_tx.send(inbound) {
            BackpressureResult::Accepted => {}
            BackpressureResult::Busy => {
                diagnostics::log_feishu_channel_busy(event_type);
            }
            BackpressureResult::Rejected => {
                diagnostics::log_feishu_channel_full(event_type);
            }
        }
    }
}
