use std::sync::Arc;

use crate::kernel::Runtime;

#[derive(Clone)]
pub struct AppState {
    pub runtime: Arc<Runtime>,
    pub auth_key: String,
}
