//! Smoke tests for the core benchmark bodies (Arc 2B).
//!
//! `cargo bench` is opt-in and slow, so it rarely runs in CI; without a cheap
//! guard the bench code silently rots (an API rename breaks it, unnoticed). These
//! `#[test]`s run each bench's *workload* once — no timing — so a plain
//! `cargo test --workspace` proves every bench body still compiles and executes.
//! They mirror `benches/core.rs`; keep them in step.

use std::cell::Cell;
use std::rc::Rc;

use rabbitui_core::buffer::Buffer;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::input::{InputEvent, Key};
use rabbitui_core::layout::{Constraint, split_columns, split_rows};
use rabbitui_core::routing::{Focus, route};
use rabbitui_core::scroll::ScrollState;
use rabbitui_core::store::StateStore;
use rabbitui_core::style::{Color, Style};
use rabbitui_core::widget::{RenderContext, Widget};

const BENCH_SIZE: Size = Size::new(240, 70);

fn fill_buffer(buffer: &mut Buffer) {
    let style = Style::new().fg(Color::Rgb(0xfa, 0xb3, 0x87)).bold();
    for y in 0..buffer.size().height {
        let text = format!("row {y:03} the quick brown fox jumps over the lazy dog 0123456789 ");
        let line = text.repeat(4);
        buffer.set_string(Position::new(0, y), &line, style);
    }
}

#[test]
fn set_string_and_diff_body_runs() {
    let mut buffer = Buffer::new(BENCH_SIZE);
    fill_buffer(&mut buffer);
    // Something was painted (the top-left cell is non-blank).
    assert_ne!(buffer.get(Position::ORIGIN).unwrap().symbol, " ");

    let previous = Buffer::new(BENCH_SIZE);
    let changes = buffer.diff(&previous);
    // A fully-filled frame against a blank one produces changes.
    assert!(!changes.is_empty());
}

#[test]
fn layout_split_body_runs() {
    let area = Rect::from_size(BENCH_SIZE);
    let rows = split_rows(
        area,
        [
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Fill(2),
            Constraint::Length(1),
        ],
    );
    // The bands tile the height exactly.
    let total: u16 = rows.iter().map(|r| r.size.height).sum();
    assert_eq!(total, BENCH_SIZE.height);

    let cols = split_columns(
        area,
        [
            Constraint::Length(20),
            Constraint::Fill(1),
            Constraint::Length(20),
        ],
    );
    let total_w: u16 = cols.iter().map(|r| r.size.width).sum();
    assert_eq!(total_w, BENCH_SIZE.width);
}

/// Mirrors `benches/core.rs`'s `MeasuredRow`: a one-row scroll item counting
/// `desired_height` calls, so the virtualization property is asserted
/// structurally — by measure-callback count, never by wall-clock.
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

/// Mirrors the scroll bench body: one frame of a million-item scroll plus one
/// routed Down key (the scroll step).
fn scroll_million_frame(store: &mut StateStore, measures: &Rc<Cell<usize>>) {
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

#[test]
fn scroll_million_body_runs_and_measures_o_window() {
    let measures = Rc::new(Cell::new(0usize));
    let mut store = StateStore::new();
    scroll_million_frame(&mut store, &measures);
    assert!(
        measures.get() <= 64,
        "first frame measured {} items",
        measures.get()
    );
    measures.set(0);
    scroll_million_frame(&mut store, &measures);
    assert!(
        measures.get() <= 64,
        "steady frame measured {} items",
        measures.get()
    );
    assert!(measures.get() > 0, "the fresh window is still re-measured");
    let scroll_id = WidgetId::ROOT.child(key("feed"));
    assert_eq!(
        store.peek::<ScrollState>(scroll_id).unwrap().anchor(),
        (1, 0),
        "the routed scroll step moved the anchor"
    );
}

#[derive(Default)]
struct Counter(u64);

#[test]
fn store_churn_body_runs() {
    const WIDGETS: usize = 500;
    let ids: Vec<WidgetId> = (0..WIDGETS)
        .map(|i| WidgetId::ROOT.child(key("w").index(i)))
        .collect();
    let mut store = StateStore::new();
    // Two frames so state persists and the second sees the incremented values.
    for _ in 0..2 {
        store.begin_frame();
        for id in &ids {
            store.get_or_default::<Counter>(*id).0 += 1;
        }
        store.end_frame();
    }
    assert_eq!(store.len(), WIDGETS);
    assert_eq!(store.peek::<Counter>(ids[0]).unwrap().0, 2);
}
