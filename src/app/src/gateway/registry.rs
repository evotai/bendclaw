use std::sync::Arc;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent::Agent;
use crate::conf::ChannelsConfig;

pub fn spawn_all(
    conf: &ChannelsConfig,
    agent: Arc<Agent>,
    cancel: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let mut handles = vec![];

    if let Some(ref fc) = conf.feishu {
        handles.push(super::channels::feishu::FeishuChannel::spawn(
            fc.clone(),
            agent.clone(),
            cancel.clone(),
        ));
    }

    handles
}
