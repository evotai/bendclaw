//! Variables — app-layer variable management with scope resolution and persistence.

use std::sync::Arc;

use bend_base::logx;
use bend_engine::tools::GetVariableFn;
use bend_engine::tools::GetVariableResponse;
use futures::future::BoxFuture;
use parking_lot::RwLock;

use crate::error::Result;
use crate::storage::Storage;
use crate::types::VariableRecord;
use crate::types::VariableScope;

// ---------------------------------------------------------------------------
// VariableInfo — projection for REPL display (value included for masked display)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub key: String,
    pub value: String,
    pub used_count: u64,
    pub last_used_at: Option<String>,
    pub last_used_by: Option<String>,
}

pub struct Variables {
    records: Arc<RwLock<Vec<VariableRecord>>>,
    storage: Arc<dyn Storage>,
}

impl Variables {
    pub fn new(storage: Arc<dyn Storage>, records: Vec<VariableRecord>) -> Self {
        Self {
            records: Arc::new(RwLock::new(records)),
            storage,
        }
    }

    // -- REPL-facing ---------------------------------------------------------

    pub fn list_global(&self) -> Vec<VariableInfo> {
        let mut items: Vec<VariableInfo> = self
            .records
            .read()
            .iter()
            .filter(|r| r.scope == VariableScope::Global)
            .map(|r| VariableInfo {
                key: r.key.clone(),
                value: r.value.clone(),
                used_count: r.used_count,
                last_used_at: r.last_used_at.clone(),
                last_used_by: r.last_used_by.clone(),
            })
            .collect();

        items.sort_by(|a, b| {
            b.used_count
                .cmp(&a.used_count)
                .then_with(|| b.last_used_at.cmp(&a.last_used_at))
                .then_with(|| a.key.cmp(&b.key))
        });

        items
    }

    pub async fn set_global(&self, key: String, value: String) -> Result<()> {
        {
            let mut recs = self.records.write();
            let now = now_iso8601();
            if let Some(existing) = recs
                .iter_mut()
                .find(|r| r.key == key && r.scope == VariableScope::Global)
            {
                existing.value = value;
                existing.updated_at = now;
            } else {
                recs.push(VariableRecord {
                    key,
                    value,
                    scope: VariableScope::Global,
                    project_id: None,
                    session_id: None,
                    secret: true,
                    updated_at: now,
                    used_count: 0,
                    last_used_at: None,
                    last_used_by: None,
                });
            }
        }
        self.save().await
    }

    pub async fn delete_global(&self, key: &str) -> Result<bool> {
        let removed = {
            let mut recs = self.records.write();
            let before = recs.len();
            recs.retain(|r| !(r.key == key && r.scope == VariableScope::Global));
            recs.len() < before
        };
        if removed {
            self.save().await?;
        }
        Ok(removed)
    }

    pub fn has_variables(&self) -> bool {
        !self.records.read().is_empty()
    }

    /// Return all secret variable values (for display-layer masking).
    pub fn secret_values(&self) -> Vec<String> {
        self.records
            .read()
            .iter()
            .filter(|r| r.secret)
            .map(|r| r.value.clone())
            .collect()
    }

    // -- Tool-facing ---------------------------------------------------------

    pub async fn get_for_context(
        &self,
        name: &str,
        cwd: &str,
        session_id: &str,
    ) -> std::result::Result<GetVariableResponse, String> {
        let result = {
            let mut recs = self.records.write();
            let idx = resolve_variable(&recs, name, cwd, session_id);
            match idx {
                Some(i) => {
                    let value = recs[i].value.clone();
                    recs[i].used_count += 1;
                    recs[i].last_used_at = Some(now_iso8601());
                    recs[i].last_used_by = Some(session_id.to_string());
                    let all = recs.clone();
                    Some((value, all))
                }
                None => None,
            }
        }; // guard dropped here

        match result {
            Some((value, all)) => {
                if let Err(err) = self.storage.save_variables(all).await {
                    logx!(warn, "variables", "save_error", error = %err,);
                }
                Ok(GetVariableResponse::Found(value))
            }
            None => Ok(GetVariableResponse::NotFound),
        }
    }

    pub fn as_get_fn(&self, cwd: &str, session_id: &str) -> GetVariableFn {
        let records = self.records.clone();
        let storage = self.storage.clone();
        let cwd = cwd.to_string();
        let session_id = session_id.to_string();
        Arc::new(move |name: String| -> BoxFuture<'static, std::result::Result<GetVariableResponse, String>> {
            let records = records.clone();
            let storage = storage.clone();
            let cwd = cwd.clone();
            let session_id = session_id.clone();
            Box::pin(async move {
                let vars = Variables {
                    records,
                    storage,
                };
                vars.get_for_context(&name, &cwd, &session_id).await
            })
        })
    }

    // -- Internal ------------------------------------------------------------

    async fn save(&self) -> Result<()> {
        let all = self.records.read().clone();
        self.storage.save_variables(all).await
    }
}

// ---------------------------------------------------------------------------
// Scope resolution: session > project > global
// ---------------------------------------------------------------------------

fn resolve_variable(
    records: &[VariableRecord],
    name: &str,
    cwd: &str,
    session_id: &str,
) -> Option<usize> {
    // 1. session match
    let session_match = records.iter().position(|r| {
        r.key == name
            && r.scope == VariableScope::Session
            && r.session_id.as_deref() == Some(session_id)
    });
    if session_match.is_some() {
        return session_match;
    }

    // 2. project match
    let project_match = records.iter().position(|r| {
        r.key == name && r.scope == VariableScope::Project && r.project_id.as_deref() == Some(cwd)
    });
    if project_match.is_some() {
        return project_match;
    }

    // 3. global match
    records
        .iter()
        .position(|r| r.key == name && r.scope == VariableScope::Global)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
