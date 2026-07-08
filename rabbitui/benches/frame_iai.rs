//! Instruction-count budgets for the frame + diff hot paths (Arc 4 item 4,
//! `docs/plans/arc4-spine.md` §4).
//!
//! # UNVERIFIED LOCALLY — needs the Linux CI runner
//!
//! This target uses **iai-callgrind**, which drives **valgrind/callgrind** to count
//! CPU instructions instead of measuring wall-clock. valgrind does **not** run on
//! macOS, so this bench **cannot be compiled or run in the dev environment it was
//! authored in** — it is verified only by the `perf-budgets` CI job on ubuntu (see
//! `.github/workflows/perf-budgets.yml`). To keep the workspace compiling offline
//! (iai-callgrind is not vendored and there is no network here), the `[[bench]]`
//! entry and the `iai-callgrind` dev-dependency in `rabbitui/Cargo.toml` are left
//! **commented out**; uncomment both to enable the target on a valgrind host.
//!
//! # Why instruction counts, not wall-clock
//!
//! The 2B benchmark verification found wall-clock is load-sensitive (scroll-10k
//! measured 1.13 ms quiet vs 2.4 ms under load), so a wall-clock CI budget is
//! flaky by construction. Instruction count is deterministic across load, which is
//! exactly what a *budget* (a regression gate, not a trend line) needs. criterion
//! stays for local trend work; CI gates on this.
//!
//! # Coverage
//!
//! The three frame benches mirrored from `benches/frame.rs` — declared 1,000 and
//! 10,000 flat widgets, and the measure-twice `frame.scroll` at 10,000 items — plus
//! the full-frame **diff** at 240×70 (the damage-tracking cost, mirrored from
//! `rabbitui-core/benches/core.rs`'s `buffer/full_diff_240x70`). Thresholds are set
//! ~30% above the first committed baseline; see the workflow for how to update a
//! baseline intentionally.

#![allow(missing_docs)]

use std::hint::black_box;

use iai_callgrind::{library_benchmark, library_benchmark_group, main};
use rabbitui::core::buffer::Buffer;
use rabbitui::core::frame::Frame;
use rabbitui::core::geometry::{Position, Rect, Size};
use rabbitui::core::id::key;
use rabbitui::core::store::StateStore;
use rabbitui::core::style::{Color, Style};
use rabbitui::core::widget::{RenderCtx, Widget};

/// The buffer size the frame benches paint into: a realistic large window (same as
/// the criterion frame bench).
const FRAME_SIZE: Size = Size::new(240, 70);

/// A synthetic content cell: a stateless one-row label — the representative "leaf"
/// widget for declared-frame cost.
struct Cell {
    label: &'static str,
}

impl Widget for Cell {
    type State = ();
    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        ctx.set_string(Position::ORIGIN, self.label, Style::new());
    }
}

/// One full declared frame of `count` flat widgets: begin_frame → build a Frame →
/// declare → into_parts → end_frame — the whole declare → facts → paint cycle minus
/// terminal I/O.
fn full_frame_flat(count: usize) {
    let mut store = StateStore::new();
    let mut buffer = Buffer::new(FRAME_SIZE);
    store.begin_frame();
    let parts = {
        let mut frame = Frame::new(&mut buffer, &mut store);
        for i in 0..count {
            let y = u16::try_from(i).unwrap_or(u16::MAX);
            let area = Rect::new(Position::new(0, y), Size::new(FRAME_SIZE.width, 1));
            frame.widget(
                key("cell").index(i),
                area,
                &Cell {
                    label: "synthetic row",
                },
            );
        }
        frame.into_parts()
    };
    black_box(parts);
    store.end_frame();
}

/// One full frame declaring `count` items into a `frame.scroll` — the measure-twice
/// path (measure every item to size content + scrollbar, paint only the visible).
fn full_frame_scroll(count: usize) {
    let mut store = StateStore::new();
    let mut buffer = Buffer::new(FRAME_SIZE);
    store.begin_frame();
    let parts = {
        let mut frame = Frame::new(&mut buffer, &mut store);
        let area = frame.area();
        frame.scroll(key("scroll"), area, |scroll| {
            for i in 0..count {
                scroll.item(
                    key("item").index(i),
                    &Cell {
                        label: "synthetic row",
                    },
                );
            }
        });
        frame.into_parts()
    };
    black_box(parts);
    store.end_frame();
}

/// Two 240×70 buffers that differ in every cell, for the full-frame diff cost.
fn diff_buffers() -> (Buffer, Buffer) {
    let mut previous = Buffer::new(FRAME_SIZE);
    let mut current = Buffer::new(FRAME_SIZE);
    let a = Style::new().fg(Color::GREEN);
    let b = Style::new().fg(Color::RED);
    for y in 0..FRAME_SIZE.height {
        previous.set_string(
            Position::new(0, y),
            &"a".repeat(usize::from(FRAME_SIZE.width)),
            a,
        );
        current.set_string(
            Position::new(0, y),
            &"b".repeat(usize::from(FRAME_SIZE.width)),
            b,
        );
    }
    (previous, current)
}

#[library_benchmark]
fn frame_declared_1000() {
    full_frame_flat(black_box(1_000));
}

#[library_benchmark]
fn frame_declared_10000() {
    full_frame_flat(black_box(10_000));
}

#[library_benchmark]
fn frame_scroll_10000() {
    full_frame_scroll(black_box(10_000));
}

#[library_benchmark]
fn full_diff_240x70() {
    let (previous, current) = diff_buffers();
    let changes = black_box(&current).diff(black_box(&previous));
    black_box(changes);
}

library_benchmark_group!(
    name = frame_budgets;
    benchmarks = frame_declared_1000, frame_declared_10000, frame_scroll_10000, full_diff_240x70
);

main!(library_benchmark_groups = frame_budgets);
