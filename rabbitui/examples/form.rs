//! A validated form with a modal confirm, declared with the [`form`] helper.
//!
//! The layout is not hand-rolled: [`form`] lays out each field — a right-aligned
//! label column, the input beside it, an error line below — so the [`view`] reads
//! top to bottom as a *description* of the form, one [`field`](FormScope::field)
//! call per row. What the example still owns is what a framework must never own:
//!
//! - **Validation, in `update`.** The app validates each field on its
//!   [`Changed`]/[`Submitted`] outcome and stores an `Option<String>` error; the
//!   form only *displays* it (ADR 0001). The required `*` markers are hints the
//!   form draws; the *rules* live here.
//! - **A modal on a z-layer.** Submitting opens a confirm modal declared with
//!   [`Frame::layer`]: while it is open Tab cycles only its two buttons and Esc
//!   dismisses it, and focus moves into it via a declare-then-focus request.
//! - **Mouse routing through facts.** Clicking a field focuses it, clicking a
//!   button activates it, and the wheel over the notes list moves its selection —
//!   all routed by the framework against the previous frame's facts, layer-aware,
//!   with no mouse code in the example.
//!
//! Run with `cargo run --example form`. Tab/↑↓ between fields; type a name and an
//! email; press Submit (or Enter in a field) to open the confirm modal; Ok
//! submits and clears, Cancel or Esc dismisses; `q` (no field focused) or Ctrl-C
//! quits.
//!
//! [`form`]: rabbitui_widgets::form::form
//! [`FormScope::field`]: rabbitui_widgets::form::FormScope::field
//! [`field`]: rabbitui_widgets::form::FormScope::field
//! [`view`]: rabbitui::App::view
//! [`Changed`]: rabbitui_core::outcome::Outcome::Changed
//! [`Submitted`]: rabbitui_core::outcome::Outcome::Submitted
//! [`Frame::layer`]: rabbitui_core::frame::Frame::layer

use std::ops::ControlFlow;

use rabbitui::App;
use rabbitui::app::{Event, Update};
use rabbitui_core::frame::Frame;
use rabbitui_core::geometry::{Position, Rect, Size};
use rabbitui_core::id::key;
use rabbitui_core::input::Key;
use rabbitui_core::layout::{Constraint, center, split_rows};
use rabbitui_core::outcome::Outcome;
use rabbitui_core::theme::Role;
use rabbitui_widgets::form::{FieldSpec, form, label_width};
use rabbitui_widgets::{Button, Panel, SelectionList, Text, TextInput};

/// The form's owned state — including the validation errors the form displays.
#[derive(Default)]
struct Form {
    /// The current name draft, tracked from the name field's `Changed` outcomes.
    name: String,
    /// The current email draft, tracked from the email field's `Changed` outcomes.
    email: String,
    /// The name field's validation error, recomputed in `update` (app-land).
    name_error: Option<String>,
    /// The email field's validation error, recomputed in `update` (app-land).
    email_error: Option<String>,
    /// Whether the confirm modal is open.
    confirming: bool,
    /// Whether a focus request into the modal is still owed (set when the modal
    /// opens, cleared once honored — the declare-then-focus handshake).
    focus_modal: bool,
    /// A status line shown after a successful submit.
    submitted: Option<String>,
}

/// The field labels, in declaration order — the single source for the label
/// column width, so the layout stays arithmetic-free.
const LABELS: [&str; 3] = ["Name", "Email", "Notes"];

/// The notes options — a small list to prove wheel-over-list routing.
const NOTES: &[&str] = &[
    "Follow up by email",
    "Add to newsletter",
    "No further contact",
];

impl Form {
    /// Validates the name: non-empty. Returns the error to display, if any — the
    /// app's rule, called on the name field's edits.
    fn validate_name(name: &str) -> Option<String> {
        name.trim()
            .is_empty()
            .then(|| "name is required".to_string())
    }

    /// Validates the email: looks like an address (an `@` with text around it).
    fn validate_email(email: &str) -> Option<String> {
        let email = email.trim();
        if email.is_empty() {
            Some("email is required".to_string())
        } else if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
            Some("email must contain @".to_string())
        } else {
            None
        }
    }

    /// Whether the form is submittable (both fields currently valid).
    fn valid(&self) -> bool {
        Self::validate_name(&self.name).is_none() && Self::validate_email(&self.email).is_none()
    }

    /// Closes the modal and clears the transient focus request.
    fn close_modal(&mut self) {
        self.confirming = false;
        self.focus_modal = false;
    }
}

impl App for Form {
    /// Folds one update into the form: track field edits, **validate** them, open
    /// or close the modal, and quit.
    fn update(&mut self, update: Update<'_>) -> ControlFlow<()> {
        // Track and validate the two text fields (uncontrolled inputs report via
        // Changed). Validation is the app's — the form only displays the result.
        if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("form"), key("name")]) {
            self.name = value.clone();
            self.name_error = Self::validate_name(&self.name);
        }
        if let Some(Outcome::Changed(value)) = update.outcome_for(&[key("form"), key("email")]) {
            self.email = value.clone();
            self.email_error = Self::validate_email(&self.email);
        }

        if self.confirming {
            // Modal is open: Ok submits and closes; Cancel closes.
            if update.outcome_for(&[key("modal"), key("ok")]) == Some(&Outcome::Activated) {
                self.submitted = Some(format!(
                    "Submitted {} <{}>",
                    self.name.trim(),
                    self.email.trim()
                ));
                self.close_modal();
            }
            if update.outcome_for(&[key("modal"), key("cancel")]) == Some(&Outcome::Activated) {
                self.close_modal();
            }
            // Esc dismisses the modal (an unconsumed key at the base falls through).
            if let Event::Input(input) = update.event()
                && input.as_key().map(|k| k.key) == Some(Key::Escape)
            {
                self.close_modal();
            }
            // The focus request into the modal is owed exactly once, when it opens.
            if self.focus_modal {
                update.focus(&[key("modal"), key("ok")]);
                self.focus_modal = false;
            }
        } else {
            // Base form: the Submit button, or Enter inside a text field (the
            // web-form convention), opens the confirm modal when the form is
            // valid. On an invalid submit, surface every field's error and focus
            // the first offender.
            let button =
                update.outcome_for(&[key("form"), key("submit")]) == Some(&Outcome::Activated);
            let field_enter = update.outcome_for(&[key("form"), key("name")])
                == Some(&Outcome::Submitted)
                || update.outcome_for(&[key("form"), key("email")]) == Some(&Outcome::Submitted);
            if button || field_enter {
                if self.valid() {
                    self.confirming = true;
                    self.focus_modal = true;
                    self.submitted = None;
                } else {
                    self.name_error = Self::validate_name(&self.name);
                    self.email_error = Self::validate_email(&self.email);
                    let first_invalid = if self.name_error.is_some() {
                        "name"
                    } else {
                        "email"
                    };
                    update.focus(&[key("form"), key(first_invalid)]);
                }
            }
        }

        // Vertical arrows move between fields (web-form muscle memory). TextInput
        // leaves Up/Down unconsumed; the notes list consumes them for selection,
        // so this only fires where it makes sense.
        if let Event::Input(input) = update.event()
            && !update.consumed()
            && !self.confirming
        {
            let order = ["name", "email", "notes"];
            if let Some(k) = input.as_key() {
                let step: Option<i32> = match k.key {
                    Key::Up => Some(-1),
                    Key::Down => Some(1),
                    _ => None,
                };
                let at = order
                    .iter()
                    .position(|name| update.is_focused(&[key("form"), key(name)]));
                if let (Some(step), Some(at)) = (step, at) {
                    let next = (at as i32 + step).rem_euclid(order.len() as i32) as usize;
                    update.focus(&[key("form"), key(order[next])]);
                }
            }
        }

        // App-level quit: `q` with no field focused, or Ctrl-C. TextInput leaves
        // Ctrl chords for the app, so Ctrl-C quits even while a field is focused.
        if let Event::Input(input) = update.event()
            && let Some(k) = input.as_key()
            && ((k.key == Key::Char('c') && k.modifiers.ctrl)
                || ((k.key == Key::Char('q') && !update.consumed() || k.key == Key::Escape)
                    && !self.confirming))
        {
            return ControlFlow::Break(());
        }

        ControlFlow::Continue(())
    }

    /// Declares the form and, when confirming, the modal layer over it. The field
    /// layout is the `form` helper's job; this reads as a list of fields.
    fn view(&self, frame: &mut Frame<'_>) {
        let area = center(frame.area(), 60, 18);
        let panel = Panel::new()
            .title("form")
            .padding(1)
            .focused(!self.confirming);
        frame.widget(key("panel"), area, &panel);
        let inner = Panel::inner(area, &panel);

        // The whole form, declared as fields: label alignment, the error lines,
        // and the focus order all fall out of the declaration order below.
        let used = form(frame, key("form"), inner, label_width(LABELS), |form| {
            form.field(
                key("name"),
                FieldSpec::new("Name")
                    .required()
                    .error(self.name_error.as_deref()),
                &TextInput::new().placeholder("Name"),
            );
            form.field(
                key("email"),
                FieldSpec::new("Email")
                    .required()
                    .error(self.email_error.as_deref()),
                &TextInput::new().placeholder("Email"),
            );
            form.field(
                key("notes"),
                FieldSpec::new("Notes"),
                &SelectionList::new(NOTES),
            );
            form.gap(1);
            form.buttons(|frame, row| {
                let label = if self.valid() {
                    "[ Submit ]"
                } else {
                    "[ Submit (fill fields) ]"
                };
                frame.widget(
                    key("submit"),
                    row,
                    &SubmitButton {
                        label,
                        enabled: self.valid(),
                    },
                );
            });
        });

        // A result / hint line, placed just under the form (its reported height
        // is the only geometry the caller needs — no field arithmetic).
        let [result_row] = split_rows(
            Rect::new(
                Position::new(inner.origin.x, inner.origin.y + used),
                Size::new(inner.size.width, 1),
            ),
            [Constraint::Length(1)],
        );
        let result = match &self.submitted {
            Some(message) => Text::new(message).role(Role::Success),
            None => Text::new("Tab/↑↓: move  Enter: submit  Ctrl-C: quit").role(Role::Muted),
        };
        frame.widget(key("result"), result_row, &result);

        // The confirm modal, on a higher layer. While declared, Tab cycles only
        // its two buttons and clicks over it never reach the base.
        if self.confirming {
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
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Form::default().run().await?;
    Ok(())
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

    fn render(&self, _state: &mut (), ctx: &mut rabbitui_core::widget::RenderContext<'_>) {
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
        ctx: &mut rabbitui_core::widget::HandleContext<'_>,
    ) -> rabbitui_core::widget::Handled {
        use rabbitui_core::input::{Key, MouseButton, MouseKind};
        use rabbitui_core::widget::Handled;
        // A disabled button never renders focusable, so the router won't target it
        // for keys; it can still be *clicked*, so guard the mouse path too by only
        // activating on a left press.
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
