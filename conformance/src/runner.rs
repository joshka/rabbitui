//! The headless runner: execute a [`Case`] against a [`vt100`] model and decide.
//!
//! For each case the runner feeds [`Case::input`] to a fresh [`Model`], then
//! interprets the case's [`Expectation`] against the emulated screen and/or the
//! byte stream. Every case resolves to exactly one [`Outcome`]:
//!
//! - [`Outcome::Pass`] — the observable behavior matched.
//! - [`Outcome::Fail`] — it did not; the message says how.
//! - [`Outcome::Skipped`] — the behavior is [`Expectation::VisualOnly`] and can
//!   only be judged on real hardware under betamax. **A visual-only case is never
//!   silently passed** — it is surfaced as an explicit skip carrying its reason,
//!   so it shows as `na` (not `pass`) in the headless matrix column.

use crate::corpus::{Case, Expectation};
use crate::escape;
use crate::model::Model;

/// The result of running one corpus case headlessly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The observable behavior matched the expectation.
    Pass,
    /// The behavior did not match; the string explains how.
    Fail(String),
    /// The case is visual-only and cannot be decided headlessly; the string is the
    /// reason it must be judged under betamax on real hardware instead.
    Skipped(String),
}

impl Outcome {
    /// The single-word matrix status for this outcome.
    #[must_use]
    pub const fn matrix_status(&self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail(_) => "fail",
            Outcome::Skipped(_) => "na",
        }
    }

    /// Whether this outcome is a hard failure (fails the test suite).
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        matches!(self, Outcome::Fail(_))
    }
}

/// Runs one case and returns its [`Outcome`].
#[must_use]
pub fn run_case(case: &Case) -> Outcome {
    let (rows, cols) = case.screen;
    let mut model = Model::new(rows, cols);
    model.feed(&case.input);
    check(case, &mut model)
}

/// Runs the whole corpus, returning `(id, outcome)` in corpus order.
#[must_use]
pub fn run_all(corpus: &[Case]) -> Vec<(&'static str, Outcome)> {
    corpus
        .iter()
        .map(|case| (case.id, run_case(case)))
        .collect()
}

fn check(case: &Case, model: &mut Model) -> Outcome {
    // The byte-discipline sub-checks tied to specific ids are applied first so a
    // case that also has a screen expectation still gets its byte guarantee.
    if let Some(outcome) = id_specific_byte_check(case) {
        if outcome.is_failure() {
            return outcome;
        }
    }

    match &case.expect {
        Expectation::VisualOnly { reason } => Outcome::Skipped((*reason).to_string()),

        Expectation::VisibleRows(expected) => {
            let actual = model.visible_rows();
            for (i, want) in expected.iter().enumerate() {
                let got = actual.get(i).map(String::as_str).unwrap_or("");
                if got != *want {
                    return Outcome::Fail(format!(
                        "visible row {i}: expected {want:?}, got {got:?}\nfull screen: {actual:?}"
                    ));
                }
            }
            Outcome::Pass
        }

        Expectation::LinesInOrder(ordered) => {
            let lines = model.all_lines();
            let mut search_from = 0usize;
            for want in *ordered {
                match lines
                    .iter()
                    .skip(search_from)
                    .position(|l| l == want)
                    .map(|p| p + search_from)
                {
                    Some(at) => search_from = at + 1,
                    None => {
                        return Outcome::Fail(format!(
                            "expected {want:?} at or after position {search_from} in transcript, \
                             not found (or out of order): {lines:?}"
                        ));
                    }
                }
            }
            Outcome::Pass
        }

        Expectation::LogicalLineUnwrapped { logical, tail } => {
            // Byte channel: the logical line is emitted contiguously (the renderer
            // inserted no wrap of its own).
            if escape::find(&case.input, logical.as_bytes()).is_none() {
                return Outcome::Fail(format!(
                    "logical line {logical:?} is not emitted contiguously — the renderer \
                     appears to have pre-wrapped it (§3.1 violation)"
                ));
            }
            // Screen channel: the emulator's own soft-wrap reflows to the logical
            // line when the wrapped rows are rejoined. Concatenating the
            // transcript's non-tail rows (trailing space already trimmed) must
            // reproduce the logical line, and the tail must still be present.
            let lines = model.all_lines();
            if !lines.iter().any(|l| l == tail) {
                return Outcome::Fail(format!("tail {tail:?} missing from transcript: {lines:?}"));
            }
            let rejoined: String = lines
                .iter()
                .filter(|l| l.as_str() != *tail)
                .cloned()
                .collect();
            if rejoined != *logical {
                return Outcome::Fail(format!(
                    "wrapped rows do not rejoin to the logical line: expected {logical:?}, \
                     got {rejoined:?} from {lines:?}"
                ));
            }
            Outcome::Pass
        }

        Expectation::BlankFromRow(first_blank) => {
            let (rows, _) = model.size();
            for y in *first_blank..rows {
                let text = model.row_text(y);
                if !text.is_empty() {
                    return Outcome::Fail(format!(
                        "row {y} should be blank (orphan row after shrink), got {text:?}"
                    ));
                }
            }
            Outcome::Pass
        }

        Expectation::SyncFramed => {
            let bytes = &case.input;
            if bytes.starts_with(escape::SYNC_BEGIN) && bytes.ends_with(escape::SYNC_END) {
                Outcome::Pass
            } else {
                Outcome::Fail(
                    "frame is not bracketed by mode-2026 sync begin/end (CSI ?2026h … CSI ?2026l)"
                        .to_string(),
                )
            }
        }

        Expectation::BytesOrdered { needle, before } => {
            let n = escape::find(&case.input, needle);
            let b = escape::find(&case.input, before);
            match (n, b) {
                (Some(np), Some(bp)) if np < bp => Outcome::Pass,
                (Some(np), Some(bp)) => Outcome::Fail(format!(
                    "expected {needle:?} (at {np}) to precede {before:?} (at {bp})"
                )),
                (None, _) => Outcome::Fail(format!("{needle:?} not found in stream")),
                (_, None) => Outcome::Fail(format!("{before:?} not found in stream")),
            }
        }

        Expectation::EraseResetGuarded => erase_reset_guarded(&case.input),

        Expectation::EmptyOutput => {
            if case.input.is_empty() {
                Outcome::Pass
            } else {
                Outcome::Fail(format!(
                    "expected an empty (silent) frame, got {} byte(s)",
                    case.input.len()
                ))
            }
        }

        Expectation::CursorAt { row, col } => {
            let (r, c) = model.cursor();
            if r == *row && c == *col {
                Outcome::Pass
            } else {
                Outcome::Fail(format!("cursor expected at ({row},{col}), got ({r},{c})"))
            }
        }

        Expectation::CellAnsiFg {
            row,
            col,
            symbol,
            ansi,
        } => match model.cell(*row, *col) {
            Some((sym, fg)) if sym == *symbol && fg == Some(*ansi) => Outcome::Pass,
            Some((sym, fg)) => Outcome::Fail(format!(
                "cell ({row},{col}): expected {symbol:?} ansi-fg {ansi}, got {sym:?} ansi-fg {fg:?}"
            )),
            None => Outcome::Fail(format!("cell ({row},{col}) is off-screen")),
        },
    }
}

/// The `BCE-RESET` invariant: every erase-to-end-of-display in the stream is
/// *immediately* preceded by an SGR reset (`CSI 0 m`). Scans all erase spellings.
fn erase_reset_guarded(bytes: &[u8]) -> Outcome {
    for erase in [escape::ERASE_TO_END, escape::ERASE_TO_END_SHORT] {
        let mut from = 0usize;
        while let Some(rel) = escape::find(&bytes[from..], erase) {
            let at = from + rel;
            let reset = escape::SGR_RESET;
            let guarded = at >= reset.len() && &bytes[at - reset.len()..at] == reset;
            if !guarded {
                return Outcome::Fail(format!(
                    "erase {erase:?} at byte {at} is not immediately preceded by an SGR reset \
                     (CSI 0 m) — BCE bleed risk"
                ));
            }
            from = at + erase.len();
        }
    }
    Outcome::Pass
}

/// Byte-discipline checks bound to specific corpus ids where the fixture's screen
/// outcome alone would not prove the invariant. Returns `None` for ids without an
/// extra byte guarantee, `Some(Fail)` if the guarantee is violated, `Some(Pass)`
/// if it holds (the caller then still runs the primary screen expectation).
fn id_specific_byte_check(case: &Case) -> Option<Outcome> {
    match case.id {
        // INLINE-TAIL-CELL-DIFF: the diff frame must not erase-below and must not
        // repaint the unchanged row's text.
        "INLINE-TAIL-CELL-DIFF" => {
            let text = String::from_utf8_lossy(&case.input);
            if text.contains("\x1b[0J") || text.contains("\x1b[J") {
                Some(Outcome::Fail(
                    "a stable-tail cell diff must not erase-below (found CSI J)".to_string(),
                ))
            } else {
                Some(Outcome::Pass)
            }
        }

        // INLINE-APPEND-ONCE: the committed line must appear exactly once in the
        // byte stream (a repaint that re-committed would emit it twice).
        "INLINE-APPEND-ONCE" => {
            let count = count_occurrences(&case.input, b"log line 1");
            if count == 1 {
                Some(Outcome::Pass)
            } else {
                Some(Outcome::Fail(format!(
                    "committed line must be emitted exactly once, emitted {count} times"
                )))
            }
        }

        _ => None,
    }
}

/// How many (non-overlapping) times `needle` occurs in `haystack`.
fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut from = 0usize;
    while let Some(rel) = escape::find(&haystack[from..], needle) {
        count += 1;
        from += rel + needle.len();
    }
    count
}
