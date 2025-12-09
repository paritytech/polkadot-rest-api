//! Route registry for dynamic endpoint introspection.
//!
//! This module provides a registry that tracks all registered routes,
//! allowing the root endpoint to return a list of available routes
//! similar to substrate-api-sidecar.

use axum::{Router, routing::MethodRouter};
use serde::Serialize;
use std::sync::{Arc, RwLock};

/// Current API version prefix for all routes.
pub const API_VERSION: &str = "/v1";

/// Information about a registered route.
#[derive(Clone, Serialize)]
pub struct RouteInfo {
    /// The path pattern (e.g., "/blocks/{blockId}")
    pub path: String,
    /// The HTTP method (e.g., "get", "post")
    pub method: String,
}

/// A thread-safe registry of routes.
///
/// Routes are registered as they are added to the router,
/// and can be retrieved later for introspection.
#[derive(Clone, Default)]
pub struct RouteRegistry(Arc<RwLock<Vec<RouteInfo>>>);

impl RouteRegistry {
    /// Create a new empty route registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a route to the registry.
    pub fn add(&self, path: &str, method: &str) {
        if let Ok(mut routes) = self.0.write() {
            routes.push(RouteInfo {
                path: path.to_string(),
                method: method.to_string(),
            });
        }
    }

    /// Get all registered routes.
    pub fn routes(&self) -> Vec<RouteInfo> {
        self.0.read().map(|r| r.clone()).unwrap_or_default()
    }
}

/// Extension trait for registering routes with automatic registry tracking.
pub trait RegisterRoute<S: Clone + Send + Sync + 'static> {
    /// Register a route and track it in the registry.
    ///
    /// # Arguments
    /// * `registry` - The route registry to add the route to
    /// * `prefix` - The prefix to prepend to the path in the registry (e.g., "/v1")
    /// * `path` - The route path (used for both routing and registry, with prefix prepended to registry)
    /// * `method` - The HTTP method (e.g., "get", "post")
    /// * `handler` - The route handler
    fn route_registered(
        self,
        registry: &RouteRegistry,
        prefix: &str,
        path: &str,
        method: &str,
        handler: MethodRouter<S>,
    ) -> Self;
}

impl<S: Clone + Send + Sync + 'static> RegisterRoute<S> for Router<S> {
    fn route_registered(
        self,
        registry: &RouteRegistry,
        prefix: &str,
        path: &str,
        method: &str,
        handler: MethodRouter<S>,
    ) -> Self {
        // Add to registry with prefix, converting :param to {param} for display
        let display_path = convert_path_params_for_display(path);
        let full_path = format!("{}{}", prefix, display_path);
        registry.add(&full_path, method);
        // Route without prefix (since routes are nested)
        self.route(path, handler)
    }
}

/// Convert Axum path parameters (:param) to OpenAPI style ({param}) for display
fn convert_path_params_for_display(path: &str) -> String {
    let mut result = String::new();
    let mut chars = path.chars().peekable();

    while let Some(c) = chars.next() {
        if c == ':' {
            // Collect the parameter name
            let mut param = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_alphanumeric() || next == '_' {
                    param.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            result.push('{');
            result.push_str(&param);
            result.push('}');
        } else {
            result.push(c);
        }
    }

    result
}
