# Bubble Tea (Go / Charm) — Research Memo

**Verdict:** Bubble Tea proves that a single-event-loop Elm architecture with async commands is the most *teachable* TUI programming model in existence — and also proves, via six years of identical user complaints, that pure TEA without a component/identity layer collapses under composition; v2 quietly concedes this by moving rendering, layers, and terminal state out of the model and into a cell-based runtime.

Date: 2026-07-06

**Sources** (all fetched 2026-07-06):

- README / repo: <https://github.com/charmbracelet/bubbletea> (43.6k stars, v2.0.8 current, 18k+ dependents)
- v2 announcement: <https://charm.land/blog/v2/>
- "What's New in v2": <https://github.com/charmbracelet/bubbletea/discussions/1374>
- v2 upgrade guide: <https://github.com/charmbracelet/bubbletea/blob/main/UPGRADE_GUIDE_V2.md>
- v1 renderer source: <https://raw.githubusercontent.com/charmbracelet/bubbletea/v1.3.4/standard_renderer.go>
- Commands blog: <https://charm.land/blog/commands-in-bubbletea/>
- Subscription removal commit `ade8203c` ("Remove entire subscription model"), found via `gh search commits --repo charmbracelet/bubbletea "subscription"`
- Discussions: [#176 nested components](https://github.com/charmbracelet/bubbletea/discussions/176), [#751 multi-model message routing](https://github.com/charmbracelet/bubbletea/discussions/751), [#707 state duplication / sibling communication](https://github.com/charmbracelet/bubbletea/discussions/707), [#704 Init/Update signatures](https://github.com/charmbracelet/bubbletea/discussions/704)
- leg100, "Tips for building Bubble Tea programs": <https://leg100.github.io/en/posts/building-bubbletea-programs/> + HN thread <https://news.ycombinator.com/item?id=41369065> (comments via <https://hn.algolia.com/api/v1/items/41369065>)
- Issues: [#32 refresh on slow networks](https://github.com/charmbracelet/bubbletea/issues/32), [#1019 Windows flicker ≥0.26.0](https://github.com/charmbracelet/bubbletea/issues/1019)
- Focus handling: bubbles textinput `Focus()/Blur()/Focused()` API via <https://pkg.go.dev/github.com/charmbracelet/bubbles/textinput>, official `focusIndex` traversal pattern in <https://github.com/charmbracelet/bubbletea/blob/main/examples/textinputs/main.go>
- Ecosystem: <https://github.com/charmbracelet/bubbles>, <https://github.com/charmbracelet/lipgloss>, <https://github.com/charmbracelet/huh>, teatest API via <https://pkg.go.dev/github.com/charmbracelet/x/exp/teatest>, lipgloss v2 compositor via <https://pkg.go.dev/charm.land/lipgloss/v2>

## Core architecture

**The loop.** One interface, three methods: `Init() Cmd`, `Update(Msg) (Model, Cmd)`, `View()`. All input, resize, timer, and I/O results arrive as `tea.Msg` values through a single serialized `Update`; `View` is a pure projection of the model. The runtime literally replaces your whole model with the one returned from `Update` (discussion #704).

**Side effects: commands only, no subscriptions.** `tea.Cmd` is just `func() tea.Msg` run on a goroutine by the runtime; the returned message is fed back into `Update`. Bubble Tea *had* Elm-style subscriptions (`tea.Sub`, `tea.Every`) in early 2020 and deleted them — commit `ade8203c`: "we can achieve the same functionality in a much simpler fashion with commands, especially because Go is not held to the same restrictions as Elm." Recurring work is a command that re-issues itself (`tea.Tick`); long-lived streams are a command blocking on a channel (issue #1135 asks for `tea.Listen` sugar). Charm's doctrine ("Commands in Bubble Tea" blog): use commands for *all* I/O, *only* for I/O, and "never use goroutines within a Bubble Tea program."

**Rendering, v1: string diffing at ~60fps.** `View()` returns one big string. The standard renderer buffers it and flushes on a ticker (`defaultFPS = 60`, `maxFPS = 120`, standard_renderer.go:16), splits on `\n`, and skips lines identical to `lastRenderedLines[i]` (standard_renderer.go:~174). So the diff granularity is a *whole line*, the identity granularity is *the whole frame string*, and correctness depends on ANSI-aware width math done in userland (lipgloss). A bypassed "high-performance renderer" escape hatch existed and is deprecated as "philosophically different" (standard_renderer.go:~315).

**Rendering, v2 (Feb 2026, first breaking change in 6 years): cell buffer.** The "Cursed Renderer" is "based on the ncurses rendering algorithm" — a real cell grid with damage-based updates, order-of-magnitude gains claimed especially over SSH (charm.land/blog/v2, discussion #1374). `View()` now returns a `tea.View` struct: `Content` plus declarative terminal state — `Cursor` (position/shape/blink), `AltScreen`, `MouseMode`, `KeyboardEnhancements`, `WindowTitle`, `BackgroundColor`, `ProgressBar` (upgrade guide). Imperative toggle commands (`tea.EnterAltScreen` etc.) are gone; the view is the single source of truth. Synchronized output (mode 2026) and grapheme handling (mode 2027) are on by default. Input got fidelity: `KeyMsg` split into `KeyPressMsg`/`KeyReleaseMsg` with `Code`/`Text`/`Mod`, progressive keyboard enhancement (kitty-style shift+enter etc.), split mouse messages, OSC52 clipboard, terminal capability queries.

**Ecosystem.** `bubbles`: 13 stock components (textinput, textarea, list, table, viewport, spinner, progress, paginator, filepicker, timer, stopwatch, help, key) — each is itself a mini-TEA model you embed and forward messages to. `lipgloss`: immutable chained `Style` values (`NewStyle().Bold(true).Padding(...)`), `JoinHorizontal/JoinVertical/Place` for layout-by-string-concatenation, `Width/Height` ANSI-aware measurement; v2 adds a real compositor — `Layer` with `X/Y/Z()`, flattened and z-sorted by a `Compositor` that also does mouse **hit testing** (pkg.go.dev charm.land/lipgloss/v2). `huh`: declarative Form→Group→Field builder with validation, `*Func` dynamic fields, an accessibility mode that swaps the TUI for plain prompts, and dual-mode execution (blocking `form.Run()` or embedded, since "a huh.Form is just a tea.Model").

## What it gets right

- **One mental model, brutally small API.** Three methods and `Cmd = func() Msg`. This is why it has 18k+ dependents and why HN users call it "great and fun to work with" (Instantnoodl, HN 41369065). The tutorial-to-working-app distance is the shortest of any TUI framework surveyed.
- **Serialized `Update` kills data races by construction** — as long as you obey the rules. All async results re-enter through the loop as messages. klabb3 on HN: mutating the model outside the loop is "a proper race condition"; the framework's answer is "don't — return a Cmd."
- **Deleting subscriptions was correct.** Commands-that-reissue-themselves and channel-pumping commands cover Elm's `Sub` use cases with one concept instead of two (commit `ade8203c`). Less machinery, same power.
- **v2's declarative `tea.View`** ends the "who owns terminal state" fight: cursor, alt-screen, mouse mode as *data returned from render* rather than imperative side effects. This composes and is trivially testable.
- **Styles as values.** lipgloss `Style` is immutable and copied on modification — themes are plain Go values you pass around, no cascade engine, no globals (v2 also removed implicit global color-profile detection).
- **huh's accessibility mode** — degrade the whole form to sequential stdin prompts for screen readers — is a genuinely novel feature no Rust TUI has.
- **teatest** gives a real headless driver: `NewTestModel(tb, m, WithInitialTermSize(x,y))`, `Send`/`Type`, `WaitFor(output-condition)`, `FinalModel`, and `RequireEqualOutput` golden files with `-update` (pkg.go.dev). Plus VHS for GIF-based visual regression.

## What users complain about

- **Composition is the #1 complaint, unresolved for 5+ years.** Nested models mean the parent hand-routes every message to every child and reassembles returned models (#176, maintainer: nest sub-models, forward messages "sequentially"). #751: no way to know which child a reply `Msg` belongs to; users invent `Wrap()`/integer-tag schemes; `sequenceMsg` being private blocks custom routers. #707 (state shared between siblings: duplicate it and risk desync, or add getter plumbing) got **zero replies**. leg100's post formalizes the workaround — a "tree of models" where the root is "a message router and screen compositor" — i.e., every serious app rebuilds a component runtime by hand.
- **No focus system.** The framework has no notion of which component owns keyboard input: bubbles expose per-component `Focus() tea.Cmd` / `Blur()` / `Focused()` (pkg.go.dev bubbles/textinput) and every app hand-rolls traversal on top. The official `textinputs` example is the canonical pattern — a `focusIndex int` the parent increments on tab/shift+tab, then loops over children calling `Focus()` on the match and `Blur()` on the rest. The maintainer's #176 answer sidesteps it entirely (all sub-models receive every message; only one was created focused), so focus routing is just another instance of the hand-written message-routing problem above.
- **Boilerplate + type assertions.** `Update` returns the `tea.Model` interface, so parents downcast children after every update; single-`Cmd` returns force `tea.Batch` everywhere (#704 — maintainer defends it as Elm-style composability; v2 changed neither).
- **No layout system.** GeertJohan (HN): layouting is "very difficult" because it's "not part of the framework"; leg100: manual width/height arithmetic is "error-prone" — add a border, break the layout. Everything is lipgloss string-joining plus hand-propagated `WindowSizeMsg`.
- **Simple things aren't simple.** georgemcbay (HN) ditched it for tview: the "message-based architecture" demands full buy-in for a two-widget tool. sweeter: gave up because complex apps got "hard to maintain." The Elm loop has a step-function learning curve at the exact point apps grow a second screen.
- **v1 rendering artifacts.** Whole-area re-render blink on slow links (#32, from 2020), Windows flicker regression ≥0.26.0 (#1019), line-diff renderer losing rows on resize (#1039). These are what the v2 cell renderer + mode 2026 exist to fix.
- **Loop stalls and message ordering.** Slow `Update`/`View` back up the queue; commands run on goroutines so completion order is unspecified; a panic inside a command skips terminal restore, leaving users running `reset` (leg100).

## What's worth stealing

- **`Cmd = async fn() -> Msg` as the *only* effect primitive**, plus the discipline trio (all I/O in commands, only I/O in commands, no bare task spawns touching state). Maps perfectly onto Rust: `Cmd = impl Future<Output = Msg>` spawned by the runtime, result injected into the loop.
- **The subscription deletion.** Don't build a `Sub` abstraction; a stream is a command that yields messages (rabbitui: `Cmd::stream(impl Stream<Item = Msg>)`), a timer is a command that re-arms.
- **v2 `tea.View` struct**: render output = content **+ declarative terminal state** (cursor pos/shape, title, mouse mode, keyboard enhancement level). Steal the shape, not the string.
- **lipgloss v2's Layer/Compositor**: layers with x/y/z, flattened once, z-sorted, **hit-testing derived from the same structure that painted** — that's the correct way to route mouse events, and qwertty's compositor should own it.
- **teatest's exact API surface** (test model, `Send`/`Type`, `WaitFor` on rendered output, golden files with `-update`) — proven ergonomics, near-zero design risk to copy.
- **huh's Form/Group/Field declarative builder + accessibility fallback** as the model for a high-level forms layer above the widget set.
- **Style-as-value theming** (immutable, chainable, explicitly passed) as the base layer even if a Textual-CSS cascade sits above it.
- **Keyboard enhancement negotiation as render-time data** (v2 `KeyboardEnhancements` on the view; `KeyPress`/`KeyRelease` split with `Code`/`Text`/`Mod`) — cleaner than global kitty-protocol toggles.

## Implications for rabbitui

- **Keep the Elm loop as the *runtime contract* (single serialized update, effects re-enter as messages) but do not make hand-written `Update` trees the *user-facing component model*.** Six years of #176/#751/#707 show message routing to children is where TEA dies; Xilem-style view diffing or a component trait with framework-owned routing must sit on top, with the loop underneath for determinism.
- **Solve widget identity in the framework, not userland.** Bubble Tea has no answer to "which child does this async reply belong to" (#751's hand-rolled `Wrap()` tags). Give every component a stable ID/path assigned by the framework and address `Msg` delivery and focus by that ID — this is exactly Xilem's id-path insight, and it's the single biggest gap to close. Focus traversal in particular should be framework-owned (tab-order over those same IDs, with per-widget focusable flags), not a hand-maintained `focusIndex` loop calling `Focus()`/`Blur()` on every child as in Bubble Tea's own textinputs example.
- **Effects: adopt `Cmd = Future<Msg>` + `Stream<Msg>`, executor-owned, on qwertty's async event source.** Also steal the failure lesson: catch panics in effect tasks and restore the terminal (leg100's `reset` complaint), and document ordering (command completion is unordered — provide `sequence` explicitly like `tea.Sequence`).
- **Render to a cell buffer with damage tracking from day one; never strings.** Bubble Tea spent six years on line-granularity string diffing and the bug tail to match (#32, #1019, #1039) before rebuilding on an ncurses-style cell grid in v2. Ratatui-style double buffer + diff is already the v2 endpoint; add lipgloss-v2-style z-ordered layers with hit testing in the same structure.
- **Ship a real layout engine (taffy or constraint-based) as a core crate, not an afterthought** — "layout is not part of the framework" is the top-cited HN reason people abandon Bubble Tea, and `WindowSizeMsg` hand-propagation is its worst boilerplate. Size should flow through the framework's layout pass, never through user-routed messages.
- **Make view output a `Frame`/`View` struct carrying declarative terminal state** (cursor, title, mouse/keyboard modes, bg color), reconciled by the runtime against qwertty — v2 proved this beats imperative escape-command soup and it makes headless testing exact.
- **Copy teatest wholesale for the testing crate**: headless `TestApp::new(model, size)`, `send`/`type_text`, `wait_for(|buf| ...)`, buffer golden-snapshots with an update flag; pair with a VHS-like recorder later.
- **Have a "two-widget tool" story.** georgemcbay's defection to tview is the warning: provide a blocking one-shot layer (huh-equivalent forms/prompts built on the same widgets) so trivial apps don't pay the full architecture tax.
