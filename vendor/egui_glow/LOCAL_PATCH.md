# Local zero-size framebuffer patch

Copied from the already resolved crates.io `egui_glow` 0.30.0 package; the source,
normalized Cargo manifest, README and example are retained. Crate VCS metadata:
`0a3edabd3544c6d339eacd9e606fccee302e4c35`, `crates/egui_glow`.
Original registry checksum:
`3eaf6264cc7608e3e69a7d57a6175f438275f1b3889c1a551b418277721c95e6`.
Licenses are the official upstream 0.30.0 MIT and Apache files.

Local changes, 2026-10-18:
- `painter.rs`: skip clear/paint for zero-area surfaces; keep texture updates/free
  processing. Route framebuffer reads through the checked helper and return an
  empty `ColorImage` when no image exists, instead of `chunks_exact(0)` panicking.
- `screen_capture.rs` and its `lib.rs` declaration: reject zero/overflow sizes
  before GL access, reverse RGBA rows in place. The exact helper is regression
  tested by `tests/framebuffer_capture.rs` in the parent application.

No shared Cargo registry files are modified. The application pins this local crate
with `[patch.crates-io]`; its egui/eframe versions and GL renderer are unchanged.
