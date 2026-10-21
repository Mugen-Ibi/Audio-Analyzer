# Native measurement UI review — 2026-10-18

Follow-up: [ZERO_FRAMEBUFFER_FIX.md](ZERO_FRAMEBUFFER_FIX.md) records the later
reproduced zero-size capture failure, renderer/application fix and new verification.
The earlier framebuffer failure below is retained as the original audit
record; use the follow-up for their current status.

Scope: a separate local UI revision after functional commit `b636b27` on
`codex/requirements-v03`. No capture/output, remote push, PR or publication.
The measurement pipeline, calibration limits and privacy policy remain applicable.

## Evidence and audit

Fresh native release framebuffers were captured in this audit run, using the
application's egui screenshot command. They are not browser mockups or cached
screenshots from the preceding implementation. Files are in
`target/design-audit/before/` (ignored build evidence):

| Step | State / file | Observed issue | Decision |
| --- | --- | --- | --- |
| 01 | Input stopped / `01-idle.png` | Empty spectrum lacks useful axes and next action context; five transport buttons compete | One state-dependent start/stop/cancel action, explanatory empty state with axes |
| 02 | Synthetic measuring / `02-measuring.png` | Diagnostics, caveats and large meters consume space; spectrum gets half the width; sparse log ticks | Compact source strip, wide spectrum, compact meter rail, meaningful frequency/dB ticks |
| 03 | HOLD / `03-hold.png` | LIVE and display-stop wording need careful reading to distinguish | Distinct HOLD badge, frozen window time, explicit ongoing input/history |
| 04 | Settings / `04-settings.png` | One long modal puts lower conditions and save below the fold | Four purpose-based tabs and reachable footer; scroll only the content |
| 05 | 760×640 / `05-narrow.png` | Initial viewport contains only spectrum; oversized header hides meters and histories | Compact two-channel meter strip, sticky transport and scope note, scrolling history |

The main task is to inspect frequency/level and compare changes under the same
conditions. Source selection, analysis setup and diagnostics support that task.
Warnings must stay discoverable without dominating every measurement frame.
All current screenshots were visually inspected. They do not establish successful
use by an unfamiliar person or screen-reader accessibility.

## Chosen structure

- Sticky transport: brand, connection/display state, one input action, HOLD, settings.
  No capture begins on normal launch. Synthetic mode explicitly identifies itself.
- Source strip shows applied rate, channel count and buffer; details go in Diagnostics.
  Result age is labelled separately from total audio latency. Errors/stale input take
  precedence; the frozen target window is explicit during HOLD.
- Desktop spectrum uses the width remaining after a 220-point meter rail. Narrow
  layouts put two compact meters above the plot. Data numbers retain a fixed-width face;
  ordinary Japanese UI text uses the proportional family.
- Frequency histories are full-width stacked strips. Both channels and RMS history
  share graph insets, time bounds and cursor. Duration is a single 10–600 s selector;
  older/newer/current controls keep existing history semantics and 60 s default.
- The bottom scope strip always exposes dBFS/relative comparison and unknown input
  processing. A visible button opens the full limitations and reproducibility notes.
- Settings: Input / Analysis & display / Comparison conditions / Diagnostics.
  Changes apply immediately; Save means persist settings for next launch. This wording
  avoids implying a Cancel transaction. A footer provides Save and Close.
- Keyboard: F5 input start/stop, H HOLD, L return to live, F1 settings, Esc close.
  Letter shortcuts do not steal text entry, held keys do not repeat transport, and
  measurement shortcuts do not run behind open settings. Native focus/Tab remains.

Visual tokens extend the existing slate theme and blue/green channel mapping:
background `#10141b`, surface `#161c25`, outline `#354356`, text `#f1f5fb`,
secondary text `#a6b3c5`, blue `#5ba8ff`, green `#3dd68c`, amber `#ffc857`.
UI text 13 pt, secondary 11 pt, numerical RMS 24 pt; fine outlines and restrained
rounding. Color is accompanied by channel text, state words and numerical units.
No decorative raster assets are embedded in the application.

## Design process / tools

Product Design router, audit, user-context and audit framework were read. Missing
cloud common-reference reads were recovered from the matching local Product Design
0.1.56 package (critical overrides and communication protocol). User context preflight
reported no saved personal design context; nothing was invented or saved as preference.
No Figma canvas was requested or created.

One built-in ImageGen `ui-mockup` concept grounded in the five current screenshots
was used to review hierarchy, then the native Rust/egui code was implemented. The
first file-path image request failed before generation because that tool could not
deserialize a Windows absolute path; the supported conversation-image reference
succeeded. No API/CLI fallback was used. The concept is a design study, not test
evidence; its curves and text are not copied into runtime assets.

Prompt: native Windows dark slate laboratory instrument; compact transport/source
strip; dominant 80%-width logarithmic 20 Hz–20 kHz spectrum and 220 px digital
RMS/Peak meter rail; aligned stacked two-channel spectral histories and RMS history;
readable Japanese UI, blue/green channels, explicit dBFS/relative scope and unknown
AGC. Avoid decorative dashboards, oversized bars, playback, recording and SPL claims.

## Verification status

Implementation and QA results are appended after the native review. Physical input,
screen reader, unfamiliar-user usability and real input-to-display latency remain
separate acceptance work; synthetic screenshots cannot pass those checks.

Iteration log:
- Initial keyboard regression: one failure because the fixture did not release L
  between presses. Added key-up events; all five UI tests then passed. Repeat-key
  protection was behaving correctly and was retained.
- First native review found the automatic vertical separator consumed remaining
  viewport height and pushed history below the desktop fold. Replaced it with a
  bounded 280-point divider; recapture required. Also made the central background
  explicit and reduced grid contrast so spectral traces dominate.
- A grid rendering edit initially landed in the history-cursor block and failed
  compilation (undefined plot/advanced/bounds). Moved it into the spectrum closure.
  Failed builds and intermediate screenshots are not acceptance evidence.
- Native QA stopped once while launching the Analysis settings scene; both process
  logs were empty and the former script did not include its nonzero exit in the
  exception. The same unchanged executable and scene passed on retry. Improved the
  runner to always print the exit and reject stale screenshots. Cause of that
  isolated process termination is unknown; it is not counted as a successful run.
- Kept cumulative audio/display loss and clip counts available in the source strip
  whenever nonzero. HOLD wording now distinguishes a stopped input from ongoing
  capture. Historical channel labels say “current”; older routes remain in segment
  metadata, so old history is not silently relabelled as the current physical input.
- Strict Clippy found one collapsible nested condition in that status button.
  Collapsed it without changing behavior, then repeated the full validation script.

Final automated verification (script exit 0):
- `cargo fmt --check`; strict Clippy for all features/all targets and core: passed.
- All-feature debug, WASAPI-only desktop and all-feature release: **63 passed each**
  (46 unit + 17 integration); one live-device test ignored in each desktop suite.
- Core: **56 passed**. Windows MSVC ASIO release build: passed.
- Five UI regressions include keyboard hold/repeat/settings guard, text entry,
  disconnected/failed rendering, HOLD/condition changes and synthetic reconnect.
- Existing frequency/amplitude, average reset, gap and clipping, pool allocation,
  history and lifecycle regressions all passed. No new dependencies or audio/DSP
  pipeline changes were introduced by this UI revision.
- Final fresh 13-scene native run exited 0: desktop; minimum; scrolled minimum
  history; all four settings tabs; minimum settings; 1440×900; application zoom
  1.5 and 2.0; input stopped; HOLD. All 13 framebuffers were visually inspected.
  `target/design-audit/after/release-*.png` and `target/qa/*.out.log` hold evidence.
- Normal desktop screenshot 1800×1250 at the observed 125% display scale;
  minimum 950×800. The HOLD window moved to a 1920×1111 framebuffer during the
  run; its independent live/history values continued while the display stayed held.
  Zoom scenes change egui application zoom, **not Windows system DPI settings**.
- HOLD proof in final run: held sequence 180, live sequence 510, held window center
  about 1.492 s while retained history reached 4.927 s. Required plots remain
  visible on desktop; smaller/zoomed viewports scroll with sticky transport/scope.
- Foreground/secondary/blue/green/amber text contrast against their assigned surfaces:
  15.64 / 8.05 / 7.46 / 9.84 / 11.13 respectively (sRGB luminance calculation).
  This is a color check, not full accessibility certification.
- Final `git diff --check`: passed. No demo process remains running.

Executable: `target/deliverables/AudioAnalyzer.exe`, SHA256
`B94415F911847DDBB7F3FE4F678AFF908ABFDA14C4290EEB9E5CB8741B3D4276`.

The following historical screenshot failure was subsequently fixed; see [ZERO_FRAMEBUFFER_FIX.md](ZERO_FRAMEBUFFER_FIX.md).

The final rebuilt executable passed a fresh desktop smoke run after the
behavior-preserving Clippy cleanup. Its following HOLD screenshot smoke run
**failed (exit 101)** in `egui_glow-0.30.0/src/painter.rs:693` with
`chunk size must be non-zero`, in the native framebuffer screenshot path. This
indicates an empty framebuffer during capture; it is not a successful final HOLD
capture. The previous thirteen-scene run (including HOLD) and the synthetic HOLD
functional regression passed; those are the retained evidence, rather than the
stale screenshot left by the failed run. The required QA runner correctly rejected
the failure. Capture while the native window/monitor is stable, and handling of
zero-size/minimized screenshots, remain to be checked. No dependency or renderer
was replaced to hide this failure. These runs used only synthetic/idle modes.
Failure log: `target/qa/release-hold.err.log`. No process remains running.

Physical microphone capture, speaker output, real device latency/AGC evaluation,
60-minute physical stability, unfamiliar-user interaction and screen-reader QA were
not performed. User permission is still needed before real capture/output.
