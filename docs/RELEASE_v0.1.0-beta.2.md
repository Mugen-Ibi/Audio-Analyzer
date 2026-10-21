# Audio Visualizer v0.1.0-beta.2

Windows x64 prerelease. The ASIO-enabled executable and combined corresponding source are distributed under GNU GPL version 3; original MIT/dependency/font notices are retained. See [distribution terms](../LICENSE-DISTRIBUTION.md) and `COPYING.GPLv3`. Download the dedicated corresponding-source asset for the project, locked dependency sources and required ASIO sources. The product tag contains the production visualizer; a separate diagnostic branch preserves the bounded endurance instrumentation and aggregate report. No default-branch merge is required for this release.

## Use

Download the Windows x64 binary ZIP (containing `AudioVisualizer.exe` and all license notices) and verify `SHA256SUMS.txt`. The corresponding-source ZIP is provided beside it. Run `AudioVisualizer.exe --demo` for a synthetic two-input demonstration with no microphone or speaker stream. Normal startup stays stopped: explicitly choose the backend, device and physical stream channel(s), then start input. Two microphones are not automatically stereo L/R. ASIO requires an installed compatible vendor driver; WASAPI supports built-in inputs. Start/stop controls acquisition; HOLD freezes the displayed epoch while history collection continues.

Drag the main chart to rotate, use the wheel to zoom, and double-click or reset to restore the initial camera. Switch to 2D for the same retained data, browse older time or select frequency/time values. The shared total-RMS history uses the same time range. Reference spectra can be retained and overlaid; save/load is explicit and disabled in demo mode.

## Changes

- Unified logarithmic frequency, per-band digital level and time in a 3D waterfall, with synchronized total RMS/peak context and a same-data 2D alternative.
- Two inputs share fixed scales and individual visibility controls; current/HOLD/stopped/missing states remain distinct.
- Up to ten minutes of analysis history, a default 60-second view, camera controls, linked inspection, reference comparison and saved settings.
- Explicit input/analysis conditions, stale/gap/clipping indications, bounded queues and a zero-size framebuffer capture correction.

Band height is the maximum FFT-bin peak amplitude within a log band, not integrated band RMS, PSD or calibrated SPL. Total RMS/peak is separate. The 3D view selects at most 64 actual history records; a coarse view can omit weaker events. Inspect retained records or reduce the time range for detail. Display scales do not automatically erase comparison differences.

## Verification and known limits

The product source passed format and strict lint, 76 all-feature debug tests, 76 WASAPI desktop tests, 76 release tests and 66 core tests. Physical capture tests were deliberately ignored. Native synthetic/no-input screenshot and camera/reference/HOLD checks passed. The final QA scrolling correction additionally passed all nine UI tests, strict lint and the Windows release build.

A diagnostic derivative completed one 600.005-second 3D demo with continuous camera movement, reference comparison and HOLD changes, then exited 0. Of 85,489 UI-update intervals, p95 was 7.782 ms and maximum 70.282 ms; two exceeded 50 ms, none exceeded 100 ms. The earlier approximately 2.4-second stop was not reproduced and its cause remains unknown. Callback intervals are not GPU presentation or physical microphone latency, and no 50 ms worst-case guarantee is made.

The run retained 5,623 ordered records spanning 599.872 seconds, with 15 history-gap markers, 20 display-result queue drops and 110 coalesced results. Synthetic audio-frame drops, invalid samples and stream errors were zero. Missing history is marked rather than interpolated. Private-memory median increased from 153.34 to 173.86 MiB and working-set median from 146.50 to 152.60 MiB. Memory growth and long-term stability remain undetermined; a filling ten-minute history does not establish a post-capacity plateau or a leak. OS event-log access was unavailable, so sleep-related confirmation is incomplete.

Physical microphone latency, M2/AT4040 operation, built-in AGC/noise processing, driver reconnect and long-duration hardware stability remain unverified. Gain, processing, microphone placement and route changes alter relative comparison conditions. Observed spectra include the source, microphone, playback chain and room; they are not a calibrated room-only response. Separate-device clock synchronization, IR and reverberation measurements are not promised. No microphone capture or speaker output was used for this release verification. No continuous PCM recording or audio upload is implemented.

## Build and package provenance

The Windows MSVC executable is rebuilt from this tag with Rust 1.99, `Cargo.lock`, all features and the existing vendored framebuffer patch. Rust `--remap-path-prefix` removes private workspace and dependency-cache paths. ASIO build inputs require a complete SDK, LLVM/libclang and ASCII SDK/target directories. The package includes the executable, usage, project license and third-party license notices. Debug symbols, raw diagnostic logs, local OS inventories and private transfer records are excluded. Release assets include SHA256 information; the executable's Rust/Cargo/font/vendor inputs match the previously tested product snapshot exactly.

The published executable is built offline from the supplied corresponding-source contents using `scripts/build-corresponding-source.ps1`. No additional feature or performance work was performed for publication.

Release packaging verification: the complete corresponding-source inputs built successfully with locked/offline Cargo, including ASIO. The new native synthetic desktop smoke exited 0 and its screenshot was inspected. Production Rust/Cargo/font/vendor inputs remain identical to the previously tested implementation. Private source paths are remapped in the executable. Binary and corresponding-source packages are distributed together under the terms above.
