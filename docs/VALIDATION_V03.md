# Requirements v0.3 historical validation

These results describe the earlier functional baseline. Current product verification and the later bounded synthetic check are summarized in [AUDIO_VISUALIZER.md](AUDIO_VISUALIZER.md). The UI redesign and zero-framebuffer correction have separate historical records.

## Confirmed baseline checks

- Baseline core: 31 tests passed.
- Format, strict all-target/all-feature Clippy and core Clippy passed.
- Functional baseline: 61 all-feature debug tests, 56 core tests, 61 WASAPI desktop tests and 61 all-feature release tests passed. The physical capture test was deliberately ignored.
- Windows MSVC all-feature release build and six synthetic/no-input native screenshot scenes passed.
- HOLD preserved displayed values while the live sequence and retained history advanced. Synthetic lifecycle coverage includes conditions, history boundaries, stop and reconnect with a fresh epoch.
- Callback no-allocation, spectral pool exhaustion/immutability/recovery, history gaps, clipping recovery, configuration generations and corrupted settings are covered by tests.
- Read-only ASIO/WASAPI device enumeration was exercised without opening a stream.

The UI revision increased the desktop suite to 63 tests; the subsequent framebuffer correction increased it to 68 (59 core). The integrated visualizer subsequently reached 76 desktop tests (66 core). These are distinct source stages, not contradictory totals.

## Build and native QA issues addressed

Use Rust/Cargo 1.99 with the Windows MSVC toolchain, LLVM/libclang and a complete ASIO SDK. The ASIO C++ build required ASCII SDK and target-directory paths when the workspace contained Japanese characters. Set `CPAL_ASIO_DIR`, `CARGO_TARGET_DIR` and `LIBCLANG_PATH` to appropriate local installations; no developer-specific directory is required.

The initial native stack overflow was resolved by eight preallocated spectral slots. A real missing-interval fill error and cursor/renderer disagreement were fixed with regressions. Configuration changes discard stale queued generations, reset overlap/averages at block boundaries and preserve condition metadata. A short clipped interval remains visible in retained history and invalidates comparisons until clean recovery. Atomic settings replacement fixtures use ignored repository test data because the sandbox's redirected temporary directory rejected writes.

Synthetic screenshot fixtures were corrected to scroll after layout. A later empty-framebuffer capture panic was fixed in the vendored renderer and validated separately. An unsuccessful or stale screenshot is never counted as a passed capture. `scripts/validate.ps1` and the native QA scripts reproduce automated checks; hardware capture is excluded by default.

## Conditional performance evidence

Pure DSP measurements used release builds, 48 kHz, two inputs, 50% overlap, 3,000 measured analysis windows and 100 warmup windows. They exclude audio-device, queue and rendering latency.

| FFT / window / exponential averaging | p50 | p95 | maximum |
| --- | ---: | ---: | ---: |
| 1024 Hann / none | 24.50 µs | 28.60 µs | 178.80 µs |
| 2048 Hann / 100 ms | 48.60 µs | 55.00 µs | 795.30 µs |
| 8192 Hann / 500 ms | 200.30 µs | 240.30 µs | 468.70 µs |
| 8192 rectangular / none | 129.50 µs | 155.80 µs | 280.80 µs |

The pre-change 2048-point baseline had p95 48.00 µs. The added validation, metadata and ownership work is not claimed faster. Earlier native desktop UI intervals had p95 7.445 ms / maximum 46.960 ms; HOLD had p95 7.475 ms / maximum 60.775 ms. These finite samples include startup and do not certify a 50 ms response or input latency.

## Remaining limits

Physical M2/AT4040 and built-in microphone behavior, gain/AGC/noise suppression, driver disconnect/reconnect, real-input step response, end-to-end latency, 60-minute stability, clean-PC deployment, screen-reader behavior and unfamiliar-user acceptance are unverified. The later ten-minute demo does not replace hardware or long-duration acceptance. Input and any playback require explicit person-led action. No SPL calibration, room-only response, separate-device synchronization, impulse response or reverberation measurement is guaranteed.
