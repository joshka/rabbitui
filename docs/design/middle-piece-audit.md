# Middle-piece audit

Arc 2.1 (`ROADMAP.md`). One honest gap analysis of rabbitui's shipped surface against what real
applications need — the "missing middle" the whole project exists to fill
(`docs/research/recent-rust-tui-wave.md`). This drives arcs 2.2–2.5; it is a read-only audit that
does not change code. Grades are deliberately harsh: this is the document that decides what ships
next, and flattery here costs later.

Date: 2026-07-07 · Scope: the v0.1 surface (`rabbitui-core`, `rabbitui`, `rabbitui-widgets`,
`rabbitui-testing`, `rabbitui-ratatui`) and all eight `rabbitui/examples/*.rs`, which are the honest
record of what an app must hand-roll today.

Grade key: **solid** = a real app would use it as-is; **partial** = present but forces a workaround
or covers less than the demand; **missing** = an app must hand-roll it or go without.

## Part 1 — Demand-list scorecard

The wave's ranked demand list (memo §"What people are asking for"), each scored against the shipped
surface, with the example code that proves the grade.

| #   | Demand                 | rabbitui's answer                                                                                                | Grade       | What is missing                                                                                                                                                                                                                                                                                                               |
| --- | ---------------------- | ---------------------------------------------------------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Event loop             | `app::run` / `App` builder own a `select!` loop with coalescing, resize-poll, panic-safe restore (`app.rs`)      | **solid**   | Resize is polled not pushed (substrate gap, `app.rs:80`); "bring your own loop" escape hatch is undocumented/unbuilt                                                                                                                                                                                                          |
| 2   | Focus                  | Framework-owned `Focus` keyed by `WidgetId`, traversal from facts, layer-scoped (`routing.rs`, `facts.rs:193`)   | **solid**   | Shift-Tab/BackTab undecoded by substrate (`focus.rs:11`); no focus-scope save/restore API beyond modal layers                                                                                                                                                                                                                 |
| 3   | Forms / text input     | `TextInput` (grapheme-correct), outcomes, widget-command clear (`text_input.rs`)                                 | **partial** | No multiline/textarea, no numeric/masked/validated field, no form/fieldset abstraction — every form hand-rolls validation + layout (`form.rs:55`, `:154`)                                                                                                                                                                     |
| 4   | Overlays / modals      | `Frame::layer` z-layer, layer-scoped hit-test + focus, declare-then-focus retry (`frame.rs:334`, `facts.rs:176`) | **partial** | No modal/dialog/popup/menu/tooltip _widget_ — `form.rs:237` hand-rolls the whole modal (layout, focus handshake `form.rs:116`, Esc dismiss `form.rs:111`); backdrop/outside-click/centering are app-land                                                                                                                      |
| 5   | Theming                | Semantic `Role` (9), `Theme` presets, TOML file + debug hot-reload (`theme.rs`, `rabbitui/src/theme.rs`)         | **partial** | Only `dark` + `catppuccin_mocha` ship; DESIGN.md promises Nord + Dracula and they do not exist; no capability degradation (truecolor→256→16 deferred, `style.rs:4`); no per-state role matrix (hover/pressed/disabled/invalid)                                                                                                |
| 6   | Widget catalog breadth | 5 widgets: `Text`, `Button`, `TextInput`, `SelectionList`, `Collapsible`                                         | **missing** | The catalog _is_ the product (prior-art law #4); prior-art calls for 20+. No spinner, checkbox, radio, tabs, table, gauge/progress, scrollview, list-with-headers, menu, key-hint bar, markdown/rich-text — `agent.rs` hand-rolls markdown→spans (`agent.rs:616`) and a spinner (`agent.rs:66`, `:913`)                       |
| 7   | MVU ergonomics         | State/update/view with typed `Outcome`s and `Update` sink; no runtime takeover of state (`app.rs`)               | **solid**   | No optional `rabbitui-tea` shell yet (deferred by design); outcome enum is closed (`outcome.rs:32`) so custom-widget signals ride `Activated`/`Changed` only                                                                                                                                                                  |
| 8   | Inline mode            | Peer inline/alt engines, append-once commit + bounded tail, runtime switch (`engine/inline.rs`, `app.rs:1149`)   | **solid**   | Whole-message commit only — a streaming answer taller than the tail scrolls its top away invisibly until it commits (slice-8 finding #5, `agent.rs` `TAIL_HEIGHT` at `:59`); no block-level early commit                                                                                                                      |
| 9   | Rendering correctness  | Cell buffer + double-buffer diff inside mode-2026 framing, vt100 harness (`buffer.rs:431`, `testing/src/vt.rs`)  | **solid**   | No damage beyond the diff (by design); no perf measurement (see Part 2); wide-grapheme half-scroll drops rather than clips (`text_input.rs:397`)                                                                                                                                                                              |
| 10  | Styling depth          | Typed `Style` (fg/bg + 6 attrs), `Color` (Reset/Ansi/Indexed/Rgb) (`style.rs`)                                   | **partial** | `Attributes` is a write-only bitset — no `remove`/`&`/`!`, so nested markdown reconstructs the complement by hand (slice-8 finding #1, `agent.rs:762`); no styled-span `Text` (widget `Text` wraps one `&str` in one style, `text.rs:54`) so styling _pops_ at commit (slice-8 findings #3/#4); no underline-color/blink/hyperlink |
| 11  | Async / reactive       | `Command` future/stream/timeout, groups (cancel-previous), `cancel_group`, contained panics (`effect.rs`)            | **solid**   | No "subscription from state" — two coupled streams need manual stop bookkeeping (slice-8 finding #6, `agent.rs` `ticking` flag `:153` + `stop_spinner` `:357`); completion order unspecified; tokio mandated (no sync path)                                                                                                   |
| 12  | Agent-legibility       | Small inferrable grammar, PTY-level `TestApp` + `VtScreen` agents can run (`testing/src/lib.rs`)                 | **partial** | No shipped/evaled agent skill (deferred to Arc 2.4); the flagship `agent.rs` hand-rolls markdown, spinner, transcript scroll, and cell measurement — the exact things an agent chrome needs from the framework                                                                                                                |

### One-line grades

- Event loop — **solid**. Focus — **solid**. Forms/text input — **partial**. Overlays — **partial**.
- Theming — **partial**. Widget catalog breadth — **missing**. MVU ergonomics — **solid**.
- Inline mode — **solid**. Rendering correctness — **solid**. Styling depth — **partial**.
- Async/reactive — **solid**. Agent-legibility — **partial**.

The architecture (loop, focus, inline, effects) grades _solid_; the product (catalog, styled text,
forms, overlays as widgets) grades _partial-to-missing_. That is exactly the wave's diagnosis: the
scarce good is not the model, it is the middle. The flagship `agent.rs` is the proof — 936 lines, of
which the framework contributes the loop and five primitives, and the app hand-rolls markdown
rendering, a spinner, a fixed-slot transcript with manual scroll, and a bitset-complement helper.

## Part 2 — Cross-cutting concerns inventory

Concerns no slice owned, graded the same way. These are the silent killers: each is invisible in a
demo and unavoidable in a real maintained app.

| Concern                              | rabbitui's answer today                                                                                                                                               | Grade                                  | What is missing / the honest cost                                                                                                                                                                                                                                                                                                                              |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Error handling in update/view        | `update` returns `ControlFlow<()>` only — no `Result`; a panic in `view` unwinds the loop but the panic hook + `Drop` restore the terminal (`terminal.rs:93`, `:225`) | **partial**                            | No fallible update/view path — an app that wants to quit-on-error hand-codes it into state; a `view` panic still tears the process down (restored, but dead). Effect panics _are_ contained as `Event::EffectFailed` (`effect.rs`), the one bright spot                                                                                                        |
| Logging / tracing                    | None. No `tracing`/`log` dependency anywhere (grep: zero hits)                                                                                                        | **missing**                            | A TUI owns the screen, so `println!`/`eprintln!` corrupt it — an app _must_ redirect logs, and the framework offers no writer, no `tracing` layer, no in-app log pane, no "log to file while inline" seam. `EffectError` carries text (`effect.rs:60`) but there is nowhere to send it                                                                         |
| Suspend / resume (Ctrl-Z, `$EDITOR`) | None. No SIGTSTP/SIGCONT handling, no `suspend()`/`with_raw_disabled()` (grep: zero hits)                                                                             | **missing**                            | Substrate-owned in part (qwertty owns raw mode), but _our_ surface has no "give the terminal back, run a subprocess, take it back" primitive. Shelling to `$EDITOR` or a pager — table stakes for a coding-agent chrome — is impossible without dropping to raw bytes. Filed as a substrate requirement, but the framework seam is also absent                 |
| Config / keybinding customization    | None. Keys are matched inline in each app's `update` (`focus.rs:48`, `todo.rs:89`)                                                                                    | **missing**                            | No keymap, no action-binding layer, no config file. Every app hard-codes `Key::Char('q')`; the wave (weavetui's `kb!` DSL, matetui) shows this is repeatedly rebuilt. Theming has a file loader; bindings have nothing analogous                                                                                                                               |
| Performance / benchmarks             | None. No `benches/`, no criterion, no measurement (grep: zero)                                                                                                        | **missing**                            | DESIGN.md's core claim — "full re-render is microseconds, incrementality buys nothing" — is _unmeasured_. The wave ranks perf #6 ("8% CPU while typing"). Nothing records frame time, diff cost, or the 800×-DataTable class of gap. This is a load-bearing assumption with no evidence behind it                                                              |
| Accessibility posture                | Tracked, not shipped (DESIGN.md non-goals). Facts carry `id`/`area`/`focusable`/`layer` (`facts.rs:54`) but no role, label, value, or state                           | **partial**                            | The identity + facts substrate is the _right_ shape for an AccessKit export (stable ids exist), but the data an export needs is not recorded: a `FactEntry` has no semantic role (button vs. text), no accessible name, no value/checked/expanded state. `Outcome`/`Role` exist but are not attached to facts. An export today would emit unlabeled rectangles |
| i18n / RTL                           | None, and correctly so                                                                                                                                                | **partial** (out of scope, but say it) | Explicitly out of scope for v0.1 — no bidi, no RTL, no locale. Width/grapheme handling _is_ present (`text_input.rs` CJK/emoji tests), which is the one i18n primitive that matters for correctness. Name it a non-goal in docs so scope pressure has somewhere to go                                                                                          |
| Copy / paste / selection             | Inline mode _defers_ to the terminal's native selection (the whole point of not capturing mouse inline, `app.rs:598`); alt-screen has none                            | **partial**                            | The deliberate tradeoff (ADR 0013): inline keeps native copy by _not_ owning the viewport; alt-screen, which captures the mouse, gives the app no selection model at all. No in-app selection, no clipboard (OSC 52) write. An alt-screen reader app cannot let the user select text                                                                           |
| Terminal title / notifications       | None. No OSC title, no bell, no OSC 9/777 notify (grep: zero)                                                                                                         | **missing**                            | A background agent finishing a task cannot set the title or ring the bell — both one-line OSC writes the encoder could carry. Small surface, real demand for the agent-chrome workload                                                                                                                                                                         |
| Multi-key chords / keymaps           | None. Single `KeyEvent` per event; modifiers present (`input.rs:314`) but no chord/sequence buffer                                                                    | **missing**                            | No `g g`-style sequences, no leader keys, no which-key. An app wanting Vim-style bindings buffers keys itself. Ties to the config/keybinding gap above                                                                                                                                                                                                         |
| Animation / timers ergonomics        | `Command::timeout` + `Command::stream` over a hand-rolled interval; that is the whole toolkit (`effect.rs:176`, `fetch.rs:216`)                                               | **partial**                            | Every ticker is a hand-written `Stream` impl (`agent.rs:913` SpinnerTicker, `fetch.rs:216` Ticker) — ~15 lines each, repeated. No `Command::interval`, no easing/tween, no frame-clock subscription. tachyonfx (123k dl) shows effects demand; nothing here serves it                                                                                              |
| State persistence across runs        | None, and app-owned by design                                                                                                                                         | **partial** (arguably out of scope)    | State is plain app-owned Rust (ADR 0001), so persistence is the app's job and that is defensible — but there is no session/scroll-offset restore helper, and the retained store (`store.rs`) is deliberately ephemeral (dropped after absent frames), so "reopen where you left off" is entirely hand-rolled                                                   |

## Part 3 — Prioritized gap list

Every gap from Parts 1–2, ranked by (impact on the Arc 2.3 flagship app) × (inverse cost). The
flagship candidates are all interaction-heavy, streaming, scroll-heavy apps (agent chrome, jj
viewer, tape UI), so gaps that block _a scrolling column of variable-height content_ dominate the
top. The measurement / ScrollView / styled-span trio is already the known #1–3 (ROADMAP Arc 2.2);
the rest are placed honestly around them.

| Rank | Gap                                                                                   | Impact                                                                                                                                     | Cost | Arc     |
| ---- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ---- | ------- |
| 1    | `desired_height(width)` intrinsic measurement                                         | Blocks every variable-height layout; the flagship is impossible without it (slice-8 finding #2, `agent.rs:486` fixed 4-row slots)          | Med  | 2.2     |
| 2    | Real `ScrollView` / `Column` container                                                | Transcript, diff, tool-log all need smooth row-scroll over measured children; today it is `skip(offset)` over whole cells (`agent.rs:498`) | Med  | 2.2     |
| 3    | Styled-span `Text` (render `Vec<Span>`, wrap on spans)                                | Kills the styling-pop at commit and the two-wrap-oracle split (slice-8 findings #3/#4); unblocks a markdown widget                         | Med  | 2.2     |
| 4    | Widget catalog breadth (spinner, tabs, checkbox, table, progress, menu, key-hint bar) | The catalog _is_ the product (prior-art law #4); flagship hand-rolls spinner + more                                                        | High | 2.3     |
| 5    | Markdown / rich-text widget                                                           | The agent chrome's centerpiece; `agent.rs:616` proves 140 lines of it are app-land today                                                   | High | 2.3     |
| 6    | `Attributes::remove` / `BitAnd` / `Not`                                                    | Any nested-styling consumer reconstructs the complement by hand (slice-8 finding #1, `agent.rs:762`); additive, low-risk                   | Low  | 2.2     |
| 7    | Modal / dialog / menu _widget_ (over the `layer` primitive)                           | Overlays exist as a primitive; every app rebuilds the widget (`form.rs:237`)                                                               | Med  | 2.3     |
| 8    | Logging / tracing seam (a TUI cannot print to stderr)                                 | Silent blocker for any real app; a `tracing` writer or log-pane seam. Genuinely no answer today                                            | Med  | 2.2/2.5 |

Ranks 9+ (below the Arc-2.2+ cut, tracked for later arcs): block-level early commit for bounded
tails (#5 slice-8); Nord + Dracula presets to match DESIGN.md's promise; a `Command::interval` /
subscription-from-state to end ticker hand-rolls; capability degradation (truecolor→256→16); a
config/keybinding layer + multi-key chords; suspend/`$EDITOR` handoff; performance benchmarks
(unmeasured core claim); accessibility fact enrichment (roles/labels/values on `FactEntry`);
terminal-title/OSC-notify + OSC-52 clipboard; a fallible update/view (`Result`) path; the optional
`rabbitui-tea` shell and shipped agent skill (already Arc 2.4).

## Findings that pressure the Arc 2 ordering

The audit mostly _confirms_ ROADMAP's Arc 2 ordering — 2.2's scroll/measurement/styled-span trio is
correctly #1–3, and it correctly gates 2.3. Three tensions worth surfacing to the author:

1. **The logging/tracing gap (Part 2) is unscheduled and arguably belongs in 2.2, not "later."** A
   TUI that cannot log without corrupting its own screen is unusable for real development — the
   moment the flagship (2.3) hits a bug, its author has nowhere to send a trace. It is cheap (a
   writer + optional `tracing` layer) and unblocks the flagship's own construction. ROADMAP lists
   "tracing" only as an audit _topic_, never as a 2.2 deliverable. Recommend pulling it forward.

2. **The unmeasured performance claim undercuts a load-bearing DESIGN.md decision.** The whole
   rejection of the Xilem view/element split rests on "full re-render is microseconds,
   incrementality buys nothing" (DESIGN.md, prior-art.md). There is not one benchmark. If the
   flagship's scrolling transcript ever shows frame cost, the architecture argument reopens
   (prior-art itself flags this as the trigger to promote view-diff). A benchmark harness is cheap
   and should precede, not follow, the flagship — otherwise 2.3 could invalidate a core ADR with no
   way to see it coming.

3. **DESIGN.md over-promises theming presets that do not ship.** DESIGN.md states "Catppuccin / Nord
   / Dracula presets ship in v0.1"; only `dark` and `catppuccin_mocha` exist (`theme.rs`). This is a
   documentation-vs-surface drift, not an ordering error, but it is exactly the "pretty by default"
   top-5 want (wave #7) shipping at one-third of its stated scope. Either ship the two presets (low
   cost) or correct DESIGN.md before 0.1.

Nothing in the audit contradicts the _sequencing_ of 2.2 → 2.3 → 2.4 → 2.5; the corrections are (a)
promote tracing into 2.2, (b) add a benchmark harness before the flagship, and (c) reconcile the
theming-preset promise. The trio is right; the cross-cutting concerns are where the roadmap is
quietest and the risk is highest.
