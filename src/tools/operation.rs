use std::fmt;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Impact {
    Low,
    Medium,
    High,
}

impl fmt::Display for Impact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpType {
    Reasoning,
    Execute,
    Edit,
    FileRead,
    FileWrite,
    FileList,
    SkillRun,
    Compaction,
    Databend,
    TaskWrite,
    TaskRead,
    WebSearch,
    WebFetch,
    ClusterNodes,
    ClusterDispatch,
    ClusterCollect,
    MemorySearch,
    MemorySave,
}

impl fmt::Display for OpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reasoning => write!(f, "REASONING"),
            Self::Execute => write!(f, "EXECUTE"),
            Self::Edit => write!(f, "EDIT"),
            Self::FileRead => write!(f, "FILE_READ"),
            Self::FileWrite => write!(f, "FILE_WRITE"),
            Self::FileList => write!(f, "FILE_LIST"),
            Self::SkillRun => write!(f, "SKILL_RUN"),
            Self::Compaction => write!(f, "COMPACTION"),
            Self::Databend => write!(f, "DATABEND"),
            Self::TaskWrite => write!(f, "TASK_WRITE"),
            Self::TaskRead => write!(f, "TASK_READ"),
            Self::WebSearch => write!(f, "WEB_SEARCH"),
            Self::WebFetch => write!(f, "WEB_FETCH"),
            Self::ClusterNodes => write!(f, "CLUSTER_NODES"),
            Self::ClusterDispatch => write!(f, "CLUSTER_DISPATCH"),
            Self::ClusterCollect => write!(f, "CLUSTER_COLLECT"),
            Self::MemorySearch => write!(f, "MEMORY_SEARCH"),
            Self::MemorySave => write!(f, "MEMORY_SAVE"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationMeta {
    pub op_type: OpType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<Impact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    pub duration_ms: u64,
    pub summary: String,
}

impl OperationMeta {
    pub fn new(op_type: OpType) -> Self {
        Self {
            op_type,
            impact: None,
            timeout_secs: None,
            duration_ms: 0,
            summary: String::new(),
        }
    }
    pub fn begin(op_type: OpType) -> OperationTracker {
        OperationTracker::new(op_type)
    }
}

pub struct OperationTracker {
    op_type: OpType,
    impact: Option<Impact>,
    timeout: Option<Duration>,
    summary: String,
    start: Instant,
}

impl OperationTracker {
    pub fn new(op_type: OpType) -> Self {
        Self {
            op_type,
            impact: None,
            timeout: None,
            summary: String::new(),
            start: Instant::now(),
        }
    }
    pub fn start_time(&self) -> Instant {
        self.start
    }
    pub fn impact(mut self, impact: Impact) -> Self {
        self.impact = Some(impact);
        self
    }
    pub fn maybe_impact(mut self, impact: Option<Impact>) -> Self {
        self.impact = impact;
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub fn summary(mut self, s: impl Into<String>) -> Self {
        self.summary = s.into();
        self
    }
    pub fn finish(self) -> OperationMeta {
        OperationMeta {
            op_type: self.op_type,
            impact: self.impact,
            timeout_secs: self.timeout.map(|d| d.as_secs()),
            duration_ms: self.start.elapsed().as_millis() as u64,
            summary: self.summary,
        }
    }
}
