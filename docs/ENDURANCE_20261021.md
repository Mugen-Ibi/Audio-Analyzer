# Synthetic 3D endurance — 2026-10-21

## Scope and preservation

One fixed 600-second native run, synthetic input only. No microphone capture, output stream, PCM recording or system-setting change was performed. The production release tag contains the product source; this diagnostic branch adds only the bounded measurement fixture and aggregate report. The original local implementation and diagnostic commits remain preserved locally; public snapshots remove private transfer records and machine identifiers.

The application clock starts just after native construction, separately from the runner clock. The runner permits at most 630 seconds before a scoped close request, then at most eight seconds before terminating only its own process if necessary. No repeat of the ten-minute test was performed for publication.

## Method

Rust/eframe 0.30, existing native renderer and synthetic source, 48 kHz/two inputs/1024-point Hann/no averaging/256-frame synthetic blocks. The existing 750/1500 Hz tones and three-second amplitude steps are unchanged. The active view remains 3D for the entire measurement. The reference comparison workload includes the 3D dashed overlay and the existing optional same-window spectrum UI; this is not a separate 2D-view endurance test.

| App elapsed time | Scheduled state transition |
| --- | --- |
| 0–120 s | Steady live 3D, 60-second visible span |
| 120 s | Begin continuously changing yaw, pitch and zoom through the same camera/projection path |
| 180 s | Capture reference and enable comparison |
| 240 s | HOLD; input and retained history continue |
| 300 s | LIVE; camera remains at its last view |
| 360 s | 600-second visible history and continuous camera movement |
| 420 s | HOLD while camera movement continues |
| 450 s | LIVE |
| 540 s | Reset camera and hold a steady live view |
| 600 s | End interval collection, write result/history metadata, request final screenshot and close |

No deliberate minimize, resize or screenshot occurs during the 600-second measurement. The runner shows only the uniquely identified native window of the launched PID without foreground activation. Ordinary mouse gestures are already covered by earlier UI tests; the scheduled stress actions directly use the same camera, HOLD and reference state methods, without global input injection.

`frames.csv` records every update callback, including start-to-start interval, current app-update duration, eframe's reported previous-frame CPU/render duration, UTC and OS clocks, viewport size/scale/focus/minimize state, HOLD/reference state, retained history bounds and count, discontinuities and result/stream counters. A bounded 8192-item queue (~2 MiB) sends fixed-size numeric records to a buffered writer thread. CSV formatting and flushing occur off the UI thread. Any lost frame-log rows are counted. `rebuild_ms` is the most recent cached geometry rebuild duration, not a GPU timestamp or necessarily a rebuild on that row.

`resources.csv` samples the launched process once per second: accumulated process CPU time, working set, private/virtual bytes, handles, threads, native visibility/minimize/client rectangle/DPI, primary screen metrics, uptime and awake-time counters. The monitor process's Win32 coordinates can be DPI-virtualized; application logical dimensions × pixels-per-point and the final PNG are the renderer size evidence. Monitoring and diagnostic overhead are included.

Sleep evidence uses raw deltas rather than guessing from a UI pause. Microsoft's [QueryUnbiasedInterruptTime](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryunbiasedinterrupttime) excludes sleep/hibernation, while [GetTickCount64](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-gettickcount64) reports uptime. Both are coarse counters; small differences are not evidence of sleep. A bounded, read-only System-log query also requests Kernel-Power and Power-Troubleshooter events for this run. The runner never changes logging or power settings.

## Verification and interpretation limits

Before this run: format and strict all-target/all-feature Clippy passed; all 57 library unit tests passed, including the new finite schedule test and existing UI/pipeline/synthetic lifecycle tests; Windows all-feature release build passed. Two command-line guard checks rejected endurance without `--demo` and endurance combined with short screenshot QA, before source/stream creation. The PowerShell runner parsed successfully.

Analysis uses all recorded intervals, plus a separate after-five-seconds/nonminimized distribution, with percentile rank `floor((n−1) × p)`. Per-phase statistics and the 25 largest intervals retain neighboring frame data and the nearest resource sample. Both HOLD periods are checked for a fixed displayed sequence and advancing live sequence/history. The final history is checked for monotonic timestamps, bounded count and span; missing records are reported rather than filled.

This measures native UI update cadence and CPU/resource behavior, not independent GPU presentation timestamps, GPU utilization/VRAM, or physical audio latency. A visible, unminimized window is not proof of freedom from occlusion or all display-power events. The tonal demo is not the maximum broadband mesh workload. The ten-minute history is still filling for much of the run, so increasing memory alone cannot establish a leak, and this run cannot establish a long post-capacity plateau. A non-reproduction cannot identify the cause of the previous 2.4-second interval or guarantee its absence on other devices, loads, drivers or Windows sessions.

## Results of the single fixed run

The measurement completed at 600.0052785 app seconds. The process exited 0; total runner duration was 601.1369 seconds. Neither watchdog close nor kill was used. The application error log is empty. All nine scheduled steps ran. Exactly 85,490 frame-log rows were written, with zero diagnostic row loss.

| UI update interval, 85,489 noninitial samples | ms |
| --- | ---: |
| Median | 6.9610 |
| p95 | 7.7820 |
| p99 | 8.9565 |
| Maximum | 70.2818 |

There were two intervals over 50 ms, zero over 100 ms, and zero at or above 2 seconds. The previous approximately 2.4-second pause was **not reproduced in this one run**. This does not identify its cause and is not a 50 ms worst-case guarantee.

| State / app seconds | Interval p95 / maximum, ms | Mean process CPU, one core = 100% |
| --- | ---: | ---: |
| Steady, 0–120 | 7.605 / 70.282 | 33.0% (CPU window excludes first 5 s) |
| Orbit, 120–180 | 7.628 / 13.520 | 39.6% |
| Orbit + reference, 180–240 | 7.730 / 14.378 | 42.8% |
| HOLD + orbit, 240–300 | 7.653 / 50.832 | 47.3% |
| LIVE + reference, 300–360 | 7.340 / 8.982 | 34.2% |
| 600 s span + orbit, 360–420 | 7.321 / 18.284 | 76.3% |
| HOLD + 600 s span + orbit, 420–450 | 8.026 / 23.593 | 87.9% |
| LIVE + 600 s span + orbit, 450–540 | 8.896 / 36.144 | 92.9% |
| Reset/steady + 600 s span, 540–600 | 7.597 / 21.198 | 79.6% |

CPU phase windows use the runner's one-second samples and can differ by roughly one sample from the app-clock phase boundaries. The heavier 600-second view costs more CPU even though interval percentiles stayed low. Overall measured-window process CPU averaged 56.87% of one core. These include the diagnostic writer and synthetic analysis; GPU utilization/VRAM were not measured.

### Largest intervals and environment correlation

| App seconds | Interval | Current App::update | Previous-frame eframe CPU/render | Awake-counter interval | State |
| --- | ---: | ---: | ---: | ---: | --- |
| 59.481662 | 70.282 ms | 0.422 ms | 0.777 ms | 70.101 ms | Steady, not minimized |
| 294.477754 | 50.832 ms | 1.162 ms | 1.200 ms | 51.483 ms | HOLD + orbit, not minimized |

Neither event was at a scheduled state transition. No screenshot was requested before measurement completion. The measurement-end event is at 600.006221 s, screenshot request at 600.009173 s, and PNG completion at 600.034356 s. Thus this run's two largest intervals did not coincide with our capture operation.

All frame records reported not minimized; all measured monitor samples after startup reported visible and not minimized. The monitor's largest sample gap was 1.0681 s. Across samples within the measured frame window, uptime minus unbiased/awake-time delta was −1.702 ms, within coarse-counter/sampling effects rather than evidence of a sleep interval. The two largest UI gaps also advanced the awake clock by similar amounts. These counters do not suggest sleep for those gaps.

**System event-log read was denied.** The attempted target was the local System log, Kernel-Power/Power-Troubleshooter providers, bounded to this run. The returned reason was `Attempted to perform an unauthorized operation.` The raw local log is excluded from public distribution. No elevated retry or logging/power-setting change was attempted. Therefore event-log-based confirmation is unavailable; the counter observations are not a replacement for it.

A native viewport-size change was observed late in the run, without a runner resize command. It did not coincide with either over-50-ms interval; its actor/cause is unknown. No code-caused stall was isolated, so no speculative production performance fix was made.

### History, HOLD and resource trend

- The final history has 5,623 monotonically ordered records, spanning 0.118199–599.990199 app-clock seconds (599.872 s), below the 6,001-record bound. Nominal history recording is capped at 10 Hz and quantized by FFT hops; it does not imply exactly 6,000 records in 600 seconds.
- There are 15 retained gap markers. Synthetic audio-frame drops, invalid samples and stream errors are all zero. Display-result queue drops end at 20 (16 already present in the first frame record), and latest-result coalescing ends at 110. These are different counters; the history is not represented as complete continuous samples across its gaps.
- During the first HOLD, 8,614 UI records kept displayed sequence 22,520 while live sequence advanced to 28,144 and history advanced from 239.979533 to 299.926199 s. During the second HOLD, 4,269 UI records kept displayed sequence 39,394 while live reached 42,206 and history advanced from 419.936866 to 449.920866 s. Both display freezes preserved continued history collection.
- Private-memory median grew from 153.34 MiB in runner seconds 30–90 to 173.86 MiB in the last measured minute (+20.51 MiB); working-set median grew from 146.50 to 152.60 MiB (+6.11 MiB). Peaks over the process observation were 181.46 MiB private and 159.38 MiB working set. Handles after startup ranged 324–340 and ended at 328. The private-memory fitted slope after 180 s is about +2.98 MiB/min. History filling, visible-span changes, geometry/native buffer caching and viewport changes coexist in this run; their individual contributions were not profiled. This is **not evidence sufficient to declare either a leak or a stable long-term plateau**.

## Evidence and reproduction

Raw run evidence was retained locally and is excluded from public assets: `frames.csv`, `events.csv`, `resources.csv`, `history.csv`, `summary.json`, runner start/result JSON, `analysis.json`, `cpu-intervals.csv`, app stdout/stderr, final native `final.png`, and the denied System-log query message. The final PNG was opened and inspected: live 3D, a 600-second span, reference comparison and synchronized RMS history are present. No PCM was written.

`scripts/analyze-endurance.py` reproduces the numeric analysis. Its optional time-series plot was not created because this Python has no `matplotlib`; that limitation is recorded in `plot-unavailable.txt`. No package was installed, and no data was sent outside this PC.

```powershell
# Use a fresh, nonexistent output folder; only run if another fixed trial is requested.
.\scripts\endurance-qa.ps1 -Executable .\target\diagnostics\AudioVisualizer-endurance.exe -OutputDirectory "$PWD\target\endurance\new-run-name"
python .\scripts\analyze-endurance.py .\target\endurance\new-run-name
```

The earlier production delivery was preserved. The public product tag and this separate diagnostic branch retain both source snapshots. No automatic repeat or hardware measurement was performed.

## Public source and limitations

This branch preserves the diagnostic source and aggregate measurements. Raw per-frame/resource CSV files, screenshots containing local environment information, OS event logs, private paths and transfer receipts are excluded. The diagnostic executable is not the production release asset. Refer to the product release record for usage and current limitations. A single finite synthetic test does not certify physical microphone latency, all display environments or long-term resource stability. No further performance investigation was performed for publication.
