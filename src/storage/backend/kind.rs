use std::fmt;

use crate::base::ErrorCode;
use crate::base::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Local,
    Cloud,
}

impl StorageKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "local" => Ok(Self::Local),
            "cloud" => Ok(Self::Cloud),
            other => Err(ErrorCode::invalid_input(format!(
                "unknown storage type '{other}', expected 'local' or 'cloud'"
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }
}

impl fmt::Display for StorageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
