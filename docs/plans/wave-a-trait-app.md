# Wave A — `trait App` core model (full implementation spec)

Lane claimed: wave-a workspace, 2026-07-11.

Corrections (2026-07-11, execution):

- Step 8 assumed `README.md` had a front-page code snippet to update; it had none. Added a
  "What it looks like" section with the `impl App` counter instead.
- The pre-existing `cargo doc` warnings in `rabbitui/src/log.rs` (links to the old `App`
  builder methods) were fixed as part of Step 5's rewiring; `cargo doc -p rabbitui` is now
  warning-free.

Written 2026-07-11 on Fable, after the core-model research pass
(`docs/design/core-model-and-roadmap.md` §1 carries the rationale and the resolved design
decisions). **The design is adjudicated — execute it; don't re-litigate.** If reality
contradicts a step, write the correction here (dated, with evidence) and proceed.

Read `docs/plans/00-execution-playbook.md` first (cwd resets, jj discipline, verification
gates). Everything below happens in `/Users/joshka/local/rabbitui/work/default`.

## Goal

Replace the two-closure top-level shape (`App::new(state, update, view)` + builder soup)
with an idiomatic, extensible `trait App` — keeping a zero-ceremony closure adapter
(`from_fn`). Packaging only: the declared-frame contract (identity, facts, outcomes,
`Command`-effects) is untouched. Nothing is published, so no deprecation dance.

## Files you will touch

- `rabbitui/src/effect.rs` — add `Command::none()`.
- `rabbitui/src/app.rs` — the trait, `Config`, `FnApp`, `from_fn`, loop rewiring. The bulk.
- `rabbitui/src/lib.rs` — re-exports.
- Call-site migration: `rabbitui/examples/*.rs` (9 files), `rabbitui/tests/e2e_headless.rs`,
  `rabbitui/tests/todo_flow.rs`, `rabbitui/tests/agent_flow.rs`, `rabbitui/tests/set_theme.rs`,
  `rabbitui-agent/src/app.rs` (+ its `main.rs` wiring), `comparisons/rabbitui/src/main.rs`.
- Docs: `rabbitui/src/app.rs` module doc, `README.md` snippet, `skills/rabbitui/SKILL.md`
  (it teaches `App::new` — must be updated or the agent skill goes stale),
  `docs/adr/0001-programming-model.md` amendment (§ Step 8).

## Step 1 — `Command::none()` (green step, commit alone)

In `rabbitui/src/effect.rs`: `Command<M>` wraps a private `enum Kind<M>` (line ~104). Add a
`None` variant, a constructor, and a spawn no-op:

```rust
pub fn none() -> Self { Self { kind: Kind::None } }   // adapt to actual field name
```

- In `Effects::spawn`, early-return on `Kind::None` (spawn nothing, no mailbox entry).
- If `.group(name)` is called on a `None` cmd, keep it `None` (group of nothing is nothing).
- Unit tests (in effect.rs `mod tests`): `spawn(Command::none())` leaves `group_count` at 0 and
  `try_recv` empty; `Command::none().group("x")` also spawns nothing.
- Gate: `cargo test -p rabbitui`, clippy, fmt. Commit:
  `feat(effect): Command::none() — the no-op command (trait App init hook default)`.

## Step 2 — `Config` struct

In `rabbitui/src/app.rs` (near the current builder methods):

```rust
/// Startup configuration, read once by the runtime before the loop starts.
/// Runtime switching stays on `Update` (`set_mode`/`set_theme`) — this is launch state.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Config {
    pub theme: Theme,
    pub theme_file: Option<PathBuf>,
    pub mode: Mode,
    pub mouse: Option<bool>,          // None = by-mode default (alt on, inline off)
    #[cfg(feature = "tracing")]
    pub tracing: Option<bool>,        // None = by-profile default (debug on)
    #[cfg(feature = "tracing")]
    pub log_handle: Option<rabbitui_core::log::LogHandle>,
}
```

Builder methods (because `#[non_exhaustive]` blocks literal construction downstream):
`new()`, `theme(Theme)`, `theme_file(impl Into<PathBuf>)`, `mode(Mode)`, `mouse(bool)`,
and cfg-gated `tracing(bool)`, `log_handle(LogHandle)` — each `#[must_use]`, `mut self`,
returns `Self`. Port the doc comments from the existing `App` builder methods verbatim
(they carry hard-won caveats). Export `Config` from `app` and re-export in `lib.rs`.

## Step 3 — the trait

In `rabbitui/src/app.rs`, replacing the old struct's role (struct handled in Step 5):

```rust
pub trait App<M = ()>: Sized
where
    M: Send + 'static,
{
    fn update(&mut self, update: Update<'_, M>) -> ControlFlow<()>;
    fn view(&self, frame: &mut Frame<'_>);

    fn init(&mut self) -> Command<M> { Command::none() }
    fn global(&mut self, _update: &Update<'_, M>) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn config(&self) -> Config { Config::default() }

    async fn run(self) -> Result<()> {
        let terminal = Terminal::open().await?;
        run_on(self, terminal).await
    }
    async fn run_over_device<D: qwertty::TerminalDevice>(self, device: D) -> Result<()> {
        let terminal = Terminal::from_device(device)?;
        run_on(self, terminal).await
    }
}
```

Doc-comment every method; the rationale lives in `core-model-and-roadmap.md` §1 — link it.
Document: not dyn-compatible (AFIT), deliberate; `M` is a generic param (assoc-type
defaults are unstable); a type implementing two `App<M>`s is legal but needs annotation.

## Step 4 — rewire the loop internals

Current private shape: `App::run` → `run_on(self, terminal)` → `run_loop(terminal, state,
update, view, watcher, mode, mouse, flush_handle)`. New shape: free
`async fn run_on<M, A: App<M>, D: qwertty::TerminalDevice>(app: A, terminal: Terminal<D>)`
which reads `let config = app.config();` once, installs tracing from
`config.tracing`/`config.log_handle`, builds the `ThemeWatcher` from
`config.theme_file`/`config.theme`, then calls
`run_loop(terminal, app, watcher, config.mode, config.mouse, flush_handle)`.

Inside `run_loop` (now generic over `A: App<M>` instead of `S, U, V`):

1. **draw sites** (2: initial frame + repaint): `draw` currently takes `state: &S, view: &V`.
   Change `draw` to take `view: impl FnOnce(&mut Frame<'_>)` and call it as
   `draw(&mut back, &mut store, focus, &theme, |f| app.view(f))`. Update `draw`'s other
   callers (grep `draw(` — there is at least one doc/test usage).
2. **update sites** (4: `Wake::Started`, resize, input, and `deliver_effect`): replace
   `update(&mut state, ctx)` with the global-then-update sequence:

   ```rust
   broke = if app.global(&ctx).is_break() {
       true
   } else {
       app.update(ctx).is_break()
   };
   ```

   Construct `ctx` once; lend `&ctx` to `global`, then move `ctx` into `update`. All
   `Update` methods take `&self` (RefCell pending), so `global` can spawn/commit/focus.
   On `global` Break, `update` is _not_ called; pending still drains (same code path).
3. **init**: in the `Wake::Started` arm, _before_ constructing the Started `Update`:
   `effects.spawn(app.init());` (a `Command::none()` default is a no-op by Step 1). Then
   deliver `Event::Started` exactly as today — both idioms coexist by design (§1 №4).
4. **`deliver_effect`** takes `app: &mut A` instead of `state`+`update`; apply the same
   global-then-update sequence there.

No other loop logic changes. This step must be behavior-preserving for closure apps.

## Step 5 — `FnApp` + `from_fn`, delete the old struct surface

Rename `struct App<S,U,V,M>` → `FnApp<S,U,V,M>`. Replace its config fields with one
`config: Config`. Its builders become `with_theme`, `with_theme_file`, `with_mode`,
`with_mouse`, `with_tracing`, `with_log_handle` (delegating to the `Config` builders) —
the `with_` prefix avoids shadowing trait method names. Delete its inherent
`new`/`run`/`run_over_device`/`run_on` (the trait provides run entries). Add:

```rust
pub fn from_fn<S, U, V, M>(state: S, update: U, view: V) -> FnApp<S, U, V, M>
where
    U: FnMut(&mut S, Update<'_, M>) -> ControlFlow<()>,
    V: Fn(&S, &mut Frame<'_>),
    M: Send + 'static,
{ /* … */ }

impl<S, U, V, M> App<M> for FnApp<S, U, V, M>
where /* same bounds */
{
    fn update(&mut self, update: Update<'_, M>) -> ControlFlow<()> { (self.update)(&mut self.state, update) }
    fn view(&self, frame: &mut Frame<'_>) { (self.view)(&self.state, frame) }
    fn config(&self) -> Config { self.config.clone() }
}
```

Free `run(state, update, view)` (app.rs:679) becomes `from_fn(state, update, view).run()`.
`lib.rs`: re-export `App` (now the trait), `FnApp`, `from_fn`, `Config`.

Gate here: `cargo check -p rabbitui` compiles the facade; callers are still broken — that
is expected; proceed straight to Step 6 before committing.

## Step 6 — migrate the 15 call sites

Mechanical per site: either wrap with `from_fn(state, update, view)` (one-line change), or
convert to `impl App` (move the two fns into the impl; state struct becomes `Self`).

- `examples/hello.rs` — **stays closure-shaped** (`from_fn` or free `run`): the teaching
  one-liner. Every other example converts to `impl App` — that is the idiom being taught.
- `examples/counter.rs`, `focus.rs`, `todo.rs`, `form.rs`, `gallery.rs` — `impl App for X`.
- `examples/fetch.rs`, `stream.rs`, `agent.rs` — `impl App<Msg> for X`; move any
  first-event/startup workarounds into `fn init`.
- `comparisons/rabbitui/src/main.rs` — `impl App<Msg> for App_`; move the stream spawn from
  the `Event::Started` match into `fn init` (`Command::stream(LogSource::new()).group("source")`),
  and hoisted global Ctrl-C from the top of `update` into `fn global`. Update the README
  friction notes accordingly.
- `rabbitui-agent/src/app.rs` — `impl App<Msg>`; `fn global` takes the Ctrl-C/quit chord
  check; `fn config` sets the flagship's mode/theme wiring (read what `main.rs` currently
  passes to builders and move it into `config`).
- `rabbitui/tests/e2e_headless.rs` — `harness()` uses `from_fn`; **add one trait-based
  app** to cover the trait path e2e (see Step 7).
- `rabbitui/tests/todo_flow.rs`, `agent_flow.rs`, `set_theme.rs` — `from_fn` wrap.
- Doc-tests inside `rabbitui/src/app.rs` reference `App::new` (e.g. the `log_handle`
  example) — update each to `from_fn` or a tiny `impl App`.

Gate: `cargo test --workspace`, `cargo test --manifest-path comparisons/rabbitui/Cargo.toml`,
clippy, and `cargo +nightly fmt --all`. Commit Steps 2–6 together:
`feat(app)!: trait App core model — from_fn adapter, Config, init/global hooks`.

## Step 7 — e2e tests for the new hooks (the FakeDevice harness earns its keep)

In `rabbitui/tests/e2e_headless.rs`, add a trait-shaped test app alongside the closure one:

- `init_cmd_arrives_before_input`: `fn init` returns
  `Command::future(async { Msg::Seeded })`; app renders a marker when `Seeded` lands;
  `wait_for` that marker with **no input fed** — proves init spawns at startup.
- `global_break_quits_even_when_update_would_return_early`: give the app a "modal open"
  state under which its `update` early-returns before its quit branch; put quit
  (`Ctrl-C` = byte `\x03`) in `fn global`; assert `join()` returns Ok after feeding it.
- Keep the two existing closure tests passing unchanged (they now go through `from_fn`).

Run 5× for flake check (pattern from this file's header).

## Step 8 — docs + ADR amendment (commit separately)

- `rabbitui/src/app.rs` module doc: lead with the `impl App` example; `from_fn` shown as
  the test/demo shorthand.
- `README.md`: update the front-page snippet to `impl App`.
- `skills/rabbitui/SKILL.md`: update the app-shape sections (search `App::new`).
- `docs/adr/0001-programming-model.md`: append
  `## Amendment (2026-07-11): the app-facing shape is a trait` — 2–3 paragraphs: the
  closure form failed three growth tests (findings #1/#7, suspend); trait with defaulted
  hooks + `from_fn` adapter; §6's "shells layer above" is unchanged; cite
  `docs/design/core-model-and-roadmap.md` §1 for the six resolved decisions.
- markdownlint every touched doc (100 cols, no em-dashes inside any aligned table).

## Acceptance (all gates, in order)

1. `cargo check --workspace --all-targets` — everything compiles, including all examples.
2. `cargo test --workspace` and the comparisons app suite — all green.
3. `cargo clippy --workspace --all-targets` — zero warnings.
4. `cargo +nightly fmt --all --check` — clean.
5. e2e suite (`--test e2e_headless`) 5× stable, including the two new hook tests.
6. Flag for the coordinator: betamax visual smoke of the flagship + gallery over a real
   TTY (TestApp/headless cannot prove the visible loop; this session's standing rule).

## Completion (2026-07-11, wave-a workspace)

**Landed.** Steps 1–8 complete; all acceptance gates green. Commits (oldest first,
in the `wave-a` workspace, awaiting the coordinator's rebase onto trunk):

- `topoxswl` — docs(plans): claim Wave A lane
- `mmszuupk` — feat(effect): `Command::none()` — the no-op command (Step 1, committed alone)
- `qzmlwqwu` — feat(app)!: trait App core model — `from_fn` adapter, `Config`,
  init/global hooks (Steps 2–7: the trait, `Config`, `FnApp`/`from_fn`, the rewired
  loop, all 15 call-site migrations, and the two new e2e hook tests)
- `yqntsqzm` — docs(app): teach trait App shape — README, SKILL, ADR 0001 amendment (Step 8)
- (this file's completion note commits last)

**Gates run, all green:** `cargo check --workspace --all-targets`; `cargo test --workspace`
(all suites, incl. 96 workspace doc-tests); the comparisons suite
(`cargo test --manifest-path comparisons/rabbitui/Cargo.toml` — 7 passed); `cargo clippy
--workspace --all-targets` and the comparisons clippy — zero warnings; `cargo +nightly fmt
--all --check` (and the comparisons crate) — clean; `--test e2e_headless` 5× — 4/4 stable
every run, including `init_cmd_arrives_before_input` and
`global_break_quits_even_when_update_would_return_early`; markdownlint-cli2 on every touched
doc — clean. `cargo doc -p rabbitui --no-deps` is warning-free (the pre-existing `log.rs`
links to old builder methods were fixed during Step 5).

**Net-diff smell test (met).** `global`/`init` deleted real code where the plan predicted:
the flagship (`rabbitui-agent`) collapsed a Ctrl-C quit check that had been **duplicated
across three early-return paths** (help arm, confirm branch, base bindings) into one 3-line
`fn global`, and dropped `run`/`run_themed` entirely — `app.rs` net −3 lines despite gaining
a field, a builder, `config`, and `global`. The comparisons log-follower moved its stream
spawn out of an `Event::Started` match arm into `fn init` and hoisted its Ctrl-C into
`fn global`. Examples run +0 to +7 lines each (the `struct` + `impl App` wrapper and one
extra indentation level); `hello.rs` stays closure-shaped and unchanged; `focus.rs` is
exactly equal. `agent.rs` was kept lean by reverting all but `update`/`view`/`config` to free
functions.

**Corrections logged** (also under the header): README had no front-page snippet to update
(added a "What it looks like" `impl App` section instead); the pre-existing `log.rs` doc
warnings were fixed as part of Step 5.

**Deviations from the letter of the plan (both correct):**

- Step 6 lists `todo_flow.rs`, `agent_flow.rs`, `set_theme.rs` for a `from_fn` wrap, but they
  drive `rabbitui_testing::TestApp` (a reducer harness, not the run loop) and never touched
  the old `App::new` — they already compiled and passed against the new API, so they were
  left untouched (a `from_fn` wrap would be a behavior-changing rewrite). The `App::new`
  matches in them are `TestApp::new`.
- `form.rs` and `gallery.rs` were found already broken against the pre-migration tree (they
  called `App::new(...).theme(...)`, and `rabbitui::App` now names the trait) — the migration
  fixed them.

**Flagged for the coordinator (not attempted here, per the standing rule):** betamax visual
smoke of the flagship + gallery over a real TTY — TestApp/headless cannot prove the visible
loop. Nothing visible changed by intent, but the migrations moved the flagship's launch
wiring and every example's shape, so a visual pass is warranted before trunk.

**Lane hygiene:** only the files in this session's allowed set were touched;
`rabbitui-core`/`rabbitui-widgets` (Wave B2's lane) were not. The `Cargo.lock` qwertty
0.1.1→0.1.2 bump is forced drift from the on-disk substrate path dep, not a change this lane
made. `jj st` is clean after the final commit.

## Sizing & parallelization

Steps 1–5 are one lane (single writer on `app.rs`/`effect.rs`) — roughly a day for a
careful model. Step 6 fans out cleanly after Step 5 compiles: examples / comparisons /
flagship / tests are four independent lanes. Steps 7–8 are small follow-ups. Wave B1
(`wave-b1-flagship-e2e.md`) is parallel-safe with Steps 1–5 (different files) but lands
cleaner after Step 6's flagship migration — sequence it after if only one lane is running.

## Known traps

- The shell cwd resets every turn — `cd work/default` before every command (playbook §1).
- `ctx` must be constructed once and lent to `global` before moving into `update`; do not
  build two `Update`s (double-drains pending).
- `Config` fields are cfg-gated for `tracing` — mirror the cfg on the builder methods and
  on every construction site, or non-tracing builds break.
- Doc-tests count as call sites; `cargo test --workspace` runs them — don't skip.
- Trait-method resolution: if a migrated app also has an inherent method named `update`,
  the inherent wins at call sites — rename the inherent (this bit nobody yet; watch for it).

## What good looks like (beyond the acceptance gates)

- A newcomer can read `impl App for Counter` in `examples/counter.rs` and answer "where
  does state live, where do keys arrive, how do I quit" without opening the framework.
- Every migrated example is SHORTER or equal, and none needed a comment explaining the
  trait mechanics — if one did, the trait docs (not the example) were the fix.
- `from_fn` hello is still a screenful; the trait examples read as the idiom being
  taught, not a second dialect.
- `global`/`init` in the flagship and log-follower deleted real code (the hoisted Ctrl-C
  block, the Started match) — net-negative diffstat outside the facade is the smell test.
- The ADR 0001 amendment quotes the three growth failures as evidence, not opinion.
