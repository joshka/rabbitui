# rabbitui — crates.io name reservations

Placeholder 0.0.0 releases reserving the crate names used by the rabbitui
project (a Rust terminal UI framework in early development) ahead of its
first real release:

- `rabbitui` — the framework facade
- `rabbitui-core` — core types (geometry, style)
- `rabbitui-widgets` — widget catalog
- `rabbitui-testing` — test harness and snapshot utilities
- `rabbitui-ratatui` — ratatui interop
- `rabbitui-agent` — flagship agent client

Each crate is an empty `#![no_std]` library with no dependencies. Real
releases will start at 0.1.0.

To publish after review: `./publish.sh` (or `cargo publish --workspace`
on cargo ≥ 1.90).

Licensed under MIT OR Apache-2.0.
