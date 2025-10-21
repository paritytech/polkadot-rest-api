use crate::ConfigError;

// "Express" naming is an artifact of substrate-api-sidecar that is
// being carried over to maintain backwards compatibility.
#[derive(Debug, Clone)]
pub struct ExpressConfig {
    /// Host address to bind the HTTP server to
    ///
    /// Env: SAS_EXPRESS_BIND_HOST
    /// Default: 127.0.0.1
    pub bind_host: String,

    /// Port to bind the HTTP server to
    ///
    /// Env: SAS_EXPRESS_PORT
    /// Default: 8080
    pub port: u16,

    /// Maximum request body size in bytes
    ///
    /// Env: SAS_EXPRESS_REQUEST_LIMIT
    /// Default: 512000 (500kb)
    pub request_limit: usize,
}

fn default_bind_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_request_limit() -> usize {
    512_000 // 500kb
}

impl ExpressConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        // Validate port
        if self.port == 0 {
            return Err(ConfigError::ValidateError(
                "Express port cannot be 0".to_string(),
            ));
        }

        // Validate bind_host is a valid IP address
        self.bind_host.parse::<std::net::IpAddr>().map_err(|e| {
            ConfigError::ValidateError(format!(
                "Invalid bind host '{}': {}. Must be a valid IP address (IPv4 or IPv6)",
                self.bind_host, e
            ))
        })?;

        if self.request_limit == 0 {
            return Err(ConfigError::ValidateError(
                "Request limit cannot be 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for ExpressConfig {
    fn default() -> Self {
        Self {
            bind_host: default_bind_host(),
            port: default_port(),
            request_limit: default_request_limit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_express_config() {
        let config = ExpressConfig::default();
        assert_eq!(config.bind_host, "127.0.0.1");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_validate_port_zero() {
        let config = ExpressConfig {
            bind_host: "127.0.0.1".to_string(),
            port: 0,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_port_valid() {
        let config = ExpressConfig {
            bind_host: "127.0.0.1".to_string(),
            port: 3000,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_ok())
    }

    #[test]
    fn test_validate_bind_host_ipv4() {
        let config = ExpressConfig {
            bind_host: "192.168.1.100".to_string(),
            port: 8080,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bind_host_all_interfaces() {
        let config = ExpressConfig {
            bind_host: "0.0.0.0".to_string(),
            port: 8080,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bind_host_ipv6() {
        let config = ExpressConfig {
            bind_host: "::1".to_string(),
            port: 8080,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bind_host_invalid() {
        let config = ExpressConfig {
            bind_host: "not-an-ip".to_string(),
            port: 8080,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_bind_host_hostname() {
        let config = ExpressConfig {
            bind_host: "localhost".to_string(),
            port: 8080,
            request_limit: default_request_limit(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_request_limit_zero() {
        let config = ExpressConfig {
            bind_host: "127.0.0.1".to_string(),
            port: 8080,
            request_limit: 0,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_request_limit_valid() {
        let config = ExpressConfig {
            bind_host: "127.0.0.1".to_string(),
            port: 8080,
            request_limit: 1024,
        };
        assert!(config.validate().is_ok());
    }
}
