# Breaking changes

This file records every user-visible breaking change to the rabbitui crates, so
that upgrading is a matter of reading one list rather than diffing release tags.
The discipline is copied from ratatui: the file exists from day one, before there
is any stability to protect, so the habit is in place when it matters.

## Format

Changes are grouped under the version that introduced them, newest first. Each
entry says what changed and — where a migration is not obvious — how to update.
The list is written for a reader upgrading _from the previous version_; it is not
a changelog of every commit, only the changes that break compilation or behavior.

## Pre-0.1 stance

rabbitui has not shipped a 0.1 release. **Until 0.1, everything may change** —
every type, signature, and behavior in every crate is provisional, and no
compatibility is promised across the pre-release slices that built the framework
(see `ROADMAP.md`). The entries below are therefore not a migration guide for
external users, who have none to migrate; they are an honest record of how the
API moved as the vertical slices landed, kept so the 0.1 surface is understood
and the breaking-changes habit is established before the first published release.

Once 0.1 ships, this section is replaced by the normal semver contract and every
breaking change lands under a version heading with migration notes.

## Unreleased (pre-0.1 slices)

Notable API shifts recorded during the pre-0.1 slice work, most significant
first. These are not versioned because no version boundary separates them; they
are the deltas an early reader of the code will notice against earlier slices.

- **`Event<M>` lost its `Copy` implementation.** When effects (slice 6)
  introduced the message payload, `Event` gained a generic parameter `M` and a
  `Message(M)` variant. Because `M` is arbitrary app data — commonly not `Copy`
  — `Event<M>` can no longer be `Copy`. The app receives the event by reference
  from [`Update::event`], so match on it in place (`if let Event::Input(input) =
  update.event()`) rather than copying it out. Message-less apps use the default
  `M = ()` and are unaffected in practice.

- **The `update` signature changed across slices.** The walking skeleton's
  `update` took the raw event; from slice 3 it takes a single [`Update`] value
  that bundles the event, the typed outcomes routing produced, and a sink for
  buffered side effects (commits, mode switches, spawned effects, widget
  commands, focus requests). Reach for the event with `update.event()` and for a
  widget's result with `update.outcome_for(&[key(...)])`. This consolidation is
  why later capabilities (effects, inline commits, widget commands) could be
  added without changing the `update` arity again.

- **`CommitLine` became a `Vec<Span>`.** Inline mode (slice 5) shipped one style
  per committed scrollback line and recorded multi-span lines as a known ceiling.
  The transcript flagship (slice 8) lifted it: a [`CommitLine`] now holds a
  `Vec<Span>`, so a committed markdown line can carry a bold heading, dim inline
  code, and plain prose at once. The single-span constructors (`CommitLine::new`,
  `From<&str>`, `From<String>`) and the `text`/`style` accessors are preserved
  for the common one-style case, so most call sites are unchanged.

[`Update`]: https://docs.rs/rabbitui/latest/rabbitui/app/struct.Update.html
[`Update::event`]: https://docs.rs/rabbitui/latest/rabbitui/app/struct.Update.html
[`CommitLine`]: https://docs.rs/rabbitui-core/latest/rabbitui_core/commit/struct.CommitLine.html
