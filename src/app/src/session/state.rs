use crate::storage::model::SessionMeta;

pub struct SessionState {
    pub meta: SessionMeta,
    pub messages: Vec<bend_agent::Message>,
}

impl SessionState {
    pub fn new(meta: SessionMeta, messages: Vec<bend_agent::Message>) -> Self {
        Self { meta, messages }
    }
}
