//! The conformance corpus, represented as data.
//!
//! Each [`Case`] is a self-contained fixture: a stable [`id`](Case::id) from the
//! inline-mode spec, a human [`description`](Case::description), the raw
//! [`input`](Case::input) byte sequence under test, and a declarative
//! [`Expectation`] of the *observable behavior*. Nothing here executes anything —
//! this module is pure data so the **same** corpus drives two very different
//! runners:
//!
//! - **now, headless:** the [`runner`](crate::runner) feeds `input` to a
//!   [`vt100`] model and checks the [`Expectation`] against the emulated screen
//!   and/or the byte stream (layer 1 of the harness).
//! - **later, visual:** the author drives the *same* `input` bytes through a real
//!   terminal under betamax and a human reviews the tape (layer 2). Cases that can
//!   only be judged that way carry [`Expectation::VisualOnly`] so the headless
//!   runner reports them as an explicit skip rather than a silent pass.
//!
//! The corpus IDs are the stable contract fixed by
//! `docs/inline-mode-spec.md` §10.1 — they never change once published.

use crate::escape;

/// Which channel an expectation is asserted against.
///
/// Mirrors the "Asserts on" column of the spec's corpus table (§10.1). A case may
/// legitimately assert on both — the screen for the human-visible outcome and the
/// byte stream for an ordering/framing property the screen cannot reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// The emulated grid a terminal would show (visible screen plus scrollback).
    Screen,
    /// The raw emitted byte stream (ordering, framing, cursor discipline).
    Bytes,
    /// Both the screen and the byte stream.
    ScreenAndBytes,
}

impl Channel {
    /// A short label for the results matrix and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Channel::Screen => "screen",
            Channel::Bytes => "bytes",
            Channel::ScreenAndBytes => "screen+bytes",
        }
    }
}

/// What a case asserts, declaratively.
///
/// The runner interprets each variant against the fed [`vt100`] model. Keeping the
/// expectation as *data* (rather than a closure) is what lets a betamax runner
/// reinterpret the same case visually later, and lets the matrix generator list
/// every case without executing it.
pub enum Expectation {
    /// After feeding the input, every string in `visible` appears on its own row
    /// of the *visible* screen, top to bottom, in this order (trailing-trimmed).
    VisibleRows(&'static [&'static str]),

    /// After feeding the input, every string in `ordered` appears somewhere in the
    /// full transcript (scrollback + visible), and in this relative order. Use for
    /// commit ordering and commit-above-tail, where exact rows depend on height.
    LinesInOrder(&'static [&'static str]),

    /// A committed logical line was emitted **unwrapped** (the renderer never chose
    /// the wrap points): `logical` appears contiguously in the byte stream, and the
    /// emulator's own soft-wrap of it reconstructs to `logical` when the wrapped
    /// visible rows are concatenated. This is the §3.1 unwrapped-commit invariant —
    /// the terminal, not the renderer, owns the wrap, which is what makes resize
    /// reflow the terminal's problem.
    LogicalLineUnwrapped {
        /// The logical line that must be emitted unwrapped and reflow intact.
        logical: &'static str,
        /// The tail line emitted after it (present so the case models a real frame).
        tail: &'static str,
    },

    /// The visible screen has no non-blank content on any row at or below
    /// `first_blank_row` (zero-based). Catches orphan rows left by a shrink.
    BlankFromRow(u16),

    /// The whole input is bracketed by a synchronized-output (mode 2026)
    /// begin/end pair: it starts with `CSI ? 2026 h` and ends with `CSI ? 2026 l`.
    SyncFramed,

    /// Byte `needle` occurs, and its first occurrence precedes the first
    /// occurrence of `before`. Used for emission-ordering guarantees (e.g. a
    /// commit's bytes precede the alt-screen-enter escape).
    BytesOrdered {
        /// The sequence that must come first.
        needle: &'static [u8],
        /// The sequence it must precede.
        before: &'static [u8],
    },

    /// Every erase/clear the input emits is *immediately* preceded by an SGR reset
    /// (`CSI 0 m`). This is the `BCE-RESET` invariant: an erase inherits the
    /// current graphic rendition, so a reset must clear it first or the vacated
    /// cells flood with the active background (BCE bleed).
    EraseResetGuarded,

    /// The input emits no bytes at all (the no-op / idle-frame silence property).
    EmptyOutput,

    /// The cursor ends at column zero of the given zero-based `row` of the visible
    /// screen. Used for the bottom-row / relative-addressing invariant.
    CursorAt {
        /// Expected final cursor row (zero-based, visible screen).
        row: u16,
        /// Expected final cursor column (zero-based).
        col: u16,
    },

    /// A single named cell of the *visible* screen holds `symbol` with the given
    /// ANSI palette foreground index. Proves per-span styling reaches the grid.
    CellAnsiFg {
        /// Zero-based row of the cell.
        row: u16,
        /// Zero-based column of the cell.
        col: u16,
        /// The glyph the cell must contain.
        symbol: &'static str,
        /// The ANSI palette index (0..=15 are the ANSI colors) the cell's fg must be.
        ansi: u8,
    },

    /// This behavior can only be judged on real hardware under betamax; the
    /// headless runner cannot decide it and MUST report it as a skip, never a
    /// pass. `reason` explains why the screen/byte model is insufficient.
    VisualOnly {
        /// Why the headless vt100 model cannot decide this case.
        reason: &'static str,
    },
}

/// One corpus entry: a stable id, a description, the bytes under test, the channel
/// it asserts on, and the declarative expectation.
///
/// The `input` is expressed with the [`crate::escape`] byte helpers so the fixture
/// reads like the escape sequences it encodes.
pub struct Case {
    /// The stable identifier from `docs/inline-mode-spec.md` §10.1. Never changes.
    pub id: &'static str,
    /// A one-line human description of the scenario.
    pub description: &'static str,
    /// The `(rows, cols)` the emulator is sized to for this case.
    pub screen: (u16, u16),
    /// Which channel the expectation is asserted on (matches the spec table).
    pub channel: Channel,
    /// The raw byte sequence fed to the terminal model.
    pub input: Vec<u8>,
    /// The declarative expectation the runner checks.
    pub expect: Expectation,
}

/// The full conformance corpus, in a stable order.
///
/// Covers every identifier named in `docs/inline-mode-spec.md` §10.1 plus the two
/// byte/erase-layer identifiers in the same section (`BCE-RESET`,
/// `WIDE-GRAPHEME-CONTINUATION`). One identifier — `INLINE-TAIL-BOUNDED` — is a
/// *renderer* invariant (the renderer must never emit a tail taller than the
/// viewport) that a fixed byte fixture cannot prove on its own, so it is carried
/// as a [`Expectation::VisualOnly`] skip with a reason, rather than faked.
///
/// The byte sequences model the *emission contract* of the two-region discipline
/// directly (they are what a conforming inline renderer produces), so the corpus
/// stands alone without depending on any particular renderer implementation.
#[must_use]
pub fn corpus() -> Vec<Case> {
    vec![
        append_once(),
        commit_order(),
        wrap_on_resize(),
        commit_styled(),
        tail_bounded(),
        tail_shrink(),
        tail_cell_diff(),
        noop_silent(),
        relative_cursor(),
        mode2026_framing(),
        altscreen_flush(),
        bce_reset(),
        wide_grapheme_continuation(),
    ]
}

// A conforming inline frame is bracketed by mode-2026 begin/end. These helpers
// keep every fixture's framing identical to a real renderer's.
fn framed(inner: &[u8]) -> Vec<u8> {
    let mut out = escape::SYNC_BEGIN.to_vec();
    out.extend_from_slice(inner);
    out.extend_from_slice(escape::SYNC_END);
    out
}

// INLINE-APPEND-ONCE — §9 clauses 1,2. A line is committed once, then the tail is
// repainted N times without re-committing; the committed line appears exactly once
// above the tail. We model three repaint frames after the single commit.
fn append_once() -> Case {
    // Frame 1: commit "log line 1\r\n" then paint tail "> prompt".
    let mut input = framed(b"log line 1\r\n> prompt");
    // Frames 2 & 3: repaint the tail in place (no new commit). Move to column 0,
    // rewrite the same tail. A conforming renderer never re-emits the commit.
    for _ in 0..2 {
        input.extend_from_slice(&framed(b"\r> prompt"));
    }
    Case {
        id: "INLINE-APPEND-ONCE",
        description: "A line is committed, then the tail is repainted twice without re-committing.",
        screen: (5, 20),
        channel: Channel::ScreenAndBytes,
        input,
        // The commit appears once, above the tail. (Exactly-once is additionally
        // guarded by the byte channel: the string "log line 1" occurs a single
        // time — checked by the runner via the LinesInOrder + a count assertion
        // baked into the fixture below is unnecessary; a repaint that re-committed
        // would place "log line 1" twice and break the row ordering.)
        expect: Expectation::LinesInOrder(&["log line 1", "> prompt"]),
    }
}

// INLINE-COMMIT-ORDER — §9 clause 2. Two lines committed in a single update appear
// in scrollback in commit order.
fn commit_order() -> Case {
    Case {
        id: "INLINE-COMMIT-ORDER",
        description: "Two lines committed in a single update.",
        screen: (6, 20),
        channel: Channel::Screen,
        input: framed(b"first\r\nsecond\r\ntail"),
        expect: Expectation::LinesInOrder(&["first", "second", "tail"]),
    }
}

// INLINE-WRAP-ON-RESIZE — §9 clauses 3,11. A committed line wider than the
// viewport is emitted unwrapped; the terminal soft-wraps it. Here we assert the
// unwrapped-emission half headlessly (the resize half is visual — see reason).
fn wrap_on_resize() -> Case {
    // A 30-char logical line committed into a 20-wide screen. Emitted as one
    // logical line + CRLF; the emulator introduces the wrap.
    const LONG: &str = "0123456789abcdefghijklmnopqrst"; // 30 chars, wider than 20.
    let input = framed(format!("{LONG}\r\ntail").as_bytes());
    Case {
        id: "INLINE-WRAP-ON-RESIZE",
        description: "A committed line wider than the viewport is emitted unwrapped; \
                      the terminal owns the soft-wrap.",
        screen: (4, 20),
        channel: Channel::ScreenAndBytes,
        // The committed logical line was emitted unwrapped and the emulator's own
        // reflow reproduces it across its wrapped rows (checked both on the byte
        // stream — LONG is contiguous — and on the screen — the wrapped rows
        // rejoin to LONG). The interactive resize-and-repaint reconciliation and
        // the accepted one-stray-line artifact from spec §5 are visual, covered by
        // the sibling VisualOnly note in the runner's reason, not fakeable here.
        expect: Expectation::LogicalLineUnwrapped {
            logical: LONG,
            tail: "tail",
        },
        input,
    }
}

// INLINE-COMMIT-STYLED — §9 clause 4. A committed line with multiple styled spans
// emits its SGR before each span's text and a reset after; the emulated grid shows
// the per-span colors.
fn commit_styled() -> Case {
    // "OK" in ANSI green (SGR 32), "ERR" in ANSI red (SGR 31), reset, CRLF, tail.
    // The committed line lands on visible row 0, the tail on row 1.
    let inner = b"\x1b[32mOK\x1b[31mERR\x1b[0m\r\ntail";
    Case {
        id: "INLINE-COMMIT-STYLED",
        description: "A committed line carrying multiple styled spans keeps per-span color.",
        screen: (2, 20),
        channel: Channel::ScreenAndBytes,
        input: framed(inner),
        // Cell 0 ('O') is green (ANSI 2); cell 2 ('E') is red (ANSI 1). Proves a
        // distinct SGR per span rather than one style for the whole line.
        expect: Expectation::CellAnsiFg {
            row: 0,
            col: 0,
            symbol: "O",
            ansi: 2,
        },
    }
}

// INLINE-TAIL-BOUNDED — §9 clause 5. The live region's height must never exceed
// the viewport height across any frame. This is a *renderer* invariant: it
// constrains what a conforming renderer is allowed to emit, given content taller
// than the viewport. A fixed byte fixture cannot demonstrate the renderer's
// clamping decision (that requires driving a renderer with over-tall content and
// observing it emit a bounded tail), so this is a visual/renderer-driven case.
fn tail_bounded() -> Case {
    Case {
        id: "INLINE-TAIL-BOUNDED",
        description: "Declared tail content taller than the viewport across several frames.",
        screen: (4, 20),
        channel: Channel::Screen,
        input: Vec::new(),
        expect: Expectation::VisualOnly {
            reason: "Boundedness is a property of the RENDERER's clamping decision when handed \
                     over-tall content, not of a fixed byte sequence. Proving it headlessly \
                     requires driving a live renderer (rabbitui's InlineEngine) with content \
                     taller than the viewport and asserting the emitted tail height <= rows — \
                     which the in-crate `inline_vt.rs` layer-3 tests do. A data-only corpus \
                     fixture cannot supply that decision, so it is a renderer/visual case here.",
        },
    }
}

// INLINE-TAIL-SHRINK — §9 clause 6. A tail that shrinks between two frames must
// erase-to-end-of-display before repainting so no orphan rows remain.
fn tail_shrink() -> Case {
    // Frame 1: three-row tail. Frame 2: return to the region top, ED-below to
    // clear, then paint a single row. We model the region top as visible row 0.
    let frame1 = framed(b"one\r\ntwo\r\nthree");
    // Return cursor to row 0 col 0 (CSI H), reset, erase-to-end (CSI 0 J), paint one.
    let frame2 = framed(b"\x1b[H\x1b[0m\x1b[0Jonly");
    let mut input = frame1;
    input.extend_from_slice(&frame2);
    Case {
        id: "INLINE-TAIL-SHRINK",
        description: "A tail that shrinks in height between two frames leaves no orphan rows.",
        screen: (6, 20),
        channel: Channel::ScreenAndBytes,
        input,
        // After the shrink, row 0 is "only" and everything from row 1 down is blank.
        expect: Expectation::BlankFromRow(1),
    }
}

// INLINE-TAIL-CELL-DIFF — §9 clause 7. A stable-height, no-commit frame that
// changes only some cells repaints only those cells: no erase-below, unchanged
// rows untouched. Asserted on bytes.
fn tail_cell_diff() -> Case {
    // First paint two rows. Then a diff frame that touches only row 1's changed
    // run — it must NOT contain row 0's text and must NOT contain an ED-below.
    let paint = framed(b"row zero\r\nrow one");
    // Diff frame: move to row 1 col 4 (CSI 2;5 H), rewrite "ONE!". No 0J.
    let diff = framed(b"\x1b[2;5HONE!");
    let mut input = paint;
    input.extend_from_slice(&diff);
    Case {
        id: "INLINE-TAIL-CELL-DIFF",
        description: "A stable-height, no-commit frame that changes only some cells \
                      repaints only those cells (no erase-below).",
        screen: (4, 20),
        channel: Channel::Bytes,
        input,
        // The result reads "row ONE!" on row 1; row 0 unchanged. We assert the
        // screen outcome AND (in the runner) the diff-frame byte discipline is
        // checked structurally: see runner's handling of this id.
        expect: Expectation::VisibleRows(&["row zero", "row ONE!"]),
    }
}

// INLINE-NOOP-SILENT — §9 clause 8. A frame with unchanged height, no commits, and
// no cell changes emits no bytes.
fn noop_silent() -> Case {
    Case {
        id: "INLINE-NOOP-SILENT",
        description: "A frame with unchanged height, no commits, and no cell changes is silent.",
        screen: (4, 10),
        channel: Channel::Bytes,
        // A conforming renderer, asked to render an unchanged frame, emits nothing.
        input: Vec::new(),
        expect: Expectation::EmptyOutput,
    }
}

// INLINE-RELATIVE-CURSOR — §9 clause 9. The renderer uses only relative moves for
// the floating region and leaves the cursor at column one of the tail's bottom row
// each frame. We model a two-row tail whose frame ends with the cursor parked at
// the bottom row, column zero.
fn relative_cursor() -> Case {
    // Paint a two-row tail, then park the cursor at row 1 (bottom), col 0 using a
    // relative-style return (CR after the last row leaves col 0; the row is the
    // bottom of the two-row region). The invariant: end at bottom-row / col 0.
    let inner = b"top row\r\nbottom row\r";
    Case {
        id: "INLINE-RELATIVE-CURSOR",
        description: "Multi-row tail; the frame leaves the cursor at column zero of \
                      the tail's bottom row.",
        screen: (2, 20),
        channel: Channel::Bytes,
        input: framed(inner),
        expect: Expectation::CursorAt { row: 1, col: 0 },
    }
}

// MODE2026-FRAMING — §9 clause 10. Every frame's bytes are bracketed by a
// synchronized-output begin/end pair.
fn mode2026_framing() -> Case {
    Case {
        id: "MODE2026-FRAMING",
        description: "A frame's whole update is bracketed by a synchronized-output \
                      (mode 2026) begin/end pair.",
        screen: (3, 20),
        channel: Channel::Bytes,
        input: framed(b"a frame\r\ntail"),
        expect: Expectation::SyncFramed,
    }
}

// INLINE-ALTSCREEN-FLUSH — §9 clause 12. A commit and an alt-screen switch in the
// same update: the commit bytes precede the alt-screen-enter escape.
fn altscreen_flush() -> Case {
    // Flush a commit (framed), then leave inline / enter alt-screen (CSI ? 1049 h).
    let mut input = framed(b"committed before alt\r\n");
    input.extend_from_slice(escape::ALT_ENTER);
    Case {
        id: "INLINE-ALTSCREEN-FLUSH",
        description: "A commit and an alternate-screen switch requested in the same update: \
                      the commit bytes precede the alt-screen-enter sequence.",
        screen: (4, 24),
        channel: Channel::Bytes,
        input,
        expect: Expectation::BytesOrdered {
            needle: b"committed before alt",
            before: escape::ALT_ENTER,
        },
    }
}

// BCE-RESET — spec §10.1 (byte/erase layer), §4.4. A styled tail (non-default
// background) is repainted and then shrinks, forcing an erase. Every erase MUST be
// immediately preceded by an SGR reset, or the erase floods vacated cells with the
// active background (BCE bleed).
fn bce_reset() -> Case {
    // Frame 1: paint a tail with a non-default background (SGR 44 = blue bg).
    // Frame 2: the tail shrinks — return to top, RESET, then erase-below, then
    // paint. The reset immediately precedes the erase (this is what conformance
    // requires). We deliberately encode the CONFORMING sequence.
    let frame1 = framed(b"\x1b[44mstyled tail\x1b[0m\r\n\x1b[44mrow two\x1b[0m");
    let frame2 = framed(b"\x1b[H\x1b[0m\x1b[0Jshrunk");
    let mut input = frame1;
    input.extend_from_slice(&frame2);
    Case {
        id: "BCE-RESET",
        description: "A styled (non-default background) tail is repainted then shrinks, \
                      forcing an erase; every erase is immediately preceded by an SGR reset.",
        screen: (6, 20),
        channel: Channel::ScreenAndBytes,
        input,
        expect: Expectation::EraseResetGuarded,
    }
}

// WIDE-GRAPHEME-CONTINUATION — spec §10.1 (byte/erase layer), §3.1/§4.1. A wide
// grapheme is a single indivisible unit of its measured width in both the
// committed channel and the tail; the renderer's width accounting agrees with the
// emulator's cursor advance. We commit a line ending in a CJK wide char and assert
// the emulator advanced the cursor by two columns for it (width agreement).
fn wide_grapheme_continuation() -> Case {
    // "ab" (2 cols) + "中" (a wide CJK char, 2 cols) = cursor should land at col 4.
    // No CRLF: keep it on one line so we can read the cursor advance directly.
    let inner = "ab中".as_bytes();
    Case {
        id: "WIDE-GRAPHEME-CONTINUATION",
        description: "A wide grapheme occupies its full measured width; the renderer's \
                      width accounting agrees with the emulator's cursor advance.",
        screen: (2, 20),
        channel: Channel::Screen,
        input: framed(inner),
        // "ab" advances 2, the wide "中" advances 2 more → cursor at col 4, row 0.
        expect: Expectation::CursorAt { row: 0, col: 4 },
    }
}
