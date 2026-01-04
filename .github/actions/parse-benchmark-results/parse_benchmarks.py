#!/usr/bin/env python3
"""Parse Criterion benchmark results and generate markdown summary."""

import json
import os
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


@dataclass
class BenchmarkResult:
    name: str
    current_mean_ns: float
    current_stddev_ns: float
    baseline_mean_ns: float | None = None
    baseline_stddev_ns: float | None = None
    change_percent: float | None = None
    change_factor: float | None = None
    status: str = "no_baseline"


def format_time(ns: float) -> str:
    """Format nanoseconds to human-readable string."""
    if ns >= 1_000_000_000:
        return f"{ns / 1_000_000_000:.2f} s"
    elif ns >= 1_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    elif ns >= 1_000:
        return f"{ns / 1_000:.2f} µs"
    else:
        return f"{ns:.0f} ns"


def parse_estimates_json(filepath: Path) -> tuple[float, float] | None:
    """Parse Criterion's estimates.json file and return (mean_ns, stddev_ns)."""
    try:
        with open(filepath) as f:
            data = json.load(f)
        mean = data.get("mean", {}).get("point_estimate")
        stddev = data.get("mean", {}).get("standard_error", 0)
        if mean is not None:
            return (float(mean), float(stddev))
    except (json.JSONDecodeError, IOError, KeyError):
        pass
    return None


def find_benchmarks(criterion_dir: Path, baseline_exists: bool) -> list[BenchmarkResult]:
    """Find all benchmark results in the Criterion output directory."""
    results = []

    if not criterion_dir.exists():
        return results

    # Find estimate files - prefer "new" (comparison run) over "main" (baseline only)
    new_files = list(criterion_dir.glob("*/*/new/estimates.json"))
    if new_files:
        estimate_files = new_files
        is_comparison = True
    else:
        estimate_files = list(criterion_dir.glob("*/*/main/estimates.json"))
        is_comparison = False

    for current_file in sorted(estimate_files):
        # Extract benchmark name from path: criterion/<bench_name>/<variant>/<new|main>/estimates.json
        parts = current_file.relative_to(criterion_dir).parts
        if len(parts) < 3:
            continue
        bench_name = parts[0]

        current_data = parse_estimates_json(current_file)
        if not current_data:
            continue

        result = BenchmarkResult(
            name=bench_name,
            current_mean_ns=current_data[0],
            current_stddev_ns=current_data[1],
        )

        # Look for baseline if this is a comparison run
        if is_comparison and baseline_exists:
            base_file = Path(str(current_file).replace("/new/", "/main/"))
            if base_file.exists():
                base_data = parse_estimates_json(base_file)
                if base_data:
                    result.baseline_mean_ns = base_data[0]
                    result.baseline_stddev_ns = base_data[1]
                    result.change_percent = (
                        (result.current_mean_ns - result.baseline_mean_ns)
                        / result.baseline_mean_ns
                        * 100
                    )
                    result.change_factor = result.current_mean_ns / result.baseline_mean_ns

        results.append(result)

    return results


def classify_results(results: list[BenchmarkResult], threshold: float) -> None:
    """Classify each benchmark result as improved/regressed/unchanged."""
    for r in results:
        if r.change_percent is None:
            r.status = "no_baseline"
        elif r.change_percent > threshold:
            r.status = "regressed"
        elif r.change_percent < -threshold:
            r.status = "improved"
        else:
            r.status = "unchanged"


def generate_json_output(
    results: list[BenchmarkResult],
    baseline_exists: bool,
    baseline_sha: str,
    threshold: float,
) -> dict:
    """Generate structured JSON output."""
    summary = {
        "total": len(results),
        "improved": sum(1 for r in results if r.status == "improved"),
        "regressed": sum(1 for r in results if r.status == "regressed"),
        "unchanged": sum(1 for r in results if r.status == "unchanged"),
    }

    benchmarks = []
    for r in results:
        entry = {
            "name": r.name,
            "current": {"mean_ns": r.current_mean_ns, "stddev_ns": r.current_stddev_ns},
            "baseline": None,
            "change_percent": r.change_percent,
            "change_factor": r.change_factor,
            "status": r.status,
        }
        if r.baseline_mean_ns is not None:
            entry["baseline"] = {
                "mean_ns": r.baseline_mean_ns,
                "stddev_ns": r.baseline_stddev_ns,
            }
        benchmarks.append(entry)

    return {
        "metadata": {
            "baseline_exists": baseline_exists,
            "baseline_sha": baseline_sha,
            "regression_threshold": threshold,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        },
        "benchmarks": benchmarks,
        "summary": summary,
    }


def generate_markdown(
    results: list[BenchmarkResult],
    baseline_exists: bool,
    baseline_sha: str,
    threshold: float,
    json_data: dict,
) -> str:
    """Generate markdown summary."""
    lines = ["## 📊 Benchmark Results", ""]

    summary = json_data["summary"]
    has_regression = summary["regressed"] > 0

    # Status header
    if not baseline_exists:
        lines.append(
            "⚠️ **No baseline available for comparison.** Results saved for future comparisons."
        )
    elif has_regression:
        lines.append(
            f"❌ **Performance regression detected!** ({summary['regressed']} benchmark(s) regressed by >{threshold}%)"
        )
    else:
        lines.append(
            f"✅ **All benchmarks within threshold** ({summary['improved']} improved, {summary['unchanged']} unchanged)"
        )
    lines.append("")

    # Comparison table
    comparisons = [r for r in results if r.baseline_mean_ns is not None]
    if comparisons:
        short_sha = baseline_sha[:7] if baseline_sha else "baseline"
        lines.append(f"### Comparison against `{short_sha}`")
        lines.append("")
        lines.append("| Benchmark | Baseline | Current | Change | Status |")
        lines.append("|-----------|----------|---------|--------|--------|")

        for r in comparisons:
            base_disp = format_time(r.baseline_mean_ns)
            curr_disp = format_time(r.current_mean_ns)

            if r.change_percent >= 0:
                change_disp = f"+{r.change_percent:.2f}% ({r.change_factor:.2f}x)"
            else:
                change_disp = f"{r.change_percent:.2f}% ({r.change_factor:.2f}x)"

            status_map = {
                "regressed": "🔴 regressed",
                "improved": "🟢 improved",
                "unchanged": "⚪ unchanged",
            }
            status_disp = status_map.get(r.status, r.status)

            lines.append(
                f"| {r.name} | {base_disp} | {curr_disp} | {change_disp} | {status_disp} |"
            )
        lines.append("")

    # Current results table
    if results:
        lines.append("### Current Results")
        lines.append("")
        lines.append("| Benchmark | Mean Time | Std Dev |")
        lines.append("|-----------|-----------|---------|")

        for r in results:
            curr_disp = format_time(r.current_mean_ns)
            stddev_disp = format_time(r.current_stddev_ns)
            lines.append(f"| {r.name} | {curr_disp} | ±{stddev_disp} |")
        lines.append("")

    lines.append("📈 Full HTML reports available in workflow artifacts.")
    lines.append("")

    # Collapsible JSON
    lines.append("<details><summary>📋 Raw JSON Results</summary>")
    lines.append("")
    lines.append("```json")
    lines.append(json.dumps(json_data, indent=2))
    lines.append("```")
    lines.append("</details>")

    return "\n".join(lines)


def main():
    # Read inputs from environment
    baseline_exists = os.environ.get("INPUT_BASELINE_EXISTS", "false").lower() == "true"
    baseline_sha = os.environ.get("INPUT_BASELINE_SHA", "")
    threshold = float(os.environ.get("INPUT_REGRESSION_THRESHOLD", "5"))
    criterion_dir = Path(os.environ.get("INPUT_CRITERION_DIR", "target/criterion"))

    print(f"📊 Parsing Criterion results from {criterion_dir}")
    print(f"   Baseline exists: {baseline_exists}")
    print(f"   Baseline SHA: {baseline_sha or '(none)'}")
    print(f"   Regression threshold: {threshold}%")

    # Parse benchmarks
    results = find_benchmarks(criterion_dir, baseline_exists)
    classify_results(results, threshold)

    print(f"   Found {len(results)} benchmark(s)")

    # Generate outputs
    json_data = generate_json_output(results, baseline_exists, baseline_sha, threshold)
    markdown = generate_markdown(results, baseline_exists, baseline_sha, threshold, json_data)

    # Write JSON results
    json_path = Path("benchmark_results.json")
    with open(json_path, "w") as f:
        json.dump(json_data, f, indent=2)
    print(f"✅ Generated {json_path}")

    # Write markdown summary
    md_path = Path("benchmark_summary.md")
    with open(md_path, "w") as f:
        f.write(markdown)
    print(f"✅ Generated {md_path}")

    # Write to GitHub outputs
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        has_regression = json_data["summary"]["regressed"] > 0
        with open(github_output, "a") as f:
            f.write(f"has_regression={'true' if has_regression else 'false'}\n")

    # Write to GitHub step summary
    github_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_summary:
        with open(github_summary, "a") as f:
            f.write(markdown)

    # Exit with error if regression detected (optional - controlled by caller)
    if json_data["summary"]["regressed"] > 0:
        print(f"⚠️  {json_data['summary']['regressed']} regression(s) detected!")
        sys.exit(0)  # Don't fail the job, just report


if __name__ == "__main__":
    main()

