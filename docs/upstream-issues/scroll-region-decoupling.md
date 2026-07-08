# Report scroll state without capturing the mouse

> **DRAFT — pending author review. Not filed. See `README.md` in this directory.**

_Proposed title:_ Scroll awareness and native scrolling are mutually exclusive — a mode to report
scroll state without capturing the mouse.

## Summary

An application that wants to know the scrollback position — whether the user is pinned to the bottom,
and to be notified when they scroll — has only one way to find out today: enable mouse tracking. But
enabling mouse tracking makes the terminal stop scrolling the pane natively and instead forward wheel
events to the application. So the moment an application gains scroll _awareness_, it loses the native
scroll _behaviour_ (scrollback, wheel, selection) it was trying to cooperate with. These two things
should be separable.

## Minimal reproduction

A minimal program that just wants to display a status hint ("↓ jump to latest") when the user has
scrolled up:

1. To detect scrolling at all, it enables mouse tracking (e.g. `CSI ? 1000 h` plus `CSI ? 1006 h`
   for SGR encoding).
2. Run any workload that produces more than one screen of output beneath it.
3. Scroll up with the wheel or trackpad.

## Expected

The application learns the user scrolled (so it can show the hint), **and** the terminal still
scrolls its own scrollback natively — the wheel moves the viewport through history, text selection
works, copy works — exactly as it would with no mouse mode enabled.

## Actual

With mouse tracking on, the wheel no longer scrolls the pane. The terminal forwards wheel events to
the application, which must now reimplement scrolling itself. Worse, the forwarded events carry no
magnitude and no device type: terminals emit anywhere from one to nine or more events per physical
notch, and the count differs per terminal, so an application cannot even normalize them without a
per-terminal table. Turning mouse tracking off restores native scroll but removes all scroll
awareness. There is no configuration that yields both.

## Why the current mechanisms do not cover this

- Alternate-scroll mode (`CSI ? 1007 h`) translates the wheel into arrow keys. That is lossy (no
  position, no magnitude) and diverges across operating systems.
- The terminal already receives the OS-level high-resolution scroll delta and discards it before
  forwarding, so the magnitude information exists but is thrown away
  ([wezterm#7645](https://github.com/wezterm/wezterm/issues/7645):
  "discards the magnitude of the delta and forwards only a single tick";
  [ghostty discussion#4259](https://github.com/ghostty-org/ghostty/discussions/4259)).
- `SU`/`SD` (scroll up/down) semantics themselves differ between implementations
  ([microsoft/terminal#11078](https://github.com/microsoft/terminal/issues/11078)), so there is not
  even a portable way to move the viewport programmatically.

## Proposed direction

A DECRQM-gated mode — call it a _scroll-report_ mode — under which the terminal **keeps scrolling
natively** but additionally emits an in-band event on user scroll carrying `(offset, total, height,
source)`, plus a query for the current state. This decouples scroll _awareness_ from mouse _capture_,
which is the structural coupling in modes 1000/1006. Optionally, a companion extension to the SGR
mouse encoding could carry a signed high-resolution (v120-style) delta and a discrete/continuous
device bit for applications that do opt into full wheel handling — obsoleting the per-terminal
normalization tables — but the notification mode is the higher-leverage half and is independently
useful.

## Degradation and multiplexers

- **Degradation:** the mode answers "unsupported" via DECRQM and the application falls back to
  today's behaviour; nothing regresses.
- **Multiplexers:** in a large share of sessions the multiplexer, not the terminal, owns the
  scrollback, so this must be a multiplexer-native feature — the multiplexer answers scroll state for
  _its_ scrollback rather than passing the query through. A passthrough-only story does not work here
  because the multiplexer is the scrollback owner.
