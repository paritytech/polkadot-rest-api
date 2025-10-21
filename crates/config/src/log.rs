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

    /// Whether to write logs to a log file (logs.log)
    ///
    /// Env: SAS_LOG_WRITE
    /// Default: false
    /// Note: Only logs what is available based on SAS_LOG_LEVEL
    pub write: bool,

    /// Path to write the log files
    ///
    /// Env: SAS_LOG_WRITE_PATH
    /// Default: ./logs
    pub write_path: String,

    /// The max size the log file should not exceed (in bytes)
    ///
    /// Env: SAS_LOG_WRITE_MAX_FILE_SIZE
    /// Default: 5242880 (5MB)
    pub write_max_file_size: u64,

    /// The max number of log files to keep
    ///
    /// Env: SAS_LOG_WRITE_MAX_FILES
    /// Default: 5
    pub write_max_files: usize,
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

fn default_write() -> bool {
    false
}

fn default_write_path() -> String {
    "./logs".to_string()
}

fn default_write_max_file_size() -> u64 {
    5_242_880 // 5MB
}

fn default_write_max_files() -> usize {
    5
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

        if self.write_max_file_size == 0 {
            return Err(ConfigError::ValidateError(
                "Log write max file size must be greater than 0".to_string(),
            ));
        }

        if self.write_max_file_size < 1024 {
            return Err(ConfigError::ValidateError(
                "Log write max file size must be at least 1KB (1024 bytes)".to_string(),
            ));
        }

        if self.write_max_files == 0 {
            return Err(ConfigError::ValidateError(
                "Log write max files must be at least 1".to_string(),
            ));
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
            write: default_write(),
            write_path: default_write_path(),
            write_max_file_size: default_write_max_file_size(),
            write_max_files: default_write_max_files(),
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
        assert_eq!(config.write, false);
        assert_eq!(config.write_path, "./logs");
        assert_eq!(config.write_max_file_size, 5_242_880);
        assert_eq!(config.write_max_files, 5);
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

    #[test]
    fn test_write_enabled() {
        let config = LogConfig {
            write: true,
            ..Default::default()
        };
        assert_eq!(config.write, true);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_write_disabled() {
        let config = LogConfig {
            write: false,
            ..Default::default()
        };
        assert_eq!(config.write, false);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_write_path_default() {
        let config = LogConfig::default();
        assert_eq!(config.write_path, "./logs");
    }

    #[test]
    fn test_write_path_custom() {
        let config = LogConfig {
            write_path: "/var/log".to_string(),
            ..Default::default()
        };
        assert_eq!(config.write_path, "/var/log");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_write_max_file_size_default() {
        let config = LogConfig::default();
        assert_eq!(config.write_max_file_size, 5_242_880); // 5MB
    }

    #[test]
    fn test_write_max_file_size_custom() {
        let config = LogConfig {
            write_max_file_size: 10_485_760, // 10MB
            ..Default::default()
        };
        assert_eq!(config.write_max_file_size, 10_485_760);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_write_max_file_size_zero() {
        let config = LogConfig {
            write_max_file_size: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_write_max_file_size_too_small() {
        let config = LogConfig {
            write_max_file_size: 512, // Less than 1KB
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_write_max_file_size_minimum() {
        let config = LogConfig {
            write_max_file_size: 1024, // Exactly 1KB
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_write_max_files_default() {
        let config = LogConfig::default();
        assert_eq!(config.write_max_files, 5);
    }

    #[test]
    fn test_write_max_files_custom() {
        let config = LogConfig {
            write_max_files: 10,
            ..Default::default()
        };
        assert_eq!(config.write_max_files, 10);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_write_max_files_zero() {
        let config = LogConfig {
            write_max_files: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_write_max_files_minimum() {
        let config = LogConfig {
            write_max_files: 1,
            ..Default::default()
        };
        assert_eq!(config.write_max_files, 1);
        assert!(config.validate().is_ok());
    }
}
