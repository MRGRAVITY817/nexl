//! Resource lifecycle verification (spec §15.1).
//!
//! Resources from WASM Component Model imports must be properly closed or
//! transferred. This module provides a simple lifecycle tracker that verifies
//! resources are consumed exactly once.

use std::collections::HashMap;

/// The state of a resource in the lifecycle tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceState {
    /// Resource has been created/acquired but not yet consumed.
    Live,
    /// Resource has been closed/consumed.
    Closed,
    /// Resource has been transferred (e.g. returned from function).
    Transferred,
}

/// A resource lifecycle error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceLifecycleError {
    /// Resource was never closed or transferred.
    Leaked { name: String },
    /// Resource was closed more than once.
    DoubleFree { name: String },
    /// Resource was used after being closed.
    UseAfterClose { name: String },
}

impl std::fmt::Display for ResourceLifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceLifecycleError::Leaked { name } => {
                write!(f, "resource `{name}` was never closed or transferred")
            }
            ResourceLifecycleError::DoubleFree { name } => {
                write!(f, "resource `{name}` was closed more than once")
            }
            ResourceLifecycleError::UseAfterClose { name } => {
                write!(f, "resource `{name}` used after close")
            }
        }
    }
}

impl std::error::Error for ResourceLifecycleError {}

/// Tracks the lifecycle of named resources within a scope.
#[derive(Debug, Clone)]
pub struct ResourceTracker {
    resources: HashMap<String, ResourceState>,
}

impl ResourceTracker {
    /// Create a new empty tracker.
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
        }
    }

    /// Record that a resource has been created/acquired.
    pub fn create(&mut self, name: impl Into<String>) {
        self.resources.insert(name.into(), ResourceState::Live);
    }

    /// Record that a resource has been closed/consumed.
    pub fn close(&mut self, name: &str) -> Result<(), ResourceLifecycleError> {
        match self.resources.get(name) {
            Some(ResourceState::Live) => {
                self.resources
                    .insert(name.to_string(), ResourceState::Closed);
                Ok(())
            }
            Some(ResourceState::Closed) => Err(ResourceLifecycleError::DoubleFree {
                name: name.to_string(),
            }),
            Some(ResourceState::Transferred) => Err(ResourceLifecycleError::UseAfterClose {
                name: name.to_string(),
            }),
            None => Err(ResourceLifecycleError::UseAfterClose {
                name: name.to_string(),
            }),
        }
    }

    /// Record that a resource has been transferred (e.g. returned).
    pub fn transfer(&mut self, name: &str) -> Result<(), ResourceLifecycleError> {
        match self.resources.get(name) {
            Some(ResourceState::Live) => {
                self.resources
                    .insert(name.to_string(), ResourceState::Transferred);
                Ok(())
            }
            Some(ResourceState::Closed) => Err(ResourceLifecycleError::UseAfterClose {
                name: name.to_string(),
            }),
            Some(ResourceState::Transferred) => Err(ResourceLifecycleError::DoubleFree {
                name: name.to_string(),
            }),
            None => Err(ResourceLifecycleError::UseAfterClose {
                name: name.to_string(),
            }),
        }
    }

    /// Verify that all resources have been properly closed or transferred.
    ///
    /// Returns errors for any leaked resources.
    pub fn verify(&self) -> Vec<ResourceLifecycleError> {
        let mut errors = Vec::new();
        let mut names: Vec<_> = self.resources.keys().collect();
        names.sort(); // deterministic ordering
        for name in names {
            if self.resources[name] == ResourceState::Live {
                errors.push(ResourceLifecycleError::Leaked { name: name.clone() });
            }
        }
        errors
    }
}

impl Default for ResourceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──

    #[test]
    fn test_resource_lifecycle_valid() {
        let mut tracker = ResourceTracker::new();
        tracker.create("conn");
        tracker.close("conn").unwrap();
        assert!(tracker.verify().is_empty());
    }

    // ── Test 2 ──

    #[test]
    fn test_resource_lifecycle_leaked() {
        let mut tracker = ResourceTracker::new();
        tracker.create("conn");
        let errors = tracker.verify();
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            ResourceLifecycleError::Leaked { name } => assert_eq!(name, "conn"),
            other => panic!("expected Leaked, got {other:?}"),
        }
    }

    // ── Test 3 ──

    #[test]
    fn test_resource_lifecycle_double_close() {
        let mut tracker = ResourceTracker::new();
        tracker.create("conn");
        tracker.close("conn").unwrap();
        let err = tracker.close("conn").unwrap_err();
        match err {
            ResourceLifecycleError::DoubleFree { name } => assert_eq!(name, "conn"),
            other => panic!("expected DoubleFree, got {other:?}"),
        }
    }

    // ── Test 4 ──

    #[test]
    fn test_resource_lifecycle_transferred() {
        let mut tracker = ResourceTracker::new();
        tracker.create("conn");
        tracker.transfer("conn").unwrap();
        assert!(tracker.verify().is_empty());
    }

    // ── Test 5 ──

    #[test]
    fn test_resource_lifecycle_multiple() {
        let mut tracker = ResourceTracker::new();
        tracker.create("conn1");
        tracker.create("conn2");
        tracker.create("conn3");
        tracker.close("conn1").unwrap();
        tracker.transfer("conn2").unwrap();
        // conn3 is leaked
        let errors = tracker.verify();
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            ResourceLifecycleError::Leaked { name } => assert_eq!(name, "conn3"),
            other => panic!("expected Leaked, got {other:?}"),
        }
    }
}
