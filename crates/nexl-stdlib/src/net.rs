//! `net` module — networking functions.
//!
//! Stage 0 provides stub implementations. Full HTTP client and TCP support
//! requires async runtime and effect system integration.

use crate::StdlibEntry;

/// Return all `net` module function entries.
///
/// Currently empty — networking requires async effects not yet available in Stage 0.
pub fn entries() -> Vec<StdlibEntry> {
    vec![]
}
