use serde::Deserialize;

pub const FEISHU_API: &str = "https://open.feishu.cn/open-apis";
pub const FEISHU_MAX_MESSAGE_LEN: usize = 30_000;

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
