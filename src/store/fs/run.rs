use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::BendclawError;
use crate::error::Result;
use crate::run::RunEvent;
use crate::run::RunMeta;
use crate::store::run::RunStore;

pub struct FsRunStore {
    base_dir: PathBuf,
}

impl FsRunStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn meta_path(&self, run_id: &str) -> PathBuf {
        self.base_dir.join(format!("{run_id}.json"))
    }

    fn events_path(&self, run_id: &str) -> PathBuf {
        self.base_dir.join(format!("{run_id}.jsonl"))
    }
}

#[async_trait]
impl RunStore for FsRunStore {
    async fn save_run(&self, meta: &RunMeta) -> Result<()> {
        fs::create_dir_all(&self.base_dir).await?;
        let json = serde_json::to_string_pretty(meta)?;
        fs::write(self.meta_path(&meta.run_id), json).await?;
        Ok(())
    }

    async fn append_event(&self, run_id: &str, event: &RunEvent) -> Result<()> {
        fs::create_dir_all(&self.base_dir).await?;
        let mut line = serde_json::to_string(event)?;
        line.push('\n');
        let path = self.events_path(run_id);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn load_events(&self, run_id: &str) -> Result<Vec<RunEvent>> {
        let path = self.events_path(run_id);
        match fs::read_to_string(&path).await {
            Ok(content) => {
                let mut events = Vec::new();
                for line in content.lines() {
                    if !line.trim().is_empty() {
                        let event: RunEvent = serde_json::from_str(line)?;
                        events.push(event);
                    }
                }
                Ok(events)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(BendclawError::Io(e)),
        }
    }
}
