use super::DiscoveredHost;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct DiscoveryTracker {
    hosts: BTreeMap<String, DiscoveredHost>,
}

impl DiscoveryTracker {
    pub fn new() -> Self {
        Self {
            hosts: BTreeMap::new(),
        }
    }

    pub fn upsert(&mut self, host: DiscoveredHost) {
        self.hosts.insert(host.id.clone(), host);
    }

    pub fn remove(&mut self, id: &str) {
        self.hosts.remove(id);
    }

    pub fn snapshot(&self) -> Vec<DiscoveredHost> {
        self.hosts.values().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.hosts.clear();
    }
}
