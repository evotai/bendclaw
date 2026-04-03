use async_trait::async_trait;

use crate::types::entities::Channel;
use crate::types::Result;

#[async_trait]
pub trait ChannelRepo: Send + Sync {
    async fn get_channel(
        &self,
        user_id: &str,
        agent_id: &str,
        channel_id: &str,
    ) -> Result<Option<Channel>>;
    async fn save_channel(&self, channel: &Channel) -> Result<()>;
    async fn delete_channel(&self, user_id: &str, agent_id: &str, channel_id: &str) -> Result<()>;
    async fn list_channels(&self, user_id: &str, agent_id: &str) -> Result<Vec<Channel>>;
}
