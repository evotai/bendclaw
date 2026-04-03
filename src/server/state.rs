use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::runtime::Runtime;

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<Runtime>,
    pub auth_key: String,
    pub shutdown_token: CancellationToken,
}
