"""Read a completed, local synthetic endurance trace. Never opens audio or UI."""
import argparse
import bisect
import csv
import json
import math
import statistics
from pathlib import Path


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig")) if path.exists() else None


def rows(path):
    with path.open(encoding="utf-8-sig", newline="") as stream:
        return [{k: float(v) for k, v in row.items()} for row in csv.DictReader(stream)]


def distribution(values):
    v = sorted(x for x in values if math.isfinite(x) and x >= 0)
    if not v:
        return {"n": 0}
    return {"n": len(v), "p50": v[(len(v)-1)//2], "p95": v[(len(v)-1)*95//100],
            "p99": v[(len(v)-1)*99//100], "max": v[-1],
            "over_50ms": sum(x > 50 for x in v), "over_100ms": sum(x > 100 for x in v),
            "over_500ms": sum(x > 500 for x in v), "at_least_2000ms": sum(x >= 2000 for x in v)}


def slope(records, key):
    if len(records) < 2:
        return None
    xs = [r["elapsed_s"] for r in records]
    ys = [r[key] for r in records]
    x0, y0 = statistics.mean(xs), statistics.mean(ys)
    denominator = sum((x-x0)**2 for x in xs)
    return sum((x-x0)*(y-y0) for x, y in zip(xs, ys))/denominator if denominator else None


def main(directory):
    root = Path(directory)
    frames = rows(root / "frames.csv")
    resources = rows(root / "resources.csv")
    summary = read_json(root / "summary.json")
    runner = read_json(root / "runner-result.json")
    start = read_json(root / "runner-start.json")
    with (root / "events.csv").open(encoding="utf-8", newline="") as stream:
        events = list(csv.DictReader(stream))
    history = []
    if (root / "history.csv").exists():
        with (root / "history.csv").open(encoding="utf-8", newline="") as stream:
            history = list(csv.DictReader(stream))
    active = [r for r in frames if r["elapsed_s"] >= 5 and r["minimized"] == 0]
    phases = []
    for phase in sorted({int(r["phase"]) for r in frames}):
        part = [r for r in frames if int(r["phase"]) == phase]
        phases.append({"phase": phase, "from_s": part[0]["elapsed_s"], "to_s": part[-1]["elapsed_s"],
                       "interval_ms": distribution(r["interval_ms"] for r in part),
                       "update_ms": distribution(r["update_ms"] for r in part),
                       "previous_eframe_cpu_ms": distribution(r["previous_cpu_ms"] for r in part),
                       "records_first": part[0]["records"], "records_last": part[-1]["records"]})
    holds = []
    for phase in [3, 6]:
        part = [r for r in frames if int(r["phase"]) == phase and r["held"] == 1]
        holds.append({"phase": phase, "samples": len(part),
                      "displayed_sequences": sorted({int(r["displayed_sequence"]) for r in part}),
                      "live_sequence_first": part[0]["live_sequence"] if part else None,
                      "live_sequence_last": part[-1]["live_sequence"] if part else None,
                      "history_first": part[0]["newest_s"] if part else None,
                      "history_last": part[-1]["newest_s"] if part else None})
    ordered_resources = sorted(resources, key=lambda r: r["utc_ms"])
    times = [r["utc_ms"] for r in ordered_resources]
    worst = []
    indexed = sorted(enumerate(frames), key=lambda item: item[1]["interval_ms"], reverse=True)
    for index, sample in indexed[:25]:
        pos = bisect.bisect_left(times, sample["utc_ms"])
        candidates = ordered_resources[max(0, pos-1):min(len(times), pos+1)]
        near = min(candidates, key=lambda r: abs(r["utc_ms"]-sample["utc_ms"])) if candidates else None
        worst.append({"frame_row": index, "frame": sample, "previous_frame": frames[index-1] if index else None,
                      "nearest_resource_sample": near})
    resource_info = {}
    if len(resources) > 1:
        r0, r1 = resources[0], resources[-1]
        elapsed = r1["elapsed_s"]-r0["elapsed_s"]
        cpu_percent = (r1["cpu_s"]-r0["cpu_s"])/elapsed*100
        measured_start = frames[0]["utc_ms"]
        measured_end = frames[-1]["utc_ms"]
        measured_resources = [r for r in resources if measured_start <= r["utc_ms"] <= measured_end]
        late = [r for r in measured_resources if r["elapsed_s"] >= 180]
        resource_info = {"samples": len(resources), "cpu_percent_one_core_equivalent": cpu_percent,
                         "cpu_percent_normalized": cpu_percent / start["logical_processors"],
                         "private_first_mb": r0["private_bytes"]/2**20, "private_last_mb": r1["private_bytes"]/2**20,
                         "private_peak_mb": max(r["private_bytes"] for r in resources)/2**20,
                         "working_first_mb": r0["working_bytes"]/2**20, "working_last_mb": r1["working_bytes"]/2**20,
                         "working_peak_mb": max(r["working_bytes"] for r in resources)/2**20,
                         "private_slope_mb_per_min_after_180s": (slope(late, "private_bytes") or 0)*60/2**20,
                         "handles_first": r0["handles"], "handles_last": r1["handles"],
                         "handles_min_after_5s": min(r["handles"] for r in resources if r["elapsed_s"] >= 5),
                         "handles_max_after_5s": max(r["handles"] for r in resources if r["elapsed_s"] >= 5),
                         "minimized_samples_after_5s": sum(r["minimized"] != 0 for r in measured_resources if r["elapsed_s"] >= 5),
                         "not_visible_samples_after_5s": sum(r["visible"] != 1 for r in measured_resources if r["elapsed_s"] >= 5),
                         "wall_minus_awake_delta_ms": (r1["utc_ms"]-r0["utc_ms"])-(r1["awake_ms"]-r0["awake_ms"]),
                         "uptime_minus_awake_delta_ms": (r1["uptime_ms"]-r0["uptime_ms"])-(r1["awake_ms"]-r0["awake_ms"])}
        resource_info["monitor_sample_gap_max_s"] = max(b["elapsed_s"]-a["elapsed_s"] for a, b in zip(resources, resources[1:]))
        if len(measured_resources) > 1:
            a, b = measured_resources[0], measured_resources[-1]
            measured_cpu = (b["cpu_s"]-a["cpu_s"])/(b["elapsed_s"]-a["elapsed_s"])*100
            resource_info["measured_cpu_percent_one_core_equivalent"] = measured_cpu
            resource_info["measured_cpu_percent_normalized"] = measured_cpu/start["logical_processors"]
            resource_info["measured_uptime_minus_awake_delta_ms"] = (b["uptime_ms"]-a["uptime_ms"])-(b["awake_ms"]-a["awake_ms"])
        for name, group in [("30_90s", [r for r in measured_resources if 30 <= r["elapsed_s"] < 90]),
                            ("last_60s", [r for r in measured_resources if r["utc_ms"] >= measured_end-60000])]:
            if group:
                resource_info["private_median_"+name+"_mb"] = statistics.median(r["private_bytes"] for r in group)/2**20
                resource_info["working_median_"+name+"_mb"] = statistics.median(r["working_bytes"] for r in group)/2**20
        cpu_rows = []
        for a, b in zip(resources, resources[1:]):
            dt = b["elapsed_s"]-a["elapsed_s"]
            if dt > 0:
                cpu_rows.append({"elapsed_s": b["elapsed_s"], "one_core_percent": (b["cpu_s"]-a["cpu_s"])/dt*100})
        with (root / "cpu-intervals.csv").open("w", encoding="utf-8", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=["elapsed_s", "one_core_percent"])
            writer.writeheader(); writer.writerows(cpu_rows)
    history_times = [float(r["seconds"]) for r in history]
    output = {"summary": summary, "runner": runner, "trace_rows": len(frames),
              "all_intervals_ms": distribution(r["interval_ms"] for r in frames[1:]),
              "after_5s_nonminimized_intervals_ms": distribution(r["interval_ms"] for r in active),
              "update_ms": distribution(r["update_ms"] for r in frames),
              "previous_eframe_cpu_ms": distribution(r["previous_cpu_ms"] for r in frames),
              "row_count_matches": bool(summary and summary["frames"]-summary["trace_rows_dropped"] == len(frames)),
              "viewport_shapes": sorted({(r["width"], r["height"], r["ppp"]) for r in frames}),
              "minimized_frames": sum(r["minimized"] == 1 for r in frames),
              "unknown_minimized_frames": sum(r["minimized"] < 0 for r in frames),
              "phases": phases, "holds": holds, "events": events, "resources": resource_info,
              "history": {"rows": len(history), "strictly_monotonic": all(a < b for a, b in zip(history_times, history_times[1:])),
                          "span_s": history_times[-1]-history_times[0] if history_times else None,
                          "records_at_most_6001": len(history) <= 6001,
                          "gap_records": sum(r["gap"] == "true" for r in history)},
              "worst_intervals": worst,
              "limits": ["UI update starts, not independently measured GPU presentation timestamps or audio input latency",
                         "diagnostic writer and monitoring overhead included", "tonal synthetic source, not worst-case broadband or hardware input",
                         "memory can grow while the ten-minute history fills; this run cannot establish a long post-capacity leak plateau",
                         "OS counters are coarse; absence of sleep events alone is not proof of continuous rendering"]}
    (root / "analysis.json").write_text(json.dumps(output, ensure_ascii=False, indent=2, allow_nan=False), encoding="utf-8")
    compact = {k: output[k] for k in ["summary", "runner", "trace_rows", "all_intervals_ms", "row_count_matches", "viewport_shapes", "minimized_frames", "unknown_minimized_frames", "holds", "resources", "history"]}
    print(json.dumps(compact, ensure_ascii=True, indent=2))
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        fig, axes = plt.subplots(3, 1, figsize=(13, 10), sharex=True, constrained_layout=True)
        axes[0].scatter([r["elapsed_s"] for r in frames[1:]], [r["interval_ms"] for r in frames[1:]], s=1, alpha=.35, color="#2966a3")
        axes[0].set_yscale("log"); axes[0].set_ylabel("UI interval (ms, log)")
        axes[0].axhline(50, color="#b34c32", linestyle="--", linewidth=.8, label="50 ms reference (not a guarantee)")
        axes[0].legend(loc="upper right")
        axes[1].plot([r["elapsed_s"] for r in resources], [r["private_bytes"]/2**20 for r in resources], label="Private MB")
        axes[1].plot([r["elapsed_s"] for r in resources], [r["working_bytes"]/2**20 for r in resources], label="Working set MB")
        axes[1].set_ylabel("Process memory (MiB)"); axes[1].legend(loc="upper left")
        axes[2].plot([r["elapsed_s"] for r in cpu_rows], [r["one_core_percent"] for r in cpu_rows], color="#267d55")
        axes[2].set_ylabel("CPU % (one core = 100%)"); axes[2].set_xlabel("Elapsed seconds")
        for ax in axes:
            ax.grid(alpha=.2)
            for second in [120, 180, 240, 300, 360, 420, 450, 540]:
                ax.axvline(second, color="gray", alpha=.25, linewidth=.7)
            ax.set_xlim(0, max(600, frames[-1]["elapsed_s"]))
        axes[0].set_title("Synthetic native 3D endurance | steady / orbit / reference / HOLD / live / 600s history")
        fig.savefig(root / "endurance-metrics.png", dpi=160)
        plt.close(fig)
    except ImportError as error:
        (root / "plot-unavailable.txt").write_text(str(error), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    main(parser.parse_args().directory)
