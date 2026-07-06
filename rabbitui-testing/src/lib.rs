//! Headless test harness for rabbitui applications.
//!
//! Arrives with roadmap slice 2 (`ROADMAP.md`): a headless driver that injects
//! events, advances an injectable clock, runs frames, and asserts on buffers —
//! plus, from slice 5, a PTY-level harness that asserts on emitted escape
//! sequences through a vt100 parser. Testing ships before the widget catalog
//! by design (`docs/adr/0009-testing.md`).
