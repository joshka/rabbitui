//! Item 4 (arc4-spine.md §5): the catalog widgets record their accessibility
//! **role** and **label** into frame facts, so a future accessibility exporter has what it
//! needs. This asserts those facts for a representative gallery, and that the
//! devtools facts dump surfaces them.

use rabbitui_core::accessibility::SemanticRole;
use rabbitui_core::buffer::Buffer;
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::{WidgetId, key};
use rabbitui_core::store::StateStore;
use rabbitui_widgets::{Button, Collapsible, ErrorBanner, SelectionList, Text, TextInput};

/// Declares one of each interactive catalog widget into a frame and returns the
/// facts, so roles/labels can be asserted against declared identities.
fn gallery_facts() -> rabbitui_core::facts::FrameFacts {
    let mut buffer = Buffer::new(Size::new(40, 12));
    let mut store = StateStore::new();
    store.begin_frame();
    let mut frame = Frame::new(&mut buffer, &mut store);

    frame.widget(
        key("heading"),
        Rect::new(Position::ORIGIN, Size::new(40, 1)),
        &Text::new("Settings"),
    );
    frame.widget(
        key("save"),
        Rect::new(Position::new(0, 1), Size::new(8, 1)),
        &Button::new("Save"),
    );
    frame.widget(
        key("search"),
        Rect::new(Position::new(0, 2), Size::new(20, 1)),
        &TextInput::new().placeholder("Filter…"),
    );
    frame.widget(
        key("items"),
        Rect::new(Position::new(0, 3), Size::new(20, 4)),
        &SelectionList::new(vec!["a".to_string(), "b".to_string()]),
    );
    frame.widget(
        key("details"),
        Rect::new(Position::new(0, 7), Size::new(20, 1)),
        &Collapsible::new("Advanced", "body text"),
    );
    frame.widget(
        key("error"),
        Rect::new(Position::new(0, 8), Size::new(30, 4)),
        &ErrorBanner::new("disk full").title("Save failed"),
    );

    let facts = frame.finish();
    store.end_frame();
    facts
}

fn id(name: &str) -> WidgetId {
    WidgetId::ROOT.child(key(name))
}

#[test]
fn catalog_widgets_record_semantic_roles() {
    let facts = gallery_facts();
    assert_eq!(facts.role(id("heading")), SemanticRole::Label);
    assert_eq!(facts.role(id("save")), SemanticRole::Button);
    assert_eq!(facts.role(id("search")), SemanticRole::TextInput);
    assert_eq!(facts.role(id("items")), SemanticRole::List);
    assert_eq!(facts.role(id("details")), SemanticRole::Disclosure);
    assert_eq!(facts.role(id("error")), SemanticRole::Dialog);
}

#[test]
fn catalog_widgets_record_accessible_labels() {
    let facts = gallery_facts();
    assert_eq!(facts.label(id("heading")), Some("Settings"));
    assert_eq!(facts.label(id("save")), Some("Save"));
    // A text field is labelled by its placeholder (its purpose), not its value.
    assert_eq!(facts.label(id("search")), Some("Filter…"));
    assert_eq!(facts.label(id("details")), Some("Advanced"));
    assert_eq!(facts.label(id("error")), Some("Save failed: disk full"));
    // The list declares a role but no label (its items are the content).
    assert_eq!(facts.label(id("items")), None);
}

#[cfg(feature = "devtools")]
#[test]
fn dump_surfaces_roles_and_labels() {
    let facts = gallery_facts();
    let lines = facts.dump_lines(None);
    // The Save button's line carries both its role and its label.
    let save = lines
        .iter()
        .find(|l| l.contains(" save "))
        .expect("save line present");
    assert!(save.contains("role=button"), "line: {save}");
    assert!(save.contains(r#"label="Save""#), "line: {save}");

    // The heading is a label role with its text.
    let heading = lines
        .iter()
        .find(|l| l.contains(" heading "))
        .expect("heading line present");
    assert!(heading.contains("role=label"), "line: {heading}");
    assert!(heading.contains(r#"label="Settings""#), "line: {heading}");
}
