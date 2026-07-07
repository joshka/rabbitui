//! Overlays, forms, and mouse: a validated form with a modal confirm — the
//! slice-7 flagship (`docs/design/slice7-overlays-mouse.md`).
//!
//! Demonstrates, end to end:
//!
//! - **A real form.** Name and email [`TextInput`]s plus a small notes
//!   [`SelectionList`], with a Submit [`Button`]. Each field shows an inline
//!   validation status line, styled by [`Role`] — [`Role::Danger`] while invalid,
//!   [`Role::Success`] when it passes. Submit is enabled only when both fields
//!   validate (a disabled button declares itself non-focusable, so Tab skips it).
//! - **A modal on a z-layer.** Submitting opens a confirm modal declared with
//!   [`Frame::layer`](rabbitui_core::frame::Frame::layer): while it is open, Tab
//!   provably cycles only its two buttons (Ok / Cancel) — the base form is inert
//!   — and Esc dismisses it. Focus moves into the modal via a declare-then-focus
//!   request the moment it appears (the one-frame retry closes that edge).
//! - **Mouse routing through facts.** Click a field to focus it, click Ok/Cancel
//!   in the modal, and use the wheel over the notes list to move its selection —
//!   all routed through the same [`route`](rabbitui_core::routing::route) path as
//!   keys, hit-testing the previous frame's facts (layer-aware, so a click over
//!   the modal never reaches the base beneath it).
//!
//! Run with `cargo run --example form`. Tab between fields; type a name and an
//! email; click Submit (or press it while focused) to open the confirm modal;
//! Ok submits and clears, Cancel or Esc dismisses; `q` (with no field focused)
//! or Ctrl-C quits.

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_widgets::{Button, Panel, SelectionList, Text, TextInput};

/// The form's owned state.
#[derive(Default)]
struct Form {
    /// The current name draft, tracked from the name field's `Changed` outcomes.
    name: String,
    /// The current email draft, tracked from the email field's `Changed` outcomes.
    email: String,
    /// Whether the confirm modal is open.
    confirming: bool,
    /// Whether a focus request into the modal is still owed (set when the modal
    /// opens, cleared once honored — the declare-then-focus handshake).
    focus_modal: bool,
    /// A status line shown after a successful submit.
    submitted: Option<String>,
}

impl Form {
    /// Whether the name is non-empty.
    fn name_ok(&self) -> bool {
        !self.name.trim().is_empty()
    }

    /// Whether the email looks like an address (contains `@` with text around it).
    fn email_ok(&self) -> bool {
        let email = self.email.trim();
        email.contains('@') && !email.starts_with('@') && !email.ends_with('@')
    }

    /// Whether the form is submittable (both fields valid).
    fn valid(&self) -> bool {
        self.name_ok() && self.email_ok()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::new(Form::default(), update, view).run().await?;
    Ok(())
}

/// The notes options — a small list to prove wheel-over-list routing.
const NOTES: &[&str] = &[
    "Follow up by email",
    "Add to newsletter",
    "No further contact",
];

/// Folds one update into the form: track field edits, open/close the modal, and
/// quit.
fn update(form: &mut Form, update: Update<'_>) -> ControlFlow<()> {
    // Track the two text fields' edits (uncontrolled inputs report via Changed).
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("name")]) {
        form.name = value.clone();
    }
    if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("email")]) {
        form.email = value.clone();
    }

    if form.confirming {
        // Modal is open: Ok submits and closes; Cancel closes.
        if update.outcome_for(&[key("modal"), key("ok")]) == Some(&Outcome::Activated) {
            form.submitted = Some(format!(
                "Submitted {} <{}>",
                form.name.trim(),
                form.email.trim()
            ));
            close_modal(form, &update);
        }
        if update.outcome_for(&[key("modal"), key("cancel")]) == Some(&Outcome::Activated) {
            close_modal(form, &update);
        }
        // Esc dismisses the modal (an unconsumed key at the base falls through).
        if let Event::Input(input) = update.event() {
            if input.as_key().map(|k| k.key) == Some(Key::Escape) {
                close_modal(form, &update);
            }
        }
        // The focus request into the modal is owed exactly once, when it opens.
        if form.focus_modal {
            update.focus(&[key("modal"), key("ok")]);
            form.focus_modal = false;
        }
    } else {
        // Base form: the Submit button, or Enter inside either field (the
        // web-form convention — a field's Submitted outcome attempts the form
        // submit), opens the confirm modal when the form is valid. Enter in a
        // field of an invalid form focuses the first invalid field instead,
        // whose status line already says what is wrong.
        let button = update.outcome_for(&[key("submit")]) == Some(&Outcome::Activated);
        let field_enter = update.outcome_for(&[key("name")]) == Some(&Outcome::Submitted)
            || update.outcome_for(&[key("email")]) == Some(&Outcome::Submitted);
        if button || field_enter {
            if form.valid() {
                form.confirming = true;
                form.focus_modal = true;
                form.submitted = None;
            } else if field_enter {
                let invalid = if form.name.trim().is_empty() { "name" } else { "email" };
                update.focus(&[key(invalid)]);
            }
        }
    }

    // App-level quit: `q` with no field focused, or Ctrl-C. TextInput leaves
    // Ctrl chords for the app, so Ctrl-C quits even while a field is focused.
    if let Event::Input(input) = update.event() {
        if let Some(k) = input.as_key() {
            if (k.key == Key::Char('c') && k.modifiers.ctrl)
                || ((k.key == Key::Char('q') && !update.consumed() || k.key == Key::Escape)
                    && !form.confirming)
            {
                return ControlFlow::Break(());
            }
        }
    }

    ControlFlow::Continue(())
}

/// Closes the modal and clears the transient focus request.
fn close_modal(form: &mut Form, _update: &Update<'_>) {
    form.confirming = false;
    form.focus_modal = false;
}

/// Declares the form and, when confirming, the modal layer over it.
fn view(form: &Form, frame: &mut Frame<'_>) {
    // A centered form panel at a sensible width — a form shouldn't sprawl across
    // a wide terminal. Its border highlights while a field (not the modal) holds
    // focus; while the modal is up, the base reads as inert (unfocused border).
    let area = center(frame.area(), 60, 13);
    let panel = Panel::new().title("form").padding(1).focused(!form.confirming);
    frame.widget(key("panel"), area, &panel);
    let inner = Panel::inner(area, &panel);

    let [
        name_row,
        name_status,
        email_row,
        email_status,
        notes_area,
        submit_row,
        result_row,
    ] = split_rows(
        inner,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(NOTES.len() as u16),
            Constraint::Length(1),
            Constraint::Fill(1),
        ],
    );

    // Name field + its validation status.
    frame.widget(key("name"), name_row, &TextInput::new().placeholder("Name"));
    let (name_msg, name_role) = if form.name.is_empty() {
        ("  name: required".to_string(), Role::Muted)
    } else if form.name_ok() {
        ("  name: ok".to_string(), Role::Success)
    } else {
        ("  name: must not be blank".to_string(), Role::Danger)
    };
    frame.widget(
        key("name_status"),
        name_status,
        &Text::new(&name_msg).role(name_role),
    );

    // Email field + its validation status.
    frame.widget(
        key("email"),
        email_row,
        &TextInput::new().placeholder("Email"),
    );
    let (email_msg, email_role) = if form.email.is_empty() {
        ("  email: required".to_string(), Role::Muted)
    } else if form.email_ok() {
        ("  email: ok".to_string(), Role::Success)
    } else {
        ("  email: must contain @".to_string(), Role::Danger)
    };
    frame.widget(
        key("email_status"),
        email_status,
        &Text::new(&email_msg).role(email_role),
    );

    // Notes: a small selection list (proves wheel-over-list routing).
    frame.widget(key("notes"), notes_area, &SelectionList::new(NOTES));

    // Submit: focusable/clickable only when the form validates.
    let submit_label = if form.valid() {
        "[ Submit ]"
    } else {
        "[ Submit (fill fields) ]"
    };
    frame.widget(
        key("submit"),
        submit_row,
        &SubmitButton {
            label: submit_label,
            enabled: form.valid(),
        },
    );

    // A result / hint line.
    let result = match &form.submitted {
        Some(message) => Text::new(message).role(Role::Success),
        None => {
            Text::new("Tab: move  click: focus  Enter/click Submit  Esc: quit").role(Role::Muted)
        }
    };
    frame.widget(key("result"), result_row, &result);

    // The confirm modal, on a higher layer. While declared, Tab cycles only its
    // two buttons and clicks over it never reach the base (facts hit-test prefers
    // the top layer). It is its own centered, focused panel floating over the
    // form — the overlay reads as a distinct surface, not text over text.
    if form.confirming {
        let modal_area = center(frame.area(), 44, 8);
        frame.layer(key("modal"), |modal| {
            let modal_panel = Panel::new().title("confirm").padding(1).focused(true);
            modal.widget(key("bg"), modal_area, &modal_panel);
            let modal_inner = Panel::inner(modal_area, &modal_panel);

            let [prompt, _, ok_row, cancel_row] = split_rows(
                modal_inner,
                [
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ],
            );
            modal.widget(
                key("prompt"),
                prompt,
                &Text::new("Submit this form? (Esc to cancel)").role(Role::Warning),
            );
            modal.widget(key("ok"), ok_row, &Button::new("[ Ok ]"));
            modal.widget(key("cancel"), cancel_row, &Button::new("[ Cancel ]"));
        });
    }
}

/// A submit button that opts out of focusability (and clicks) when disabled.
///
/// Reuses [`Button`]'s look but gates focusability on `enabled`: a disabled
/// button declares `focusable(false)`, so Tab skips it and a click on it hits no
/// focusable target — exactly what "Submit is only reachable when valid" needs.
struct SubmitButton<'a> {
    label: &'a str,
    enabled: bool,
}

impl rabbitui_core::widget::Widget for SubmitButton<'_> {
    type State = ();

    fn render(&self, _state: &mut (), ctx: &mut rabbitui_core::widget::RenderCtx<'_>) {
        ctx.focusable(self.enabled);
        let role = if !self.enabled {
            Role::Muted
        } else if ctx.is_focused() {
            Role::Highlight
        } else {
            Role::Accent
        };
        let style = ctx.style(role);
        ctx.set_string(rabbitui_core::geometry::Position::ORIGIN, self.label, style);
    }

    fn handle(
        _state: &mut (),
        event: &rabbitui_core::input::InputEvent,
        ctx: &mut rabbitui_core::widget::HandleCtx<'_>,
    ) -> rabbitui_core::widget::Handled {
        use rabbitui_core::input::{Key, MouseButton, MouseKind};
        use rabbitui_core::widget::Handled;
        // A disabled button never renders focusable, so the router won't target it
        // for keys; it can still be *clicked*, so guard the mouse path too by only
        // activating on a left press (a disabled button emits nothing because the
        // app checks `valid()` before opening the modal, but activating on click is
        // still the button contract).
        if let Some(mouse) = event.as_mouse() {
            if mouse.button == MouseButton::Left && mouse.kind == MouseKind::Down {
                ctx.emit(Outcome::Activated);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(k) = event.as_key() else {
            return Handled::No;
        };
        match k.key {
            Key::Enter | Key::Char(' ') => {
                ctx.emit(Outcome::Activated);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
