//! Emits the conformance results matrix as markdown to stdout.
//!
//! Run with:
//!
//! ```text
//! cargo run --example gen_matrix > results-matrix.md
//! ```
//!
//! The `headless (vt100)` column is computed live from the corpus. The remaining
//! columns (one per real terminal the author tests under betamax) are emitted
//! blank for the author to fill in tape-by-tape. Adjust `REAL_TERMINALS` to the
//! author's local set (the plan suggests Ghostty, kitty, Terminal.app — confirm
//! with the author).

use conformance::corpus::corpus;
use conformance::matrix::render_markdown;

/// The real terminals the author fills in later via betamax. Placeholder set from
/// the Arc 5 plan; the author confirms the actual local set.
const REAL_TERMINALS: &[&str] = &["Ghostty", "kitty", "Terminal.app"];

fn main() {
    let cases = corpus();
    print!("{}", render_markdown(&cases, REAL_TERMINALS));
}
