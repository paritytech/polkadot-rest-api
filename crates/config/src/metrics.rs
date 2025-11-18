use serde::Deserialize;
use std::net::IpAddr;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("Invalid host address: {0}")]
    InvalidHost(String),

    #[error("Port must be between 1 and 65535, got {0}")]
    InvalidPort(u16),

    #[error(
        "Invalid Prometheus prefix '{0}': must start with [a-zA-Z_:] and contain only [a-zA-Z0-9_:]"
    )]
    InvalidPrometheusPrefix(String),
}

/// Configuration for Prometheus metrics
#[derive(Debug, Clone, Deserialize)]
pub struct MetricsConfig {
    /// Enable or disable metrics collection
    pub enabled: bool,

    /// Prometheus server host
    pub prom_host: String,

    /// Prometheus server port
    pub prom_port: u16,

    /// Prometheus metric name prefix (default: "polkadot_rest_api")
    pub prometheus_prefix: String,

    /// Include query parameters in route labels (matches sidecar's INCLUDE_QUERYPARAMS)
    pub include_queryparams: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "polkadot_rest_api".to_string(),
            include_queryparams: false,
        }
    }
}

impl MetricsConfig {
    pub fn validate(&self) -> Result<(), MetricsError> {
        // Validate host is a valid IP address
        if IpAddr::from_str(&self.prom_host).is_err() {
            return Err(MetricsError::InvalidHost(self.prom_host.clone()));
        }

        // Validate port is in valid range
        if self.prom_port == 0 {
            return Err(MetricsError::InvalidPort(self.prom_port));
        }

        // Validate prometheus_prefix follows Prometheus naming conventions
        // Must match regex: [a-zA-Z_:][a-zA-Z0-9_:]*
        // This ensures metrics won't be silently rejected by Prometheus
        if !self.prometheus_prefix.is_empty() {
            let first_char = self.prometheus_prefix.chars().next().unwrap();
            if !first_char.is_ascii_alphabetic() && first_char != '_' && first_char != ':' {
                return Err(MetricsError::InvalidPrometheusPrefix(
                    self.prometheus_prefix.clone(),
                ));
            }

            for ch in self.prometheus_prefix.chars() {
                if !ch.is_ascii_alphanumeric() && ch != '_' && ch != ':' {
                    return Err(MetricsError::InvalidPrometheusPrefix(
                        self.prometheus_prefix.clone(),
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_metrics_config() {
        let config = MetricsConfig::default();
        assert_eq!(config.enabled, false);
        assert_eq!(config.prom_host, "127.0.0.1");
        assert_eq!(config.prom_port, 9100);
        assert_eq!(config.prometheus_prefix, "polkadot_rest_api");
        assert_eq!(config.include_queryparams, false);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_valid_ipv6_host() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "::1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "test".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_host() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "not-an-ip".to_string(),
            prom_port: 9100,
            prometheus_prefix: "test".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_port() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 0,
            prometheus_prefix: "test".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_valid_prometheus_prefix_with_underscore() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "my_app_metrics".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_valid_prometheus_prefix_with_colon() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "app:metrics".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_valid_prometheus_prefix_starting_with_underscore() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "_metrics".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_prometheus_prefix_starting_with_number() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "123metrics".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_prometheus_prefix_with_hyphen() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "my-metrics".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_prometheus_prefix_with_special_chars() {
        let config = MetricsConfig {
            enabled: true,
            prom_host: "127.0.0.1".to_string(),
            prom_port: 9100,
            prometheus_prefix: "my.metrics".to_string(),
            include_queryparams: false,
        };
        assert!(config.validate().is_err());
    }
}
