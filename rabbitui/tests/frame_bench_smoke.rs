//! Smoke tests for the frame benchmark bodies (Arc 2B).
//!
//! Runs each `benches/frame.rs` workload once (no timing), so `cargo test` proves
//! the declared-frame and scroll bench bodies still compile and execute. Mirrors
//! `benches/frame.rs`; keep them in step.

use rabbitui::core::buffer::Buffer;
use rabbitui::core::frame::Frame;
use rabbitui::core::geometry::{Position, Rect, Size};
use rabbitui::core::id::{WidgetId, key};
use rabbitui::core::store::StateStore;
use rabbitui::core::style::Style;
use rabbitui::core::widget::{RenderContext, Widget};

struct Cell {
    label: &'static str,
}

impl Widget for Cell {
    type State = ();
    fn render(&self, (): &mut (), ctx: &mut RenderContext<'_>) {
        ctx.set_string(Position::ORIGIN, self.label, Style::new());
    }
}

const FRAME_SIZE: Size = Size::new(240, 70);

#[test]
fn declared_flat_frame_body_runs() {
    // A smaller count than the bench (1,000): enough to exercise the declare →
    // facts → paint path and prove facts accrue, fast enough for `cargo test`.
    const COUNT: usize = 1_000;
    let mut store = StateStore::new();
    let mut buffer = Buffer::new(FRAME_SIZE);
    store.begin_frame();
    let (facts, handlers) = {
        let mut frame = Frame::new(&mut buffer, &mut store);
        for i in 0..COUNT {
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
    store.end_frame();
    // Every declared widget recorded a fact and a handler.
    assert_eq!(handlers.len(), COUNT);
    let first = WidgetId::ROOT.child(key("cell").index(0));
    assert!(facts.get(first).is_some());
}

#[test]
fn scroll_frame_body_runs() {
    // The measure-twice scroll path over many items; only the visible few paint,
    // but all are measured. A smaller count keeps `cargo test` quick.
    const COUNT: usize = 1_000;
    let mut store = StateStore::new();
    let mut buffer = Buffer::new(FRAME_SIZE);
    store.begin_frame();
    {
        let mut frame = Frame::new(&mut buffer, &mut store);
        let area = frame.area();
        frame.scroll(key("scroll"), area, |scroll| {
            for i in 0..COUNT {
                scroll.item(
                    key("item").index(i),
                    &Cell {
                        label: "synthetic row",
                    },
                );
            }
        });
        let _ = frame.into_parts();
    }
    store.end_frame();
    // The scroll scope retained state; only a screenful of items painted (far
    // fewer than COUNT), proving virtualization ran.
    let scope = WidgetId::ROOT.child(key("scroll"));
    assert!(
        store
            .peek::<rabbitui::core::scroll::ScrollState>(scope)
            .is_some()
    );
    let painted = (0..COUNT)
        .filter(|i| {
            store
                .peek::<()>(scope.child(key("item").index(*i)))
                .is_some()
        })
        .count();
    assert!(painted <= usize::from(FRAME_SIZE.height));
    assert!(painted > 0);
}
