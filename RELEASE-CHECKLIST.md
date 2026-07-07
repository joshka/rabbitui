# Release checklist — the path to 0.1

rabbitui is pre-0.1: everything may still change (see `BREAKING-CHANGES.md`).
This file is the gate list for the first published release — what must be true
before `cargo publish`, each item a checkbox with its status marked _honestly_ as
of the last docs pass (slice 9). An unchecked box is a real blocker, not
aspirational; the two hard blockers are called out under "Known blockers" below.

Legend: `[x]` done · `[ ]` not yet · `[~]` partial (detail inline).

## Known blockers

These gate 0.1 and are outside the scope of the docs pass that produced this
list. They are stated first so the release owner sees them without reading the
whole checklist.

- **qwertty is a path dependency.** The workspace depends on `qwertty` by
  filesystem path (`../../../qwertty`) with no version, which cannot be published
  to crates.io. This unblocks only when qwertty publishes (or is pinned to a git
  revision as an interim); see `docs/adr/0012-terminal-substrate.md`. Blocked on
  upstream.
- **The positioning decision is undecided.** Whether rabbitui ships under the
  reserved `ratatui-*` names is deferred to the author by design
  (`docs/adr/0014-positioning.md`, ADR 0014). The crate names cannot be finalized
  — and thus nothing can be published — until this is made.

## Code and correctness

- [x] All examples compile and run (`cargo run --example <name>` for each in
      `rabbitui/examples/`).
- [x] `cargo test --workspace` is green, doctests included — every doc example
      compiles.
- [x] `cargo clippy --all-targets` across all rabbitui members is warning-free.
- [x] `unsafe_code = "forbid"` and `missing_docs = "warn"` hold workspace-wide
      (declared in the root `Cargo.toml` lints).

## Documentation

- [x] `cargo doc --workspace --no-deps` builds with zero warnings (no broken
      intra-doc links).
- [x] Every public module carries a rustdoc example, its ADR anchor where one
      exists, and cross-links to related modules.
- [x] The `rabbitui` crate root is a mini-tutorial: a complete counter plus the
      guided tour (state/update/view, keys and identity, outcomes, theming,
      effects, inline vs alt-screen, testing, interop).
- [x] Each other crate root (`core`, `widgets`, `testing`, `ratatui`) has a
      purpose statement, an orientation, and a runnable example.
- [ ] Each crate has a `README.md` (none exist yet; `readme` is unset in every
      `[package]`). crates.io shows the README as the crate's front page, so this
      is needed before publish.
- [x] `BREAKING-CHANGES.md` exists and records the pre-0.1 stance.

## Crate metadata

- [x] `description` is set on every crate.
- [x] `license`, `repository`, `edition`, and `rust-version` are set (via
      `workspace.package`, with `rabbitui-ratatui` raising `rust-version` to 1.88
      for ratatui 0.30).
- [ ] `keywords` and `categories` are set on each publishable crate (currently
      unset everywhere — needed for crates.io discoverability).
- [ ] `readme` points at each crate's `README.md` (blocked on the READMEs above).
- [ ] `documentation` is set or confirmed to default to docs.rs.

## Licensing

- [x] License is declared as `MIT OR Apache-2.0` in `workspace.package`.
- [ ] `LICENSE-MIT` and `LICENSE-APACHE` files exist at the workspace root and/or
      per crate (neither file is present yet; a dual-license declaration without
      the license texts is incomplete for publish).

## Dependencies

- [ ] No path dependencies remain in the published graph. **Blocked**: `qwertty`
      is a bare path dep (see Known blockers). The internal rabbitui crates carry
      both `path` and `version`, so they resolve correctly once published; only
      `qwertty` blocks.
- [x] Every dependency uses `default-features = false` where the default features
      are not needed (tokio, ratatui, toml, rustix, futures-core all trimmed).

## CI and process

- [ ] CI is configured and green (no `.github/workflows/` exists yet). CI should
      run, at minimum, `check`, `clippy`, `test --workspace`, `doc`, and
      `markdownlint-cli2` on the tracked markdown.
- [ ] MSRV is verified in CI (workspace `rust-version = "1.85"`; the ratatui
      bridge at 1.88), per the "stable minus one" policy in ADR 0011.
- [ ] The positioning decision (ADR 0014) is made and the crate names finalized
      (see Known blockers).

## Publish

- [ ] `cargo publish --dry-run` succeeds for each crate in dependency order
      (`core`, then `widgets`/`testing`/`ratatui`, then `rabbitui`). Blocked until
      the path-dep and metadata items above clear.
- [ ] Git tag `v0.1.0` created and `BREAKING-CHANGES.md` opens a `0.1.0` section.
