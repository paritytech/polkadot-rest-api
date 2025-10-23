use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExpressError {
    #[error("Express port cannot be 0")]
    PortZero,

    #[error("Invalid bind host '{host}': {source}. Must be a valid IP address (IPv4 or IPv6)")]
    InvalidBindHost {
        host: String,
        #[source]
        source: std::net::AddrParseError,
    },

    #[error("Request limit cannot be 0")]
    RequestLimitZero,

    #[error("Keep-alive timeout cannot be 0")]
    KeepAliveTimeoutZero,
}

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

    /// Keep-alive timeout in milliseconds
    ///
    /// Env: SAS_EXPRESS_KEEP_ALIVE_TIMEOUT
    /// Default: 5000
    pub keep_alive_timeout: u64,
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

fn default_keep_alive_timeout() -> u64 {
    5000 // 5 seconds in milliseconds
}

impl ExpressConfig {
    pub(crate) fn validate(&self) -> Result<(), ExpressError> {
        // Validate port
        if self.port == 0 {
            return Err(ExpressError::PortZero);
        }

        // Validate bind_host is a valid IP address
        self.bind_host
            .parse::<std::net::IpAddr>()
            .map_err(|source| ExpressError::InvalidBindHost {
                host: self.bind_host.clone(),
                source,
            })?;

        // Validate request_limit is not zero
        if self.request_limit == 0 {
            return Err(ExpressError::RequestLimitZero);
        }

        // Validate keep_alive_timeout is not zero
        if self.keep_alive_timeout == 0 {
            return Err(ExpressError::KeepAliveTimeoutZero);
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
            keep_alive_timeout: default_keep_alive_timeout(),
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
            port: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_port_valid() {
        let config = ExpressConfig {
            port: 3000,
            ..Default::default()
        };
        assert!(config.validate().is_ok())
    }

    #[test]
    fn test_validate_bind_host_ipv4() {
        let config = ExpressConfig {
            bind_host: "192.168.1.100".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bind_host_all_interfaces() {
        let config = ExpressConfig {
            bind_host: "0.0.0.0".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bind_host_ipv6() {
        let config = ExpressConfig {
            bind_host: "::1".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bind_host_invalid() {
        let config = ExpressConfig {
            bind_host: "not-an-ip".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_bind_host_hostname() {
        let config = ExpressConfig {
            bind_host: "localhost".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_request_limit_zero() {
        let config = ExpressConfig {
            request_limit: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_request_limit_valid() {
        let config = ExpressConfig {
            request_limit: 1024,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_keep_alive_timeout_zero() {
        let config = ExpressConfig {
            keep_alive_timeout: 0,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_keep_alive_timeout_valid() {
        let config = ExpressConfig {
            keep_alive_timeout: 10000,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }
}
