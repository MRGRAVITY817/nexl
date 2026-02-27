//! Runtime sandbox policy for restricting effectful operations.
//!
//! In Stage 0, the sandbox uses a thread-local policy that effectful stdlib
//! functions check before performing I/O. The `nexl sandbox` command sets
//! the policy before evaluation.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;

/// A capability that can be granted or denied by the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Console I/O (stdout, stderr).
    Console,
    /// File-system access (read/write files, directories).
    FileSystem,
    /// Wall-clock time access.
    Time,
    /// Network access (TCP, DNS).
    Net,
    /// Random number generation.
    Random,
    /// Concurrency primitives (sleep, fork, join).
    Concurrent,
    /// Unsafe FFI operations.
    Unsafe,
}

impl Capability {
    /// Human-readable name for error messages.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Console => "Console",
            Self::FileSystem => "FileSystem",
            Self::Time => "Time",
            Self::Net => "Net",
            Self::Random => "Random",
            Self::Concurrent => "Concurrent",
            Self::Unsafe => "Unsafe",
        }
    }

    /// CLI flag name (without the `--allow-` prefix).
    pub fn flag_name(&self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::FileSystem => "fs",
            Self::Time => "time",
            Self::Net => "net",
            Self::Random => "random",
            Self::Concurrent => "concurrent",
            Self::Unsafe => "unsafe",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A sandbox policy that controls which capabilities are available at runtime.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    /// `None` = unrestricted (normal execution).
    /// `Some(set)` = sandbox mode, only these capabilities are allowed.
    allowed: Option<HashSet<Capability>>,
}

impl SandboxPolicy {
    /// Create an unrestricted policy (normal execution, no sandbox).
    pub fn unrestricted() -> Self {
        Self { allowed: None }
    }

    /// Create a sandbox policy with the given set of allowed capabilities.
    pub fn sandbox(allowed: HashSet<Capability>) -> Self {
        Self {
            allowed: Some(allowed),
        }
    }

    /// Check whether the given capability is allowed.
    pub fn check(&self, cap: Capability) -> Result<(), String> {
        match &self.allowed {
            None => Ok(()),
            Some(allowed) => {
                if allowed.contains(&cap) {
                    Ok(())
                } else {
                    Err(format!(
                        "sandbox: `{}` capability is not allowed — use --allow-{} to grant it",
                        cap.name(),
                        cap.flag_name()
                    ))
                }
            }
        }
    }

    /// Return whether sandbox mode is active.
    pub fn is_sandboxed(&self) -> bool {
        self.allowed.is_some()
    }
}

thread_local! {
    static SANDBOX_POLICY: RefCell<SandboxPolicy> = RefCell::new(SandboxPolicy::unrestricted());
}

/// Install a sandbox policy for the current thread.
pub fn set_policy(policy: SandboxPolicy) {
    SANDBOX_POLICY.with(|p| *p.borrow_mut() = policy);
}

/// Check whether a capability is allowed by the current sandbox policy.
///
/// Returns `Ok(())` if allowed, or an error string if denied.
pub fn check(cap: Capability) -> Result<(), String> {
    SANDBOX_POLICY.with(|p| p.borrow().check(cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrestricted_allows_everything() {
        let policy = SandboxPolicy::unrestricted();
        assert!(policy.check(Capability::Console).is_ok());
        assert!(policy.check(Capability::FileSystem).is_ok());
        assert!(policy.check(Capability::Net).is_ok());
        assert!(!policy.is_sandboxed());
    }

    #[test]
    fn sandbox_denies_ungranted_capabilities() {
        let policy = SandboxPolicy::sandbox(HashSet::new());
        let err = policy.check(Capability::Console).unwrap_err();
        assert!(err.contains("Console"));
        assert!(err.contains("--allow-console"));
        assert!(policy.is_sandboxed());
    }

    #[test]
    fn sandbox_allows_granted_capabilities() {
        let mut caps = HashSet::new();
        caps.insert(Capability::Console);
        caps.insert(Capability::Time);
        let policy = SandboxPolicy::sandbox(caps);
        assert!(policy.check(Capability::Console).is_ok());
        assert!(policy.check(Capability::Time).is_ok());
        assert!(policy.check(Capability::FileSystem).is_err());
    }

    #[test]
    fn thread_local_policy_works() {
        // Save and restore
        set_policy(SandboxPolicy::sandbox(HashSet::new()));
        assert!(check(Capability::Console).is_err());

        set_policy(SandboxPolicy::unrestricted());
        assert!(check(Capability::Console).is_ok());
    }
}
