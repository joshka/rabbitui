//! Full declared-frame benchmarks (Arc 2B, `docs/design/arc2b-measurement-scroll.md`):
//! the honest one — declaring, collecting facts for, and painting a synthetic view
//! of 1,000 and 10,000 widgets, measured against ADR 0001's "full re-render is
//! microseconds" claim.
//!
//! Also benches a `frame.scroll` variant at 10,000 items to measure the
//! measure-twice `Fn` cost part 1 flagged (the scroll runs its item closure across
//! a measure pass and a paint pass, since v1 has no measurement caching).
//!
//! Run with `cargo bench -p rabbitui` (or `cargo bench --workspace`). A `#[test]`
//! smoke of each body lives in `tests/frame_bench_smoke.rs`, so `cargo test` proves
//! the bench code runs without timing it.

// `criterion_group!` expands to an undocumented `pub fn`; the workspace lints
// `missing_docs`, which does not apply to a bench harness.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use rabbitui::core::buffer::Buffer;
use rabbitui::core::frame::Frame;
use rabbitui::core::geometry::{Position, Rect, Size};
use rabbitui::core::id::key;
use rabbitui::core::store::StateStore;
use rabbitui::core::style::Style;
use rabbitui::core::widget::{RenderCtx, Widget};
use std::hint::black_box;

/// A synthetic content cell: a stateless, one-row label that paints a short
/// styled string and reports a one-row desired height — a representative "leaf"
/// widget for the declared-frame cost.
struct Cell {
    label: &'static str,
}

impl Widget for Cell {
    type State = ();
    fn render(&self, (): &mut (), ctx: &mut RenderCtx<'_>) {
        ctx.set_string(Position::ORIGIN, self.label, Style::new());
    }
}

/// The buffer size the frame benches paint into: a realistic large window. The
/// declared views overflow it, so most widgets are declared-and-clipped (facts
/// recorded, paint mostly clipped) — the honest full-frame cost.
const FRAME_SIZE: Size = Size::new(240, 70);

/// Declares `count` synthetic cells into `frame`, each in its own one-row area at
/// an increasing y — the flat-view workload. Widgets past the buffer height are
/// declared (their facts recorded) but paint-clipped, exactly as a real overflowing
/// view behaves.
fn declare_flat(frame: &mut Frame<'_>, count: usize) {
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
}

/// Runs one full declared frame of `count` flat widgets: begin_frame → build a
/// Frame → view → into_parts (facts + handlers) → end_frame. This is the whole
/// declare → facts → paint cycle the runtime runs each frame, minus the terminal
/// I/O.
fn full_frame_flat(store: &mut StateStore, buffer: &mut Buffer, count: usize) {
    buffer.reset();
    store.begin_frame();
    let parts = {
        let mut frame = Frame::new(buffer, store);
        declare_flat(&mut frame, count);
        frame.into_parts()
    };
    black_box(parts);
    store.end_frame();
}

/// The flat-view benches at 1,000 and 10,000 widgets.
fn bench_full_frame(c: &mut Criterion) {
    for &count in &[1_000usize, 10_000usize] {
        c.bench_function(&format!("frame/declared_{count}_widgets"), |b| {
            let mut store = StateStore::new();
            let mut buffer = Buffer::new(FRAME_SIZE);
            b.iter(|| full_frame_flat(&mut store, &mut buffer, black_box(count)));
        });
    }
}

/// The scroll variant: a full frame declaring `count` items into a `frame.scroll`.
/// The scroll measures every item (to size content and the scrollbar) then paints
/// only the visible few — so the item closure runs across a measure pass and a
/// paint pass (measure-twice). This isolates that cost at 10,000 items.
fn full_frame_scroll(store: &mut StateStore, buffer: &mut Buffer, count: usize) {
    buffer.reset();
    store.begin_frame();
    let parts = {
        let mut frame = Frame::new(buffer, store);
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

/// The scroll bench at 10,000 items — the measure-twice cost.
fn bench_scroll_frame(c: &mut Criterion) {
    c.bench_function("frame/scroll_10000_items", |b| {
        let mut store = StateStore::new();
        let mut buffer = Buffer::new(FRAME_SIZE);
        b.iter(|| full_frame_scroll(&mut store, &mut buffer, black_box(10_000)));
    });
}

criterion_group!(benches, bench_full_frame, bench_scroll_frame);
criterion_main!(benches);
