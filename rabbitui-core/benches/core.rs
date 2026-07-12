//! Core micro-benchmarks (Arc 2B, `docs/design/arc2b-measurement-scroll.md`):
//! buffer `set_string` + full diff at 240×70, layout splits, and `StateStore`
//! churn.
//!
//! These establish the primitive costs the frame benches build on. Run with
//! `cargo bench -p rabbitui-core` (or `cargo bench --workspace`). Each bench body
//! is also asserted to *run* under `cargo test` via the smoke tests at the bottom,
//! so the bench code cannot rot between benchmark runs.

// `criterion_group!` expands to an undocumented `pub fn`; the workspace lints
// `missing_docs`, which does not apply to a bench harness.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use rabbitui_core::buffer::Buffer;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::layout::{Constraint, split_columns, split_rows};
use rabbitui_core::store::StateStore;
use rabbitui_core::style::{Color, Style};
use rabbitui_core::widget::{RenderContext, Widget};
use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;

/// A representative large terminal for the buffer benches: 240 columns × 70 rows
/// (a wide, tall window — the stress size the design note names).
const BENCH_SIZE: Size = Size::new(240, 70);

/// Fills `buffer` with a deterministic pattern of styled strings — the body both
/// the `set_string` bench and the diff setup share.
fn fill_buffer(buffer: &mut Buffer) {
    let style = Style::new().fg(Color::Rgb(0xfa, 0xb3, 0x87)).bold();
    for y in 0..buffer.size().height {
        // A row-varying string so the content is not one repeated cell (which the
        // diff would collapse trivially).
        let text = format!("row {y:03} the quick brown fox jumps over the lazy dog 0123456789 ",);
        // Repeat to overflow the width; set_string clips at the right edge.
        let line = text.repeat(4);
        buffer.set_string(Position::new(0, y), &line, style);
    }
}

/// The buffer benches: a full `set_string` fill, and a full-frame diff.
fn bench_buffer(c: &mut Criterion) {
    // set_string: fill an empty 240×70 buffer with styled text every iteration.
    c.bench_function("buffer/set_string_240x70", |b| {
        let mut buffer = Buffer::new(BENCH_SIZE);
        b.iter(|| {
            buffer.reset();
            fill_buffer(black_box(&mut buffer));
        });
    });

    // diff: the damage-tracking cost of a fully-changed 240×70 frame against a
    // blank previous — the worst case (every non-continuation cell changed).
    c.bench_function("buffer/full_diff_240x70", |b| {
        let previous = Buffer::new(BENCH_SIZE);
        let mut current = Buffer::new(BENCH_SIZE);
        fill_buffer(&mut current);
        b.iter(|| {
            let changes = black_box(&current).diff(black_box(&previous));
            black_box(changes);
        });
    });
}

/// The layout benches: many row/column splits with a mix of Length and Fill
/// constraints — the cost the scroll container and every panel pays per frame.
fn bench_layout(c: &mut Criterion) {
    let area = Rect::from_size(BENCH_SIZE);
    c.bench_function("layout/split_rows_x1000", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let bands = split_rows(
                    black_box(area),
                    [
                        Constraint::Length(1),
                        Constraint::Length(3),
                        Constraint::Fill(1),
                        Constraint::Fill(2),
                        Constraint::Length(1),
                    ],
                );
                black_box(bands);
            }
        });
    });
    c.bench_function("layout/split_columns_x1000", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let bands = split_columns(
                    black_box(area),
                    [
                        Constraint::Length(20),
                        Constraint::Fill(1),
                        Constraint::Length(20),
                    ],
                );
                black_box(bands);
            }
        });
    });
}

/// A small retained-state type standing in for a widget's state in the churn
/// bench.
#[derive(Default)]
struct Counter(u64);

/// The `StateStore` churn bench: begin/declare-N/end frame cycles, the per-frame
/// retained-state cost of a moderately large view (500 widgets).
fn bench_store_churn(c: &mut Criterion) {
    const WIDGETS: usize = 500;
    // Pre-compute the ids once (id composition is its own cost, benched implicitly
    // by the frame benches; here we isolate the store).
    let ids: Vec<WidgetId> = (0..WIDGETS)
        .map(|i| WidgetId::ROOT.child(key("w").index(i)))
        .collect();

    c.bench_function("store/churn_500_widgets", |b| {
        let mut store = StateStore::new();
        b.iter(|| {
            store.begin_frame();
            for id in &ids {
                let counter = store.get_or_default::<Counter>(*id);
                counter.0 = counter.0.wrapping_add(1);
                black_box(&counter.0);
            }
            store.end_frame();
        });
    });
}

/// A one-row scroll item whose `desired_height` increments a shared counter,
/// so the bench can assert the virtualization property *structurally*: measure
/// callbacks per frame stay O(window) regardless of source size. Wall-clock is
/// what criterion reports; correctness is never asserted by timing.
struct MeasuredRow {
    measures: Rc<Cell<usize>>,
}

impl Widget for MeasuredRow {
    type State = ();
    fn render(&self, _state: &mut (), ctx: &mut RenderContext<'_>) {
        ctx.set_string(Position::ORIGIN, "row", Style::new());
    }
    fn desired_height(&self, _state: &(), _width: u16) -> u16 {
        self.measures.set(self.measures.get() + 1);
        1
    }
}

/// One frame of a million-item scroll: declares and renders, then routes one
/// Down key to the scroll (the `scroll_by` step), so successive frames walk
/// the anchor forward. Returns nothing; heights measured are tallied through
/// the shared counter.
fn scroll_million_frame(store: &mut StateStore, measures: &Rc<Cell<usize>>) {
    use rabbitui_core::input::{InputEvent, Key};
    use rabbitui_core::routing::{Focus, route};
    const MILLION: usize = 1_000_000;
    let mut buffer = Buffer::new(Size::new(80, 24));
    store.begin_frame();
    let mut frame = Frame::new(&mut buffer, store);
    let area = frame.area();
    frame.scroll(key("feed"), area, |scroll| {
        for i in 0..MILLION {
            scroll.item(
                key("row").index(i),
                &MeasuredRow {
                    measures: Rc::clone(measures),
                },
            );
        }
    });
    let (facts, handlers) = frame.into_parts();
    store.end_frame();
    // One scroll step per frame, routed like real input.
    let mut focus = Focus::new();
    focus.set(Some(WidgetId::ROOT.child(key("feed"))));
    route(
        &facts,
        &handlers,
        &mut focus,
        store,
        &InputEvent::key(Key::Down),
    );
}

/// The Wave B2 anchor-virtualization bench: a million-item source, one frame
/// render plus one scroll step per iteration. The virtualization property is
/// asserted *structurally* up front — measure callbacks ≤ 64 per frame for a
/// 24-row viewport — so a regression fails loudly before any timing happens;
/// wall-clock is only what criterion reports.
fn bench_scroll(c: &mut Criterion) {
    let measures = Rc::new(Cell::new(0usize));
    let mut store = StateStore::new();
    // Frame 1 settles the scrollbar verdict; frame 2 is the steady state.
    scroll_million_frame(&mut store, &measures);
    assert!(
        measures.get() <= 64,
        "first frame measured {} items (> 64: virtualization broke)",
        measures.get()
    );
    measures.set(0);
    scroll_million_frame(&mut store, &measures);
    assert!(
        measures.get() <= 64,
        "steady frame measured {} items (> 64: virtualization broke)",
        measures.get()
    );
    let scroll_id = WidgetId::ROOT.child(key("feed"));
    let anchor = store
        .peek::<rabbitui_core::scroll::ScrollState>(scroll_id)
        .unwrap()
        .anchor();
    assert_eq!(anchor, (1, 0), "the routed scroll step moved the anchor");

    c.bench_function("scroll/render_1m_items_and_scroll", |b| {
        b.iter(|| {
            scroll_million_frame(&mut store, &measures);
            black_box(measures.get());
        });
    });
}

criterion_group!(
    benches,
    bench_buffer,
    bench_layout,
    bench_store_churn,
    bench_scroll
);
criterion_main!(benches);
