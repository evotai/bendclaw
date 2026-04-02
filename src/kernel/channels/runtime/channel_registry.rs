use std::collections::HashMap;
use std::sync::Arc;

use super::channel_trait::ChannelPlugin;
use super::channel_trait::InboundKind;

/// A registered channel entry: plugin + its inbound kind.
pub struct ChannelEntry {
    pub plugin: Arc<dyn ChannelPlugin>,
    pub inbound: InboundKind,
}

/// Registry of channel plugins, keyed by channel_type.
pub struct ChannelRegistry {
    entries: HashMap<String, ChannelEntry>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn ChannelPlugin>) {
        let inbound = plugin.inbound();
        self.entries
            .insert(plugin.channel_type().to_string(), ChannelEntry {
                plugin,
                inbound,
            });
    }

    pub fn get(&self, channel_type: &str) -> Option<&ChannelEntry> {
        self.entries.get(channel_type)
    }

    pub fn list(&self) -> Vec<&str> {
        let mut types: Vec<&str> = self.entries.keys().map(|s| s.as_str()).collect();
        types.sort();
        types
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}
