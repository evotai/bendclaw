use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::state::AppState;
use crate::version;

#[derive(Serialize)]
pub struct ServiceCheck {
    ok: bool,
}

#[derive(Serialize)]
pub struct HealthChecks {
    service: ServiceCheck,
}

#[derive(Serialize)]
pub struct HealthCheck {
    status: &'static str,
    version: String,
    node_id: String,
    checks: HealthChecks,
}

pub async fn health_check(State(state): State<AppState>) -> Json<HealthCheck> {
    Json(HealthCheck {
        status: "healthy",
        version: version::commit_version(),
        node_id: state.runtime.config.node_id.clone(),
        checks: HealthChecks {
            service: ServiceCheck { ok: true },
        },
    })
}
