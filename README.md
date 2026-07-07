# rabbitui

A Rust terminal UI framework synthesizing the best ideas of existing TUI frameworks (Textual, Ink,
Bubble Tea, Ratatui, Brick, …) and the lessons of Rust-native GUI work (Xilem, Masonry, Druid) —
built research-first.

## Status

**Early implementation.** The research and design phases are complete; the framework is being built
in vertical slices ([ROADMAP.md](ROADMAP.md)). Nothing is published yet.

- [DESIGN.md](DESIGN.md) — the architecture in one read: a _declared-frame_ model (app-owned state,
  framework-owned widget identity and frame facts, commands-only async effects) on a cell-buffer
  diff renderer with inline and alt-screen as peer modes
- [docs/adr/](docs/adr/) — one ADR per design decision, with alternatives and evidence
- [docs/research/](docs/research/) — the survey memos the design is grounded in (13 studies: the
  major frameworks, the Rust GUI literature, the terminal substrate, prior next-gen attempts, the
  2024–26 framework wave, and Codex's tui2)
- [docs/field-report.md](docs/field-report.md) — a shareable state-of-the-field synthesis

## Workspace

| Crate              | Purpose                                                                             |
| ------------------ | ----------------------------------------------------------------------------------- |
| `rabbitui-core`    | Runtime-free foundation: geometry, styles, buffer, identity, facts, widget contract |
| `rabbitui`         | The facade: async event loop, terminal session, rendering                           |
| `rabbitui-testing` | Headless driver and PTY-level test harness                                          |

Try it: `cargo run --example smoke` (or `hello`, once slice 1 lands).

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
