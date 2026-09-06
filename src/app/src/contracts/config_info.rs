use serde::Deserialize;
use serde::Serialize;

/// Published NAPI JSON envelope. Field spelling is intentionally independent
/// from Rust naming. Model entries remain extensible catalog-owned objects.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfo {
    pub provider: String,
    pub protocol: String,
    pub env_path: String,
    pub has_api_key: bool,
    pub base_url: Option<String>,
    pub available_models: Vec<serde_json::Value>,
    pub thinking_level: String,
}
