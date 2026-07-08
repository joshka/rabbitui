//! Runs the whole conformance corpus headlessly under `cargo test`.
//!
//! This is the green gate: every corpus case must resolve to a pass or an explicit
//! visual-only skip. A hard [`Outcome::Fail`] fails the suite; a
//! [`Outcome::Skipped`] is surfaced (printed with its reason) but does not fail —
//! that is the whole point of visual-only cases, which cannot be judged headlessly
//! and must not be silently passed either.

use conformance::corpus::{Expectation, corpus};
use conformance::runner::{Outcome, run_case};

#[test]
fn every_case_passes_or_is_an_explicit_visual_skip() {
    let cases = corpus();
    assert!(!cases.is_empty(), "corpus must not be empty");

    let mut failures = Vec::new();
    let mut skips = Vec::new();

    for case in &cases {
        match run_case(case) {
            Outcome::Pass => {}
            Outcome::Skipped(reason) => skips.push((case.id, reason)),
            Outcome::Fail(msg) => failures.push((case.id, msg)),
        }
    }

    // Surface the visual-only skips loudly so they are never mistaken for passes.
    for (id, reason) in &skips {
        println!("SKIP (visual-only) {id}: {reason}");
    }

    assert!(
        failures.is_empty(),
        "corpus cases failed headlessly:\n{}",
        failures
            .iter()
            .map(|(id, msg)| format!("  {id}: {msg}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Every corpus id from the spec's §10.1 namespace must be present exactly once.
#[test]
fn corpus_covers_every_spec_identifier() {
    let cases = corpus();
    let ids: Vec<&str> = cases.iter().map(|c| c.id).collect();

    // The full identifier set the spec §10.1 fixes (the region-behavior table plus
    // the two byte/erase-layer identifiers).
    let expected = [
        "INLINE-APPEND-ONCE",
        "INLINE-COMMIT-ORDER",
        "INLINE-WRAP-ON-RESIZE",
        "INLINE-COMMIT-STYLED",
        "INLINE-TAIL-BOUNDED",
        "INLINE-TAIL-SHRINK",
        "INLINE-TAIL-CELL-DIFF",
        "INLINE-NOOP-SILENT",
        "INLINE-RELATIVE-CURSOR",
        "MODE2026-FRAMING",
        "INLINE-ALTSCREEN-FLUSH",
        "BCE-RESET",
        "WIDE-GRAPHEME-CONTINUATION",
    ];

    for id in expected {
        assert!(ids.contains(&id), "corpus missing spec identifier {id}");
    }
    assert_eq!(
        ids.len(),
        expected.len(),
        "corpus has an unexpected number of cases: {ids:?}"
    );
    // No duplicate ids.
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "duplicate corpus id present");
}

/// Visual-only cases must carry a non-empty reason (they must not be silent).
#[test]
fn visual_only_cases_carry_a_reason() {
    for case in corpus() {
        if let Expectation::VisualOnly { reason } = &case.expect {
            assert!(
                !reason.trim().is_empty(),
                "visual-only case {} must explain why it is not headless-decidable",
                case.id
            );
            assert_eq!(
                run_case(&case),
                Outcome::Skipped(reason.to_string()),
                "{} must resolve to a skip carrying its reason",
                case.id
            );
        }
    }
}
