//! Smoke tests for the core benchmark bodies (Arc 2B).
//!
//! `cargo bench` is opt-in and slow, so it rarely runs in CI; without a cheap
//! guard the bench code silently rots (an API rename breaks it, unnoticed). These
//! `#[test]`s run each bench's *workload* once — no timing — so a plain
//! `cargo test --workspace` proves every bench body still compiles and executes.
//! They mirror `benches/core.rs`; keep them in step.

use rabbitui_core::buffer::Buffer;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::layout::{Constraint, split_columns, split_rows};
use rabbitui_core::store::StateStore;
use rabbitui_core::style::{Color, Style};

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
