# Requirements v0.3 implementation record

The approved measurement workflow uses a 60-second viewport and up to 600 seconds of retained analysis. Signal generation is deferred; optional coherence is secondary to the main observation views. The later integrated visualizer design is documented in [AUDIO_VISUALIZER.md](AUDIO_VISUALIZER.md).

## Baseline and design decisions

- Baseline Windows Rust 1.99: core tests 31 passed (26 unit + 1 allocation + 4 lifecycle).
  One pre-existing deprecation warning (`AtomicU64::fetch_update`) must be fixed for strict lint.
- Keep capture / DSP / source / runtime / controller boundaries and bounded SPSC queues.
  Callback remains allocation-free. No async runtime or wholesale rewrite.
- F01/F08: explicit backend, stable device identity, requested rate and buffer; enumerate
  without capture, open only on Start; report unsupported conditions and actual callback size.
  An explicitly chosen missing device fails instead of silently substituting another device.
- F02/F03: retain internal reference/measurement pair, expose physical channel numbers and
  colors on both spectra, meters, and histories. Same stream only; mono is fully supported.
- F04/F05: 1024/2048/8192 FFT, Hann/rectangular windows, no averaging or time-based
  exponential power averaging. Show observation duration, bin spacing and averaging time.
  Apply analysis changes at block boundaries with a generation; reset overlap and averaging,
  discard queued results from previous generations. Fixed maximum result storage avoids
  per-window allocation. Changing FFT does allocate once on DSP worker, outside callback.
- F06: sample timeline and processing timestamp accompany results. Report age and queue
  lag separately from input latency. Flag invalid samples and clipping. A gap or route/config
  change resets averages; invalid data is not presented as a valid measurement.
- F07: retain asynchronous cancellation/retry/stop. Open only after user Start. Driver calls
  cannot be forcibly interrupted. Frozen displays retain their measured timestamp.
- F09: bounded 10 Hz history, 600 s default retention, 60 s default viewport; both channels'
  spectral bands and levels share timestamps and a cursor. Missing intervals are blank.
  Freeze the view while capture/history continue; retain history on Stop. New input/analysis
  conditions form an explicitly labeled new segment so incompatible axes never mix.
- Coherence requires at least 8 averaged windows and both signals; no single-window trust
  claim. Keep optional legacy H1/phase views with explicit same-stream limitations.
- Local settings contain route/analysis/display/condition notes only. No raw audio storage,
  network processing, app signal output, SPL calibration, IR or room-only response claims.
  AGC/noise suppression and analog gain are unknown unless manually confirmed.

## Acceptance map

| IDs | Local evidence required | Hardware / person follow-up |
| --- | --- | --- |
| F01/F08/U01 | request validation, config round trip, missing-device failure, actual metadata | M2 and built-in selection/start |
| F02/F03/A01/A02 | 48/96 kHz amplitudes -6/-20/-60, independent tones, windows, reference DFT | channel wiring |
| F04/F05/A03 | condition generation/reset, observation/bin values | interaction review |
| F06/A03 | clip/NaN/Inf/gap/stale tests; blank history | physical disconnect |
| F07/P05/U03 | cancellation, retry, stop/reconnect tests, native synthetic UI | blocked driver and person review |
| F09/U02 | bounded history, freeze, synchronized cursor, both channels, gap rendering | native layout review |
| A04 | synthetic gain and integer delay regression | no separate-device guarantee |
| P01/P02/P03/P04/A05 | synthetic timing is only a partial baseline | 100 step responses, 60 min, ASIO and built-in separately |

## Running log

- 2026-10-18: repository and Library inspection complete. Real capture/output has not been
  started. CIM machine/audio inventory denied by sandbox; enumerate read-only on a worker.
- Materialization blocked by Windows helper xattr support; direct Library read succeeded.
  This is an environment limitation, not a missing or unapproved requirement.
- Windows inventory: Win11 Education 10.0.26300, Ryzen 7 260 (8C/16T),
  32,778,732 KiB visible RAM. MOTU M Series and Realtek devices reported present.
  Enumeration does not authorize capture or output.
- Default ASIO build first failed on incomplete shared SDK cache; a complete SDK was
  found under the older 2026-10-02 task and copied, without changing that task/cache.
  Compiler execution required sandbox escalation. The asio-sys build script then
  corrupted Japanese OUT_DIR paths when importing vcvars output. Recovered using
  task-specific ASCII SDK/target directories under the real user Temp directory.
- First expanded tests exposed one history helper call missing its new channel argument;
  fixed. Settings atomic replacement failed in the sandbox's redirected package Temp,
  passed when tested under repository target/. Native Windows settings test is retained.
- A new renderer test caught a real spectrogram interval bug (subtracting the time origin
  twice), which colored a missing interval. Corrected before native QA.
- First native synthetic launch exposed main-thread stack overflow from the enlarged
  inline result arrays. Replaced them with eight preallocated Arc-backed spectral slots;
  snapshots are small handles and held views keep their slot immutable. Pool exhaustion
  skips/counts a display result rather than allocating or waiting. New tests exercise
  exhaustion, immutable held results and recovery. Allocation checks cover all FFT modes.
- Native synthetic framebuffer capture then succeeded. Desktop layout was adjusted so
  the two channel spectrograms share one row, leaving the level history visible.
- Manual rate selection now prefers the default sample format as well as channel count;
  this avoids accidentally choosing advertised U8 before F32 on WASAPI. A pure selection
  test covers this and unsupported-rate rejection.
- Result freshness uses processing age plus the queue lag captured at processing completion.
  It does not add the advancing capture clock twice, and is explicitly an estimate separate
  from end-to-end input latency. Cumulative invalid/clip counters retain brief anomalies.
- Final review: continuity counters now travel inside each result, so a later worker gap
  cannot mark an older queued result at the wrong time. Latest-value UI coalescing is
  counted separately from queue/pool loss. Startup fallback resets attempt-specific stats.
- Cursor lookup and spectral/level rendering now use the same observed intervals, never
  borrow a future value across a blank, and allow the longer normal hop cadence in precise
  mode. This avoids marking legitimate 8192-point history intervals as missing.
- Live synthetic UI regression covers both channel amplitudes/frequencies/units, HOLD
  while history advances, explicit HOLD release on changed conditions, retained boundaries,
  and reconnect with a fresh zero-generation/zero-sequence/zero-frame session.
- Disconnected UI no longer prints timing values based on an assumed 48 kHz rate;
  observation duration and bin spacing are shown only for an applied rate.
- Clipped windows invalidate comparison readiness and reset spectral averaging before
  and after the contaminated window. Clean recovery starts a fresh average. Short clip
  transitions are retained with channel-specific history flags and cursor warnings.
- Empty condition segments generated by slider motion replace one another rather than
  evicting metadata for retained measurements. A 7000-change stress regression covers it.

Detailed final validation results and limitations are in VALIDATION_V03.md.
