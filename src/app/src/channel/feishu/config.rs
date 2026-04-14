use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct FeishuChannelConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default = "default_true")]
    pub mention_only: bool,
    #[serde(default)]
    pub allow_from: Vec<String>,
}

fn default_true() -> bool {
    true
}
