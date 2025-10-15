use crate::ConfigError;

#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Log Level
    ///
    /// Env: SAS_LOG_LEVEL
    /// Valid values: trace, debug, info, warn, error
    /// Default: info
    pub level: String,
}

fn default_level() -> String {
    "info".to_string()
}

impl LogConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        let valid_levels = ["trace", "debug", "info", "warn", "error"];

        if !valid_levels.contains(&self.level.as_str()) {
            return Err(ConfigError::ValidateError(format!(
                "Invalid log level '{}'. Must be one of: {}",
                self.level,
                valid_levels.join(", ")
            )));
        }

        Ok(())
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_log_config() {
        let config = LogConfig::default();
        assert_eq!(config.level, "info");
    }

    #[test]
    fn test_validate_valid_levels() {
        for level in ["trace", "debug", "info", "warn", "error"] {
            let config = LogConfig {
                level: level.to_string(),
            };
            assert!(config.validate().is_ok(), "Level {} should be valid", level);
        }
    }

    #[test]
    fn test_validate_invalid_levels() {
        let config = LogConfig {
            level: "invalid".to_string(),
        };
        assert!(config.validate().is_err());
    }
}
