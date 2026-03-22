use serde::{Deserialize, Serialize};

use crate::base::{ErrorCode, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
    /// When true, only respond in group chats when @-mentioned.
    #[serde(default = "default_true")]
    pub mention_only: bool,
}

fn default_true() -> bool {
    true
}

impl FeishuConfig {
    pub fn from_json(v: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(v.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))
    }

    pub fn validate(&self) -> Result<()> {
        if self.app_id.is_empty() || self.app_secret.is_empty() {
            return Err(ErrorCode::config(
                "feishu app_id and app_secret are required",
            ));
        }
        Ok(())
    }

    pub fn is_sender_allowed(&self, sender_id: &str) -> bool {
        if self.allow_from.is_empty() {
            return true;
        }
        self.allow_from.iter().any(|s| s == "*" || s == sender_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_requires_app_id_and_secret() {
        let ok = FeishuConfig {
            app_id: "id".into(),
            app_secret: "secret".into(),
            allow_from: vec![],
            mention_only: true,
        };
        assert!(ok.validate().is_ok());

        let no_id = FeishuConfig { app_id: "".into(), ..ok.clone() };
        assert!(no_id.validate().is_err());

        let no_secret = FeishuConfig { app_secret: "".into(), ..ok.clone() };
        assert!(no_secret.validate().is_err());
    }

    #[test]
    fn is_sender_allowed_empty_list_allows_all() {
        let cfg = FeishuConfig {
            app_id: "id".into(),
            app_secret: "s".into(),
            allow_from: vec![],
            mention_only: true,
        };
        assert!(cfg.is_sender_allowed("anyone"));
    }

    #[test]
    fn is_sender_allowed_wildcard() {
        let cfg = FeishuConfig {
            app_id: "id".into(),
            app_secret: "s".into(),
            allow_from: vec!["*".into()],
            mention_only: true,
        };
        assert!(cfg.is_sender_allowed("anyone"));
    }

    #[test]
    fn is_sender_allowed_explicit_list() {
        let cfg = FeishuConfig {
            app_id: "id".into(),
            app_secret: "s".into(),
            allow_from: vec!["ou_abc".into()],
            mention_only: true,
        };
        assert!(cfg.is_sender_allowed("ou_abc"));
        assert!(!cfg.is_sender_allowed("ou_other"));
    }
}
