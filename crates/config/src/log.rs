use crate::ConfigError;

#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Log Level
    ///
    /// Env: SAS_LOG_LEVEL
    /// Valid values: trace, debug, info, warn, error
    /// Default: info
    pub level: String,

    /// Output logs in JSON format
    ///
    /// Env: SAS_LOG_JSON
    /// Default: false
    pub json: bool,

    /// Strip ANSI color codes from logs
    ///
    /// Env: SAS_LOG_STRIP_ANSI
    /// Default: false
    pub strip_ansi: bool,
}

fn default_level() -> String {
    "info".to_string()
}

fn default_json() -> bool {
    false
}

fn default_strip_ansi() -> bool {
    false
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
            json: default_json(),
            strip_ansi: default_strip_ansi(),
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
        assert_eq!(config.json, false);
        assert_eq!(config.strip_ansi, false);
    }

    #[test]
    fn test_validate_valid_levels() {
        for level in ["trace", "debug", "info", "warn", "error"] {
            let config = LogConfig {
                level: level.to_string(),
                ..Default::default()
            };
            assert!(config.validate().is_ok(), "Level {} should be valid", level);
        }
    }

    #[test]
    fn test_validate_invalid_levels() {
        let config = LogConfig {
            level: "invalid".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_strip_ansi_enabled() {
        let config = LogConfig {
            strip_ansi: true,
            ..Default::default()
        };
        assert_eq!(config.strip_ansi, true);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_strip_ansi_disabled() {
        let config = LogConfig {
            strip_ansi: false,
            ..Default::default()
        };
        assert_eq!(config.strip_ansi, false);
        assert!(config.validate().is_ok());
    }
}
