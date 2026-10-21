# Audio Visualizer — design and verification, 2026-10-21

## Scope and working state

Approved observation workflow: log frequency × digital band level × time in one native Windows window, with synchronized total RMS history. The implementation reuses the existing Rust/egui capture, analysis and rendering pipeline. Microphone capture, speaker output and PCM recording were not used during synthetic verification.

Product Design's index, user-context preflight, critical overrides, and screenshot-based audit workflow informed the review. No saved user design context was found. Existing native fonts/tokens and the Rust/egui renderer were reused; no website, decorative imagery, particles, or separate rendering engine was introduced.

## Screenshot audit and resulting interaction

1. Fresh baseline desktop, minimum window, and HOLD screenshots were captured with the inherited executable this turn, in `target/visualizer/before`. Desktop showed the current spectrum above two frequency histories and RMS history. Frequency, level, and time required separate visual comparisons. At 950×800 physical pixels, histories were below the first viewport. HOLD did identify continuing acquisition correctly.
2. The main chart now combines the three quantities. Frequency increases left to right on a log scale, height is the maximum FFT-bin peak amplitude within each of 96 log bands, and older time recedes into depth. The front is newest. The fixed default camera and fixed world bounds do not adapt to signal amplitude. Camera projection fits the chart rectangle independently of the data. The 2D alternative uses the same stored records, frequency horizontally and newest time at the bottom. Two inputs share an identical level scale and can be toggled individually; input channels are physical stream channel numbers, never assumed microphone L/R.
3. Primary pointer drag rotates yaw/pitch; wheel zooms directly without residual wheel smoothing; double click or the reset button restores the default. Sliders expose yaw and zoom. H/HOLD freezes the display epoch while input/history continue; input stop still stops acquisition. History span, older/newer, and return-to-current controls stay in the same window. At small heights, spacing and chart height contract; extra detail remains in the same scrollable window.
4. The total level strip explicitly says RMS/Peak and current/HOLD window. The auxiliary history explicitly says total window RMS, with the exact same time bounds as the main chart. A shared cursor links a 3D ridge, a 2D row, the RMS plot, and time/band sliders. The inspector reads the original retained record, its condition segment, rate, FFT/window/averaging, per-input band maximum, and total RMS. Gaps and expired results have no numeric replacement. A log band lacking an FFT bin reports unavailable rather than silence; the inherited analysis floor remains −120 dBFS. A frequency band is not an integrated energy band, PSD, SPL, or room-only response.
5. “基準を保持” copies one valid, unclipped current/HOLD spectrum and its conditions. A dashed reference appears at the front of the 3D chart and in the optional same-window spectrum comparison. During reference comparison both curves use the identical 96-band grid, avoiding an apparent frequency shift between raw FFT centers and log-band centers. Reference input/condition metadata is expandable alongside its FFT/rate metadata. Reference results can be explicitly saved/loaded beside the existing settings file as `spectrum-reference.json`; no PCM is stored. Demo mode disables these disk save/load controls. Different analysis/rate conditions are flagged, and gain/AGC/position/input-processing comparability always needs manual confirmation. Camera and 3D/2D choice are included in explicit settings save; old version-1 settings keep their values and acquire defaults.

## Data and drawing limits

The acquisition, callback, FFT, calibration convention, overlap, averaging, result queue and 10 Hz/600-second history remain unchanged. Only presentation and derived reference persistence were added. History is still up to 6001 records (the boundary record included).

At most 64 independent actual time slices are drawn in 3D. For each time bucket the renderer selects the actual valid record with the strongest overall spectral peak across either input, breaking ties toward newer records. All bands and both channels come from that one record. There is no temporal/channel averaging or synthetic surface bridging separate times or conditions. This selection can omit weaker events in a coarse view; reduce the time span or inspect the original records with the cursor. The 2D view uses 32–512 rows and the same selection rule. Valid intervals fill their pixels; actual missing time stays blank. Red markers retain condition boundaries, gaps and clips. Pixel/bucket resolution cannot expose every event at once.

History projection/texture generation is cached by visible records, camera, rectangle, scale, mode and channel selection. HOLD does not rebuild for new records outside its viewport, unless retention expiration moves the stored indices. A fresh current FFT profile can overlay the front between 10 Hz history updates. egui 0.30 owns submitted meshes, so submission still copies the cached mesh; it is not a zero-copy GPU history buffer. The prior zero-framebuffer guard remains in the local `egui_glow` patch.

## Failures found and resolved during this turn

- Compilation exposed egui 0.30's owned `Mesh` API; the cache now reuses projection and submits an owned mesh copy.
- Synthetic pointer tests caught residual smoothed wheel increments after camera reset. Using direct wheel deltas removed that behavior; drag, zoom and double-click are tested through egui events. The event fixture also needed a first layout frame before hit testing.
- Native screenshot review caught false horizontal gaps in 2D when a record occupied fewer pixels than its valid interval. Sampling now uses the same interval semantics as history lookup, with a dedicated missing-time regression test.
- The reference disk test initially used the sandbox-redirected system TEMP, which denied the write (`os error 5`). Its fixture now writes only inside the repository's ignored `target/test-data`, and successfully tests replacement/readback/oversize rejection there. This was a fixture destination restriction, not a Library or microphone action.
- Strict Clippy caught nested conditions and an indexed bucket loop; both were corrected.
- Native reference comparison review caught the raw-FFT/log-band grid mismatch described above; both curves now use the same band grid when a reference is present. One in-progress validation saw an older compiled library while the binary source was being edited; final validation was rerun with all Rust sources fixed, rather than accepting that mixed run.
- The native QA fixture's one-shot numeric offset could clamp to the prior frame's shorter content just as the synthetic reference panel appeared. A timed retry was still vulnerable to viewport changes. Its internal `--qa-history` mode now targets the actual post-layout bottom with egui's `scroll_to_cursor`; ordinary manual scrolling is unchanged. Only the affected UI checks and build were repeated after the final QA-only adjustment.

## Verification

Logs remain under `target/validation`, `target/visualizer/after`, `target/qa`, and `target/window-qa`. Implementation commit: `319bb3a222ffa430a180911cae8f627620cac3dc`; the subsequent commit contains the QA layout-wait correction and final verification record.

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Pass |
| Strict Clippy, all targets/all features and core | Pass |
| Debug all features | 76 passed; real capture test deliberately ignored |
| Core, no default features | 66 passed |
| WASAPI desktop configuration, no ASIO | 76 passed; real capture test ignored |
| Release all features | 76 passed; real capture test ignored |
| Windows MSVC release, all features | Pass; ASIO SDK and target directory use the existing ASCII paths documented in previous validation |
| Synthetic 6000-record/2-input projection | 97,280 worst-case history vertices; finite valid mesh, cache reuse, input toggle and gap tests passed; first projection 1.509 ms, CPU only |
| Native synthetic/no-input QA | 10 visualizer cases and 13 layout/settings/zoom cases exited 0 with fresh screenshots; additional reference/camera checks and owned-window resize/minimize/restore/close checks completed |

Synthetic cases use 48 kHz, two channels, FFT 1024/Hann/no averaging, 256-frame synthetic blocks. Tones are 750/1500 Hz with amplitudes 0.5/0.25, stepping by a factor of 0.2 every three seconds. The default remains a 60-second visible history; 10-second spans were explicitly selected in the QA captures to expose repeated steps.

Before redesign, the native desktop UI tick interval p95 was 7.440 ms (5 seconds, 1800×1200 pixels). The final production implementation's 12-second 3D/2D captures at 1800×1200 measured p95 7.320/7.270 ms. These different-duration samples are indicative, not a controlled end-to-end latency comparison. One orbit/reference run had p95 8.806 ms and maximum 2395.456 ms, with correctly marked missing history. A quiet repeat of the same arguments at 1800×1200 measured p95 7.602 ms/max 29.230 ms. The long interval's cause is unconfirmed; window/framebuffer dimensions also varied in this series. Do not treat the repeat as proof that all long stalls are eliminated, or claim a 50 ms worst case. Intentional minimize/resize intervals are excluded from any normal-display comparison.

Pure DSP timing (3000 windows, 48 kHz/two inputs/50% overlap) excludes hardware, queues and rendering. This remains a conditional CPU measurement rather than an input-latency certification. The final timing log contains all FFT/window/averaging configurations.

Physical microphone operation, M2/AT4040 behavior, built-in processing/AGC, driver reconnect and physical input latency remain unverified. No input/output stream was opened for device enumeration; the ignored hardware test was not enabled.

## Additional bounded synthetic check

A separate diagnostic derivative completed one 600.005-second 3D run with rotation, reference comparison and two HOLD intervals. Its 85,489 UI-update intervals had p95 7.782 ms and maximum 70.282 ms. There were two intervals over 50 ms and none over 100 ms. The earlier 2395.456 ms pause was not reproduced; its cause remains unknown. These are UI callback intervals, not end-to-end audio latency or GPU presentation measurements.

The run retained 5,623 monotonic history records spanning 599.872 seconds, including 15 gap markers. Display-result queue drops reached 20 and latest-result coalescing 110; synthetic audio-frame drops, invalid samples and stream errors were zero. Both HOLD periods froze the displayed sequence while live history continued. The application exited normally.

Private-memory median rose from 153.34 to 173.86 MiB; working-set median rose from 146.50 to 152.60 MiB. History filling and rendering changes coexisted, so the cause of growth and long-term stability are undetermined. The diagnostic source and aggregate report are maintained on the corresponding diagnostic branch, separately from the production release tag. Raw per-frame logs, OS information and private transfer records are not distribution assets.

## Release provenance

The release contains the tested product source with documentation privacy redactions. Diagnostic instrumentation is kept on a separate branch. The Windows executable is rebuilt from the product tag with Rust source-path remapping; it replaces no earlier local delivery. See [the release record](RELEASE_v0.1.0-beta.2.md) for build scope, usage and limitations. Screenshots establish native layout and synthetic behavior only; screen-reader support, every monitor/scaling configuration and physical input latency remain unverified.
