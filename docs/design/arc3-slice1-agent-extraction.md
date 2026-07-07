# Arc 3 slice 1 — extraction, replay, persistence

The flagship's first slice: a new binary crate `rabbitui-agent` (`rabbit`) that lifts the slice-8
example's simulated chat onto a real backend contract, adds session persistence, and pins the whole
flow with headless tests — no network code. Plan: `docs/plans/arc3-agent-client.md`.

## What landed

- **`backend` module** — the `Backend` trait (`send(ChatRequest) -> EventStream`), the wire-neutral
  `StreamEvent` enum (TextDelta / ThinkingDelta / ToolUse{Start,InputDelta,Stop} / MessageDone),
  and `ChatRequest`/`ChatMessage`/`Role`/`StopReason`/`Usage`/`BackendError`. Errors ride _inside_
  the stream as `Err` items, so a mid-turn failure lands in the transcript in order with the prose
  before it.
- **`ReplayBackend`** — plays a flat JSONL fixture; turns are delimited by `MessageDone`, so a
  multi-request conversation replays across successive `send` calls. Exhaustion synthesizes an
  `EndTurn` so the app never hangs. `stream_turn` is the shared one-turn stream builder.
- **`DemoBackend`** — scripts a markdown response seeded from the prompt, so `cargo run --bin rabbit`
  works before slice 2's real wire. It is the default backend for this slice only.
- **`session` module** — JSONL persistence (a `SessionMeta` header line then one `ChatMessage` per
  line), `load`/`resume`/`append`/`latest`, resume seeds history and marks it already-persisted.
- **`app` module** — the ported chat UI (inline live tail + alt-screen scroll), plus the reducer.
- **CLI** — `--model`, `--continue`, `--resume`, `--replay`, `--help`.
- **Tests** — 10 total: reducer + transcript via `TestApp` (5), replay turn-segmentation (2),
  session round-trip (3). `cargo check/clippy/doc` clean workspace-wide.

## Decisions this slice forced

- **Pure reducer + a `committed` marker.** `TestApp` does not run the real `update` closure — it
  renders and routes input through the shared router. So the update logic is split: `apply_message`
  and `on_submit` only _mutate state_ (testable directly), and the `update` closure is the thin
  layer that turns their results into side effects (scrollback commits, effect spawns, persistence).
  Scrollback commits are decoupled from state by `Agent.committed: usize` — after the reducer runs,
  the closure commits any not-yet-committed cells in inline mode. Commits go to native scrollback,
  which `TestApp`'s buffer does not model, so tests render in **alt-screen** mode and assert the
  buffer. This split is the load-bearing testability decision and should carry through every later
  slice.
- **`EventStream` is `Send`, owned, `'static`.** The app spawns the backend stream as a `Cmd::stream`
  effect that outlives the `send` call, so `send` returns an owned stream (the replay backend clones
  its turn out; slice 2's HTTP backend boxes a `reqwest` stream) rather than borrowing the backend.
- **Tool events deferred to slice 4 (as planned), but the `StreamEvent` tool variants exist now** so
  slice 4 does not churn the enum or the transcript type. Slice 1's reducer reflects a running tool
  in the status line and treats every `stop_reason` as end-of-turn.
- **`update.widget::<TextInput>(path, |s| s.clear())`** replaces the example's `input_generation`
  re-keying to clear the composer — the newer, cleaner API the widget's own docs recommend.

## What this revealed

- **The Backend seam is a clean fit.** The example's hand-coded `AgentStream` was already a
  deterministic `Stream<Item = Msg>`; abstracting it behind `Backend` + `StreamEvent` was mechanical,
  which is the evidence the contract is right. Slice 2 should be "implement the same trait over SSE"
  with no app changes.
- **`Line` untagged serde is the right persistence shape** — a bare `SessionMeta` serializes
  identically to a header line, so the round-trip tests build fixtures from public types with no
  private-API leak, and resume is just "deserialize the messages."
- **Inline-mode resume has a visible gap** (noted, deferred): resumed cells are not re-emitted to
  scrollback (they were not in _this_ terminal session), so after `--continue` the inline tail is
  empty until you send a message or switch to alt-screen (Ctrl-T) to see prior history. Acceptable
  for slice 1; a "replay resumed transcript into scrollback on start" option is a slice-5 chrome
  polish item.
- **qwertty mouse-decode drift (external).** A `cargo test --workspace` surfaced five _pre-existing_
  failures in `rabbitui`'s `input::tests::sgr_mouse_*` — `from_qwertty` now returns `None` for SGR
  mouse sequences after qwertty's 2026-07-07 "Decode in-band resize / SIGWINCH" commit changed the
  event shape. This is substrate drift in code slice 1 never touched (the `rabbitui-agent` suite is
  green in isolation), filed in `work/qwertty/substrate-status.md`. Per the coordination rule we are
  **not** chasing a mid-flight qwertty; the mouse-mapping adaptation is queued with the
  KeyEvent/TextPayload pre-pin migration (Arc 4 item 8). It does not block landing slice 1.

## Next (slice 2)

`AnthropicBackend` implementing `Backend` over `reqwest` + a hand-rolled SSE parser (unit-tested on
recorded fixtures), auth resolution (`x-api-key` / OAuth bearer), error/`stop_reason` cells, and a
record mode that writes the same JSONL fixtures `ReplayBackend` reads. Load the claude-api skill
before writing the wire layer (session rule).
