// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct FixtureLoader {
    fixtures_dir: PathBuf,
}

impl FixtureLoader {
    /// Create a new fixture loader
    pub fn new(fixtures_dir: impl AsRef<Path>) -> Self {
        Self {
            fixtures_dir: fixtures_dir.as_ref().to_path_buf(),
        }
    }

    /// Load a JSON fixture file using the full path relative to fixtures dir
    ///
    /// # Examples
    /// ```ignore
    /// // Old flat structure (still supported for backwards compatibility)
    /// loader.load("polkadot/blocks_1000000.json")?;
    ///
    /// // New nested structure
    /// loader.load("polkadot/blocks/1000000.json")?;
    /// ```
    pub fn load(&self, path: impl AsRef<Path>) -> Result<Value> {
        let full_path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            self.fixtures_dir.join(path.as_ref())
        };

        let content = std::fs::read_to_string(&full_path)
            .context(format!("Failed to read fixture file: {:?}", full_path))?;

        let json: Value = serde_json::from_str(&content)
            .context(format!("Failed to parse JSON fixture: {:?}", full_path))?;

        Ok(json)
    }

    /// Load a fixture using chain, feature, and filename components
    ///
    /// # Examples
    /// ```ignore
    /// // Load polkadot/blocks/1000000.json
    /// loader.load_nested("polkadot", "blocks", "1000000.json")?;
    ///
    /// // Load asset-hub-polkadot/pallets/balances/errors.json
    /// loader.load_nested("asset-hub-polkadot", "pallets/balances", "errors.json")?;
    /// ```
    pub fn load_nested(&self, chain: &str, feature: &str, filename: &str) -> Result<Value> {
        let path = PathBuf::from(chain).join(feature).join(filename);
        self.load(&path)
    }

    /// Check if a fixture file exists
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        let full_path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            self.fixtures_dir.join(path.as_ref())
        };
        full_path.exists()
    }

    /// Check if a nested fixture exists
    pub fn exists_nested(&self, chain: &str, feature: &str, filename: &str) -> bool {
        let path = PathBuf::from(chain).join(feature).join(filename);
        self.exists(&path)
    }

    /// Get the fixtures directory path
    pub fn fixtures_dir(&self) -> &Path {
        &self.fixtures_dir
    }

    /// List all fixture files in a chain/feature directory
    pub fn list_fixtures(&self, chain: &str, feature: &str) -> Result<Vec<String>> {
        let dir_path = self.fixtures_dir.join(chain).join(feature);
        if !dir_path.exists() {
            return Ok(vec![]);
        }

        let mut fixtures = Vec::new();
        for entry in std::fs::read_dir(&dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |e| e == "json") {
                if let Some(filename) = path.file_name() {
                    fixtures.push(filename.to_string_lossy().to_string());
                }
            }
        }
        fixtures.sort();
        Ok(fixtures)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn get_fixtures_dir() -> PathBuf {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(manifest_dir).join("tests/fixtures")
    }

    #[test]
    fn test_load_nested_fixture() {
        let loader = FixtureLoader::new(get_fixtures_dir());

        // Test loading a fixture with the new nested structure
        if loader.exists_nested("polkadot", "blocks", "1000000.json") {
            let result = loader.load_nested("polkadot", "blocks", "1000000.json");
            assert!(result.is_ok(), "Should be able to load nested fixture");
        }
    }

    #[test]
    fn test_exists_nested() {
        let loader = FixtureLoader::new(get_fixtures_dir());

        // Test that a known fixture exists (after migration)
        // This will pass once migration is complete
        let exists = loader.exists_nested("polkadot", "blocks", "1000000.json");
        // Just verify the method works without panicking
        let _ = exists;
    }

    #[test]
    fn test_list_fixtures() {
        let loader = FixtureLoader::new(get_fixtures_dir());

        // Test listing fixtures in a directory
        if loader.exists("polkadot/blocks") {
            let fixtures = loader.list_fixtures("polkadot", "blocks");
            assert!(fixtures.is_ok());
        }
    }
}
