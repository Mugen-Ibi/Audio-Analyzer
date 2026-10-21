# Zero-size native framebuffer — 2026-10-18

Follow-up to UI commit `3e8caff`. Only synthetic/idle inputs were used; there was
no microphone capture, speaker output, remote push, PR or merge.

## Cause and scope

The original panic is in `egui_glow` 0.30.0 `Painter::read_screen_rgba`:
the framebuffer's width becomes 0 while the native window is minimized, then
`pixels.chunks_exact(w * 4)` panics because the chunk size is zero.
eframe requests the current native window size after application update, so a UI
size check alone cannot close the request/read race. Its normal clear/paint path
does not use that row-splitting expression; readback is a screenshot-only action.

Controlled reproduction targets the real returned render window, not a guessed
handle: enumerate this fixture's own PID, exclude its console, require the unique
`Audio Analyzer — SYNTHETIC` window, verify ownership before manipulation.
It performs four resizes, two minimize/restore cycles, and places the five-second
capture deadline inside the second minimized interval. The observed client area
is `0x0` during both minimized intervals.

| Build / action | Result |
| --- | --- |
| Original B944… binary, ordinary drawing (no screenshot command) | Resize, minimize/restore and normal WM_CLOSE succeeded, exit 0 |
| Same original binary, HOLD screenshot deadline during minimize | Reproduced the exact `painter.rs:693` panic, exit 101 |
| Patched binary, ordinary drawing (no screenshot command) | Same sequence and normal WM_CLOSE succeeded, exit 0 |
| Patched binary, HOLD capture during minimize | Restored-window screenshot succeeded, exit 0; repeated runs recorded below |

Thus the reproduced crash is in framebuffer readback. Ordinary minimized drawing
did not crash in this experiment. This observation is not a guarantee about every
driver/monitor configuration.

The fixture's first iteration used .NET `MainWindowHandle`; that selected the
process's console window and could not close the analyzer. Those early fixture
timeouts and its apparent capture successes are **not valid window-state QA**.
The corrected fixture logs all returned windows and the chosen render handle;
only `*-render*` cycle logs are acceptance evidence.

## Fix

- Keep the resolved egui/eframe/GL versions. A small local source copy of
  `egui_glow` 0.30.0 is selected by Cargo's `[patch.crates-io]`. No shared registry
  files were edited. Upstream source and licenses are retained, with the exact
  changed files documented in `vendor/egui_glow/LOCAL_PATCH.md`.
- Before any framebuffer allocation or GL read, reject zero width/height and
  integer-overflow sizes. Return an empty image. Valid RGBA rows reverse in place,
  preserving the original pixel and alpha bytes.
- Skip clear and primitive/callback submission on a zero-area surface. Texture
  uploads and frees still run, so a restored window keeps its texture lifecycle.
- The application's internal capture mode waits for a non-minimized stable size.
  If a minimize race nevertheless returns an empty image, it defers capture and
  retries after restoration; it neither saves an invalid PNG nor reports success.

The dependency patch is required even with the application guard: only the
renderer knows the actual size at the instant it reads the framebuffer.

## Regression and native QA

Five new tests pass:
- zero-width, zero-height and empty framebuffers never invoke the GL reader;
- a three-row RGBA fixture flips rows without reordering channels/pixels, and a
  one-pixel image remains unchanged;
- overflow is rejected before the reader;
- an empty screenshot event causes no file write or close command;
- minimized capture waits and a restored, stable viewport requests capture again.

The pure helper compiled by the integration test is the exact file called by the
patched renderer. These tests do not depend on a live audio device or GL context.
Full validation after the patch: fmt, vendor fmt, strict all-feature/core Clippy,
all-feature/WASAPI/release **68 tests each**, core **59 tests**, Windows MSVC ASIO
release build and the existing DSP checks passed. Live-device tests remain ignored.

Evidence: `target/window-qa/*-render*.cycle.log`, corresponding `.out.log` / `.err.log`
and restored-window PNGs. Final native screenshots and Library status follow below.

Final results:
- Three consecutive patched minimize-at-deadline HOLD runs succeeded, each exit 0
  with a fresh 1782×1203 restored-window PNG. In the third run the held display was
  sequence 165, while the live input advanced to 579 and retained history to 5.901 s.
- The standard 13-scene native suite then passed in full (exit 0), including the
  previously failing HOLD scene. All 13 fresh framebuffers were visually inspected,
  including desktop, narrow, scrolled history, all settings tabs, HOLD, idle and
  zoom/shorter-height views, with no acceptance claimed for
  physical audio or actual Windows DPI changes.
- No analyzer process remains running. SHA256 of the final executable:
  `B407B09D7E4D06F90493B3EABCE92C84177E5DA4D668387E2506C9682A5A506B`.
- `git diff --check` passed. Comparing vendored upstream sources against the original
  registry copy shows changes only in `src/lib.rs` and `src/painter.rs`, plus the
  new helper; other upstream sources are unchanged.
