use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthCheck {
    status: &'static str,
}

pub async fn health_check() -> Json<HealthCheck> {
    Json(HealthCheck { status: "ok" })
}
