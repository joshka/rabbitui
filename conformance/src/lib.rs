//! Public terminal-conformance harness — headless core (Arc 5 item 1, layer 1).
//!
//! This crate is the **headless layer** of a two-layer conformance harness for the
//! two-region *inline-mode* rendering discipline specified in
//! `docs/inline-mode-spec.md`. The two layers are:
//!
//! 1. **Headless, escape-level (this crate).** A named [`corpus`] of test cases,
//!    each carrying the byte sequence under test and a declarative expectation, is
//!    executed by the [`runner`] against a real [`vt100`] terminal model and
//!    checked on the emulated screen and/or the byte stream. Fast, deterministic,
//!    CI-friendly — this is what `cargo test` runs.
//! 2. **Visual, on real hardware (author-driven, out of scope here).** The *same*
//!    corpus bytes are driven through real terminal emulators under betamax and a
//!    human reviews the tapes. Cases that can only be judged that way are marked
//!    [`corpus::Expectation::VisualOnly`] and the headless runner reports them as
//!    an explicit **skip** (`na`), never a silent pass.
//!
//! The corpus IDs are the stable contract fixed by `docs/inline-mode-spec.md`
//! §10.1 and never change once published. Results are reported as a
//! *(corpus-id × terminal)* [`matrix`]; the headless column is generated now, the
//! real-terminal columns are filled in later by the author.
//!
//! This crate grows from the existing escape-level harness (`rabbitui-testing`'s
//! `VtScreen`, ADR 0009 layer 3) in spirit and depends on the same `vt100` crate,
//! but is kept self-contained so it builds and tests on its own.
//!
//! # Example
//!
//! ```
//! use conformance::{corpus, runner};
//!
//! let cases = corpus::corpus();
//! for (id, outcome) in runner::run_all(&cases) {
//!     // Every case resolves to Pass, Fail, or a Skipped-with-reason (visual-only).
//!     assert!(!outcome.is_failure(), "{id} failed: {outcome:?}");
//! }
//! ```

pub mod corpus;
pub mod escape;
pub mod matrix;
pub mod model;
pub mod runner;
