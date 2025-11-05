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

    /// Load a JSON fixture file
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

    /// Check if a fixture file exists
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        let full_path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            self.fixtures_dir.join(path.as_ref())
        };
        full_path.exists()
    }

    /// Get the fixtures directory path
    pub fn fixtures_dir(&self) -> &Path {
        &self.fixtures_dir
    }
}


