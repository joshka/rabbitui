//! Accessibility groundwork: the semantic vocabulary recorded into frame facts.
//!
//! Per `docs/plans/arc4-spine.md` §5 and `docs/design/accessibility.md`, rabbitui
//! records — but does not yet *export* — the two things an assistive-technology
//! bridge needs beyond geometry: each widget's **semantic role** (is this a
//! button, a text field, a dialog?) and its **accessible label** (the human name a
//! screen reader would announce). A widget declares them through
//! [`RenderContext::semantic_role`](crate::widget::RenderContext::semantic_role) and
//! [`RenderContext::label`](crate::widget::RenderContext::label); the frame records them on
//! the widget's [`FactEntry`](crate::facts::FactEntry) next to its area and focus
//! fact.
//!
//! The **exporter** (AT-SPI on Linux, UIA on Windows, an in-process test probe) is
//! deliberately out of scope here. The architectural point the field reports make
//! is that the facts already carry what an exporter needs, so adding one later is a
//! consumer of existing data, not a re-plumbing. See the design note for the export
//! path and what is deferred.

/// The accessibility role of a widget — what *kind* of control it is, for an
/// assistive-technology exporter to announce (ADR arc4 §5).
///
/// A small, closed vocabulary mirroring the catalog's widget kinds and the common
/// container roles. It is recorded on the widget's
/// [`FactEntry`](crate::facts::FactEntry); no widget is *required* to set one
/// ([`None`](SemanticRole::None) is the default), but the catalog widgets do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SemanticRole {
    /// No role declared — the default. A decorative or purely-visual widget (a
    /// spacer, a backdrop) that an exporter would skip.
    #[default]
    None,
    /// A push button that activates on Enter/Space or click (e.g. [`Button`]).
    ///
    /// [`Button`]: https://docs.rs/rabbitui-widgets/latest/rabbitui_widgets/struct.Button.html
    Button,
    /// A single- or multi-line editable text field (e.g. `TextInput`).
    TextInput,
    /// A selectable list of items (e.g. `SelectionList`).
    List,
    /// A grid of rows and columns with a selectable row (e.g. `Table`).
    Table,
    /// A modal dialog / overlay grouping (an `ErrorBanner`, a confirm modal).
    Dialog,
    /// A static, non-interactive text label (e.g. `Text`).
    Label,
    /// A read-only log / status readout (e.g. `LogOverlay`).
    Log,
    /// A disclosure / expander header whose body toggles (e.g. `Collapsible`).
    Disclosure,
}

impl SemanticRole {
    /// A stable lowercase identifier for the role, for the facts dump and a future
    /// exporter's role mapping. `None` renders as the empty string so it can be
    /// omitted from a line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SemanticRole::None => "",
            SemanticRole::Button => "button",
            SemanticRole::TextInput => "textinput",
            SemanticRole::List => "list",
            SemanticRole::Table => "table",
            SemanticRole::Dialog => "dialog",
            SemanticRole::Label => "label",
            SemanticRole::Log => "log",
            SemanticRole::Disclosure => "disclosure",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SemanticRole;

    #[test]
    fn default_is_none() {
        assert_eq!(SemanticRole::default(), SemanticRole::None);
        assert_eq!(SemanticRole::None.as_str(), "");
    }

    #[test]
    fn roles_have_stable_identifiers() {
        assert_eq!(SemanticRole::Button.as_str(), "button");
        assert_eq!(SemanticRole::TextInput.as_str(), "textinput");
        assert_eq!(SemanticRole::List.as_str(), "list");
        assert_eq!(SemanticRole::Table.as_str(), "table");
        assert_eq!(SemanticRole::Disclosure.as_str(), "disclosure");
    }
}
