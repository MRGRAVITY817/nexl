//! Scope tokens and scope sets for hygienic macro expansion.
//!
//! Every syntactic region (module, `let` binding, macro expansion) introduces a
//! unique [`Scope`] token. Each identifier carries a [`ScopeSet`] — the set of
//! scopes active at its binding or reference site. Hygiene is enforced by
//! comparing scope sets during name resolution (Flatt, 2016).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique token identifying a syntactic region.
///
/// Created via [`Scope::fresh()`]. Two scopes are equal iff they were produced
/// by the same `fresh()` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope(u64);

/// Global counter for generating unique scope tokens.
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(0);

impl Scope {
    /// Generate a fresh, globally unique scope token.
    pub fn fresh() -> Self {
        Self(NEXT_SCOPE.fetch_add(1, Ordering::Relaxed))
    }
}

/// An ordered set of [`Scope`] tokens carried by an identifier.
///
/// A binding at scope set *S* captures a reference whose scope set *R* when
/// *R* ⊇ *S* (the reference's scopes are a superset of the binding's scopes).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeSet(BTreeSet<Scope>);

impl ScopeSet {
    /// Create an empty scope set.
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Insert a scope into the set.
    pub fn add(&mut self, scope: Scope) {
        self.0.insert(scope);
    }

    /// Remove a scope from the set. No-op if the scope is not present.
    pub fn remove(&mut self, scope: Scope) {
        self.0.remove(&scope);
    }

    /// Toggle a scope: add it if absent, remove it if present.
    ///
    /// This is the core operation for macro hygiene (spec §7.6 step 3).
    /// After a macro transformer runs, the expander *flips* the introduction
    /// scope in the result — identifiers the macro introduced keep it,
    /// identifiers from user code lose it.
    pub fn flip(&mut self, scope: Scope) {
        if self.0.contains(&scope) {
            self.0.remove(&scope);
        } else {
            self.0.insert(scope);
        }
    }

    /// Returns `true` if the set contains the given scope.
    pub fn contains(&self, scope: Scope) -> bool {
        self.0.contains(&scope)
    }

    /// Returns `true` if the set has no scopes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns `true` if `self` is a superset of `other` (i.e. `self ⊇ other`).
    ///
    /// Used for binding resolution: a binding at scope set *S* captures a
    /// reference at scope set *R* when *R* ⊇ *S*.
    pub fn is_superset(&self, other: &ScopeSet) -> bool {
        self.0.is_superset(&other.0)
    }
}

impl Default for ScopeSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_set_empty() {
        let ss = ScopeSet::new();
        let s = Scope::fresh();
        assert!(!ss.contains(s));
        assert!(ss.is_empty());
    }

    #[test]
    fn test_scope_set_add() {
        let mut ss = ScopeSet::new();
        let s = Scope::fresh();
        ss.add(s);
        assert!(ss.contains(s));
        assert!(!ss.is_empty());
    }

    #[test]
    fn test_scope_set_remove() {
        let mut ss = ScopeSet::new();
        let s = Scope::fresh();
        ss.add(s);
        assert!(ss.contains(s));
        ss.remove(s);
        assert!(!ss.contains(s));
        assert!(ss.is_empty());
    }

    #[test]
    fn test_scope_set_remove_absent_is_noop() {
        let mut ss = ScopeSet::new();
        let s1 = Scope::fresh();
        let s2 = Scope::fresh();
        ss.add(s1);
        ss.remove(s2); // s2 was never added
        assert!(ss.contains(s1));
        assert!(!ss.contains(s2));
    }

    #[test]
    fn test_scope_set_flip_adds_when_absent() {
        let mut ss = ScopeSet::new();
        let s = Scope::fresh();
        assert!(!ss.contains(s));
        ss.flip(s);
        assert!(ss.contains(s));
    }

    #[test]
    fn test_scope_set_flip_removes_when_present() {
        let mut ss = ScopeSet::new();
        let s = Scope::fresh();
        ss.add(s);
        assert!(ss.contains(s));
        ss.flip(s);
        assert!(!ss.contains(s));
    }

    #[test]
    fn test_scope_set_is_superset() {
        let s1 = Scope::fresh();
        let s2 = Scope::fresh();
        let s3 = Scope::fresh();

        let mut binding = ScopeSet::new();
        binding.add(s1);
        binding.add(s2);

        // Reference has s1, s2, s3 — superset of binding {s1, s2}
        let mut reference = ScopeSet::new();
        reference.add(s1);
        reference.add(s2);
        reference.add(s3);
        assert!(reference.is_superset(&binding));

        // Binding has s1, s2 — NOT superset of reference {s1, s2, s3}
        assert!(!binding.is_superset(&reference));

        // Equal sets are supersets of each other
        let mut equal = ScopeSet::new();
        equal.add(s1);
        equal.add(s2);
        assert!(binding.is_superset(&equal));
        assert!(equal.is_superset(&binding));

        // Empty set is superset of empty set
        let empty = ScopeSet::new();
        assert!(empty.is_superset(&ScopeSet::new()));

        // Any non-empty set is superset of the empty set
        assert!(binding.is_superset(&ScopeSet::new()));
    }

    #[test]
    fn test_scope_fresh_unique() {
        let s1 = Scope::fresh();
        let s2 = Scope::fresh();
        let s3 = Scope::fresh();
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
        assert_ne!(s1, s3);
    }
}
