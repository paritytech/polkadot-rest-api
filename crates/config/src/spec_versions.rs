use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Runtime version changes for a chain
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SpecVersionChanges {
    pub changes: BTreeMap<u64, u32>,
}

impl SpecVersionChanges {
    pub fn new(changes: BTreeMap<u64, u32>) -> Self {
        Self { changes }
    }

    pub fn get_version_at_block(&self, block_number: u64) -> Option<u32> {
        self.changes
            .range(..=block_number)
            .next_back()
            .map(|(_, version)| *version)
    }

    pub fn all_versions(&self) -> Vec<(u64, u32)> {
        self.changes.iter().map(|(b, v)| (*b, *v)).collect()
    }

    pub fn version_changed_at(&self, block_number: u64) -> bool {
        self.changes.contains_key(&block_number)
    }
}

impl Default for SpecVersionChanges {
    fn default() -> Self {
        let mut changes = BTreeMap::new();
        changes.insert(0, 0);
        Self { changes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_version_lookup() {
        let mut changes = BTreeMap::new();
        changes.insert(0, 1000);
        changes.insert(1000, 1001);
        changes.insert(5000, 1002);

        let spec_versions = SpecVersionChanges::new(changes);

        assert_eq!(spec_versions.get_version_at_block(0), Some(1000));
        assert_eq!(spec_versions.get_version_at_block(500), Some(1000));
        assert_eq!(spec_versions.get_version_at_block(1000), Some(1001));
        assert_eq!(spec_versions.get_version_at_block(3000), Some(1001));
        assert_eq!(spec_versions.get_version_at_block(5000), Some(1002));
        assert_eq!(spec_versions.get_version_at_block(10000), Some(1002));
    }

    #[test]
    fn test_version_changed_at() {
        let mut changes = BTreeMap::new();
        changes.insert(0, 1000);
        changes.insert(1000, 1001);

        let spec_versions = SpecVersionChanges::new(changes);

        assert!(spec_versions.version_changed_at(0));
        assert!(spec_versions.version_changed_at(1000));
        assert!(!spec_versions.version_changed_at(500));
    }
}
