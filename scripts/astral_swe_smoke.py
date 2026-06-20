#!/usr/bin/env python3
"""Run a tiny SWE-bench-style comparison for Astral and Claude Code.

This is intentionally not the official SWE-bench harness. The official harness
requires Docker images and dataset setup that are too heavy for a quick local
acceptance pass. This script creates small deterministic bugfix workspaces,
runs both agents against the same failing tests, and records pass/fail plus the
resulting diff.
"""

import argparse
import contextlib
import json
import os
import signal
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any
from typing import Optional
from urllib.parse import urlsplit
from urllib.parse import urlunsplit


DEFAULT_ASTRAL_BASE_URL = "https://api.deepseek.com/v1"
DEFAULT_CLAUDE_BASE_URL = "https://api.deepseek.com/anthropic/v1"


@dataclass(frozen=True)
class TaskSpec:
    id: str
    prompt: str
    files: dict[str, str]
    test_cmd: list[str]


@dataclass(frozen=True)
class UsageRecord:
    fixture: str
    input_tokens: Optional[int]
    logical_input_tokens: Optional[int]
    cache_creation_input_tokens: Optional[int]
    cache_read_input_tokens: Optional[int]
    cache_miss_input_tokens: Optional[int]
    output_tokens: Optional[int]
    total_tokens: Optional[int]


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a small SWE-bench-style Astral vs Claude Code comparison."
    )
    parser.add_argument("--astral-bin", default="codex-rs/target/debug/astral")
    parser.add_argument("--claude-bin", default=shutil.which("claude") or "claude")
    parser.add_argument("--model", default="deepseek-v4-pro")
    parser.add_argument("--api-key-env", default="")
    parser.add_argument("--astral-base-url", default=DEFAULT_ASTRAL_BASE_URL)
    parser.add_argument("--claude-base-url", default=DEFAULT_CLAUDE_BASE_URL)
    parser.add_argument("--astral-provider", default="deepseek")
    parser.add_argument("--report-dir")
    parser.add_argument("--limit", type=int, default=1)
    parser.add_argument("--timeout-seconds", type=int, default=420)
    parser.add_argument("--skip-claude", action="store_true")
    parser.add_argument("--skip-astral", action="store_true")
    parser.add_argument(
        "--no-capture-usage",
        action="store_true",
        help="Disable local API capture proxy and omit token/cache summaries.",
    )
    return parser.parse_args()


def api_key_from_env(explicit_env: str) -> tuple[Optional[str], Optional[str]]:
    candidates = [explicit_env] if explicit_env else []
    candidates.extend(
        ["ASTRAL_ACCEPTANCE_API_KEY", "ASTRAL_API_KEY", "DEEPSEEK_API_KEY"]
    )
    for name in candidates:
        if not name:
            continue
        value = os.environ.get(name)
        if value:
            return name, value
    return None, None


def run(
    argv: list[str],
    *,
    cwd: Path,
    env: Optional[dict[str, str]] = None,
    timeout: int = 120,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            argv,
            cwd=str(cwd),
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout if isinstance(exc.stdout, str) else ""
        stderr = exc.stderr if isinstance(exc.stderr, str) else ""
        stderr = f"{stderr}\nTimed out after {timeout} seconds.\n"
        return subprocess.CompletedProcess(argv, 124, stdout=stdout, stderr=stderr)


class CaptureProxy:
    def __init__(self, upstream_base: str, dump_dir: Path) -> None:
        self.upstream_base = upstream_base
        self.dump_dir = dump_dir
        self.info_path = dump_dir / "server-info.json"
        self.stdout_path = dump_dir / "proxy.stdout.log"
        self.stderr_path = dump_dir / "proxy.stderr.log"
        self.process: Optional[subprocess.Popen[str]] = None
        self.client_base_url: Optional[str] = None

    def start(self) -> "CaptureProxy":
        self.dump_dir.mkdir(parents=True, exist_ok=True)
        upstream_origin, upstream_path = split_base_url(self.upstream_base)
        stdout = self.stdout_path.open("w", encoding="utf-8")
        stderr = self.stderr_path.open("w", encoding="utf-8")
        self.process = subprocess.Popen(
            [
                sys.executable,
                str(repo_root() / "scripts" / "trajectory_capture_proxy.py"),
                "--upstream-base",
                upstream_origin,
                "--dump-dir",
                str(self.dump_dir),
                "--server-info",
                str(self.info_path),
            ],
            cwd=str(repo_root()),
            text=True,
            stdout=stdout,
            stderr=stderr,
        )
        deadline = time.time() + 10
        while time.time() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError("capture proxy exited early")
            if self.info_path.exists():
                info = json.loads(self.info_path.read_text(encoding="utf-8"))
                self.client_base_url = f"{info['base_url']}{upstream_path}"
                return self
            time.sleep(0.05)
        raise RuntimeError("capture proxy did not start")

    def stop(self) -> None:
        if self.process is None or self.process.poll() is not None:
            return
        self.process.send_signal(signal.SIGTERM)
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)


def split_base_url(base_url: str) -> tuple[str, str]:
    parsed = urlsplit(base_url)
    origin = urlunsplit((parsed.scheme, parsed.netloc, "", "", ""))
    path = parsed.path.rstrip("/")
    if parsed.query:
        path = f"{path}?{parsed.query}"
    return origin, path


def write_log(
    path: Path, title: str, completed: subprocess.CompletedProcess[str]
) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"\n## {title}\n")
        handle.write(f"exit={completed.returncode}\n")
        handle.write("\n### stdout\n")
        handle.write(completed.stdout)
        handle.write("\n### stderr\n")
        handle.write(completed.stderr)
        handle.write("\n")


def task_specs() -> list[TaskSpec]:
    return [
        TaskSpec(
            id="median_even",
            prompt=(
                "This repository has a failing test, similar to a SWE-bench issue. "
                "Run the tests, inspect the implementation, fix the bug with the "
                "smallest correct change, rerun the tests, and stop only when they pass."
            ),
            test_cmd=[sys.executable, "-m", "unittest", "-q"],
            files={
                "calculator.py": """
                    def median(values):
                        ordered = sorted(values)
                        middle = len(ordered) // 2
                        return ordered[middle]
                    """,
                "test_calculator.py": """
                    import unittest

                    from calculator import median


                    class MedianTests(unittest.TestCase):
                        def test_odd_count(self):
                            self.assertEqual(median([3, 1, 2]), 2)

                        def test_even_count(self):
                            self.assertEqual(median([4, 1, 2, 3]), 2.5)


                    if __name__ == "__main__":
                        unittest.main()
                    """,
            },
        ),
        TaskSpec(
            id="duration_parser",
            prompt=(
                "This repository has a failing test, similar to a SWE-bench issue. "
                "Run the tests, inspect the implementation, fix parse_duration so "
                "compound hour/minute strings work, rerun the tests, and stop only "
                "when they pass."
            ),
            test_cmd=[sys.executable, "-m", "unittest", "-q"],
            files={
                "durations.py": """
                    import re


                    def parse_duration(text):
                        match = re.search(r"(\\d+)([hm])", text)
                        if not match:
                            raise ValueError(f"invalid duration: {text}")
                        value = int(match.group(1))
                        unit = match.group(2)
                        if unit == "h":
                            return value * 60
                        return value
                    """,
                "test_durations.py": """
                    import unittest

                    from durations import parse_duration


                    class DurationTests(unittest.TestCase):
                        def test_minutes(self):
                            self.assertEqual(parse_duration("45m"), 45)

                        def test_hours(self):
                            self.assertEqual(parse_duration("2h"), 120)

                        def test_compound(self):
                            self.assertEqual(parse_duration("1h30m"), 90)

                        def test_invalid(self):
                            with self.assertRaises(ValueError):
                                parse_duration("soon")


                    if __name__ == "__main__":
                        unittest.main()
                    """,
            },
        ),
    ]


def create_workspace(path: Path, task: TaskSpec) -> None:
    path.mkdir(parents=True, exist_ok=True)
    for relative, content in task.files.items():
        target = path / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")
    run(["git", "init", "-q"], cwd=path, timeout=30)
    run(["git", "add", "."], cwd=path, timeout=30)
    run(["git", "commit", "-qm", "fixture"], cwd=path, timeout=30)


def write_astral_config(home: Path, base_url: str, key_env: str, model: str) -> None:
    home.mkdir(parents=True, exist_ok=True)
    (home / "config.toml").write_text(
        textwrap.dedent(
            f"""
            model = "{model}"
            model_provider = "deepseek"
            model_context_window = 128000
            model_input_modalities = ["text"]

            [model_providers.deepseek]
            name = "DeepSeek"
            base_url = "{base_url}"
            env_key = "{key_env}"
            wire_api = "chat_completions"
            """
        ).strip()
        + "\n",
        encoding="utf-8",
    )


def run_astral(
    *,
    astral_bin: Path,
    task: TaskSpec,
    workdir: Path,
    home: Optional[Path],
    key_env: Optional[str],
    api_key: Optional[str],
    base_url: Optional[str],
    provider: str,
    model: str,
    timeout: int,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    if home is not None:
        env["ASTRAL_HOME"] = str(home)
    if key_env is not None and api_key is not None:
        env[key_env] = api_key
    env.setdefault("NO_COLOR", "1")
    argv = [
        str(astral_bin),
        "exec",
        "--json",
        "--ephemeral",
        "--skip-git-repo-check",
        "--dangerously-bypass-approvals-and-sandbox",
        "-C",
        str(workdir),
        "-m",
        model,
    ]
    if base_url is not None:
        argv.extend(
            [
                "-c",
                f"model_providers.{provider}.base_url={json.dumps(base_url)}",
            ]
        )
    argv.append(task.prompt)
    return run(argv, cwd=workdir, env=env, timeout=timeout)


def run_claude(
    *,
    claude_bin: str,
    task: TaskSpec,
    workdir: Path,
    api_key: Optional[str],
    model: str,
    base_url: str,
    timeout: int,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    if api_key is not None:
        env["ANTHROPIC_API_KEY"] = api_key
    env["ANTHROPIC_BASE_URL"] = base_url
    env.setdefault("NO_COLOR", "1")
    argv = [
        claude_bin,
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--dangerously-skip-permissions",
        "--no-session-persistence",
        "--model",
        model,
        task.prompt,
    ]
    if api_key is not None:
        argv.insert(1, "--bare")
    return run(argv, cwd=workdir, env=env, timeout=timeout)


def summarize_agent_result(
    *,
    agent: str,
    task: TaskSpec,
    workdir: Path,
    completed: Optional[subprocess.CompletedProcess[str]],
    before: subprocess.CompletedProcess[str],
    after: Optional[subprocess.CompletedProcess[str]],
    elapsed: float,
    usage: dict[str, Any],
) -> dict[str, Any]:
    diff = run(["git", "diff", "--", "."], cwd=workdir, timeout=30)
    return {
        "agent": agent,
        "task_id": task.id,
        "before_failed": before.returncode != 0,
        "agent_exit": completed.returncode if completed is not None else None,
        "after_exit": after.returncode if after is not None else None,
        "passed": bool(
            before.returncode != 0
            and completed is not None
            and completed.returncode == 0
            and after is not None
            and after.returncode == 0
        ),
        "elapsed_seconds": round(elapsed, 2),
        "usage": usage,
        "diff": diff.stdout,
    }


def fixture_paths(capture_dir: Path) -> list[Path]:
    if not capture_dir.exists():
        return []
    return [
        path
        for path in sorted(capture_dir.glob("*.json"))
        if path.name != "server-info.json"
    ]


def read_int(value: Any, key: str) -> Optional[int]:
    if not isinstance(value, dict):
        return None
    item = value.get(key)
    if isinstance(item, bool):
        return None
    if isinstance(item, int):
        return item
    return None


def read_pointer_int(value: Any, *keys: str) -> Optional[int]:
    current = value
    for key in keys:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    if isinstance(current, bool):
        return None
    if isinstance(current, int):
        return current
    return None


def first_present(*values: Optional[int]) -> Optional[int]:
    for value in values:
        if value is not None:
            return value
    return None


def normalize_usage(fixture: str, usage: dict[str, Any]) -> UsageRecord:
    prompt_cache_hit_tokens = read_int(usage, "prompt_cache_hit_tokens")
    prompt_cache_miss_tokens = read_int(usage, "prompt_cache_miss_tokens")
    input_tokens = first_present(
        read_int(usage, "input_tokens"),
        read_int(usage, "prompt_tokens"),
        (
            prompt_cache_hit_tokens + prompt_cache_miss_tokens
            if prompt_cache_hit_tokens is not None
            and prompt_cache_miss_tokens is not None
            else None
        ),
    )
    output_tokens = first_present(
        read_int(usage, "output_tokens"),
        read_int(usage, "completion_tokens"),
    )
    cache_read_input_tokens = first_present(
        read_int(usage, "cache_read_input_tokens"),
        read_pointer_int(usage, "prompt_tokens_details", "cached_tokens"),
        read_pointer_int(usage, "input_tokens_details", "cached_tokens"),
        prompt_cache_hit_tokens,
    )
    cache_creation_input_tokens = read_int(usage, "cache_creation_input_tokens")
    cache_miss_input_tokens = prompt_cache_miss_tokens
    if (
        cache_creation_input_tokens is not None
        or read_int(usage, "cache_read_input_tokens") is not None
    ):
        logical_input_tokens = sum(
            item or 0
            for item in [
                input_tokens,
                cache_creation_input_tokens,
                cache_read_input_tokens,
            ]
        )
    else:
        logical_input_tokens = input_tokens
    total_tokens = read_int(usage, "total_tokens")
    if (
        total_tokens is None
        and logical_input_tokens is not None
        and output_tokens is not None
    ):
        total_tokens = logical_input_tokens + output_tokens
    return UsageRecord(
        fixture=fixture,
        input_tokens=input_tokens,
        logical_input_tokens=logical_input_tokens,
        cache_creation_input_tokens=cache_creation_input_tokens,
        cache_read_input_tokens=cache_read_input_tokens,
        cache_miss_input_tokens=cache_miss_input_tokens,
        output_tokens=output_tokens,
        total_tokens=total_tokens,
    )


def merge_usage(current: dict[str, Any], update: dict[str, Any]) -> None:
    for key, value in update.items():
        if value is not None:
            current[key] = value


def usage_record_from_body(fixture: str, body: Any) -> Optional[UsageRecord]:
    if isinstance(body, dict):
        usage = body.get("usage")
        if isinstance(usage, dict):
            return normalize_usage(fixture, usage)
        response = body.get("response")
        if isinstance(response, dict) and isinstance(response.get("usage"), dict):
            return normalize_usage(fixture, response["usage"])
        return None

    if not isinstance(body, str) or "data:" not in body:
        return None

    merged_usage: dict[str, Any] = {}
    for raw_line in body.splitlines():
        line = raw_line.strip()
        if not line.startswith("data:"):
            continue
        payload = line.split(":", 1)[1].strip()
        if not payload or payload == "[DONE]":
            continue
        try:
            event = json.loads(payload)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict) and isinstance(event.get("usage"), dict):
            merge_usage(merged_usage, event["usage"])
        message = event.get("message") if isinstance(event, dict) else None
        if isinstance(message, dict) and isinstance(message.get("usage"), dict):
            merge_usage(merged_usage, message["usage"])
        response = event.get("response") if isinstance(event, dict) else None
        if isinstance(response, dict) and isinstance(response.get("usage"), dict):
            merge_usage(merged_usage, response["usage"])

    if not merged_usage:
        return None
    return normalize_usage(fixture, merged_usage)


def summarize_usage_capture(capture_dir: Optional[Path]) -> dict[str, Any]:
    if capture_dir is None:
        return summarize_usage_records(
            capture_dir=None,
            model_request_count=0,
            records=[],
            source="disabled",
        )

    model_request_count = 0
    records: list[UsageRecord] = []
    for path in fixture_paths(capture_dir):
        try:
            fixture = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        client_path = str(fixture.get("client_path") or "")
        if "/chat/completions" not in client_path and "/messages" not in client_path:
            continue
        model_request_count += 1
        response_body = fixture.get("response", {}).get("body")
        record = usage_record_from_body(path.name, response_body)
        if record is not None:
            records.append(record)

    return summarize_usage_records(
        capture_dir=capture_dir,
        model_request_count=model_request_count,
        records=records,
        source="capture",
    )


def summarize_usage_records(
    *,
    capture_dir: Optional[Path],
    model_request_count: int,
    records: list[UsageRecord],
    source: str,
) -> dict[str, Any]:
    def total(field: str) -> Optional[int]:
        values = [getattr(record, field) for record in records]
        known = [value for value in values if value is not None]
        if not known:
            return None
        return sum(known)

    logical_input_tokens = total("logical_input_tokens")
    cache_read_input_tokens = total("cache_read_input_tokens")
    cache_hit_rate = (
        round(cache_read_input_tokens / logical_input_tokens, 4)
        if cache_read_input_tokens is not None and logical_input_tokens
        else None
    )
    return {
        "source": source,
        "capture_dir": str(capture_dir) if capture_dir is not None else None,
        "model_request_count": model_request_count,
        "usage_response_count": len(records),
        "input_tokens": total("input_tokens"),
        "logical_input_tokens": logical_input_tokens,
        "cache_creation_input_tokens": total("cache_creation_input_tokens"),
        "cache_read_input_tokens": cache_read_input_tokens,
        "cache_miss_input_tokens": total("cache_miss_input_tokens"),
        "output_tokens": total("output_tokens"),
        "total_tokens": total("total_tokens"),
        "cache_hit_rate": cache_hit_rate,
        "records": [record.__dict__ for record in records],
    }


def summarize_usage_output(
    agent: str,
    completed: subprocess.CompletedProcess[str],
    current: dict[str, Any],
) -> dict[str, Any]:
    if current.get("usage_response_count"):
        return current

    records: list[UsageRecord] = []
    for line in completed.stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        if agent == "claude" and event.get("type") == "result":
            record = claude_usage_record_from_result(event)
            if record is not None:
                records.append(record)
        elif agent == "astral" and event.get("type") == "turn.completed":
            usage = event.get("usage")
            if isinstance(usage, dict):
                records.append(astral_usage_record_from_event(usage))

    if not records:
        return current
    return summarize_usage_records(
        capture_dir=Path(current["capture_dir"])
        if current.get("capture_dir")
        else None,
        model_request_count=current.get("model_request_count") or len(records),
        records=records,
        source="exec_stdout",
    )


def claude_usage_record_from_result(event: dict[str, Any]) -> Optional[UsageRecord]:
    model_usage = event.get("modelUsage")
    if isinstance(model_usage, dict) and model_usage:
        input_tokens = 0
        output_tokens = 0
        cache_creation_input_tokens = 0
        cache_read_input_tokens = 0
        for usage in model_usage.values():
            if not isinstance(usage, dict):
                continue
            input_tokens += int(usage.get("inputTokens") or 0)
            output_tokens += int(usage.get("outputTokens") or 0)
            cache_creation_input_tokens += int(
                usage.get("cacheCreationInputTokens") or 0
            )
            cache_read_input_tokens += int(usage.get("cacheReadInputTokens") or 0)
        logical_input_tokens = (
            input_tokens + cache_creation_input_tokens + cache_read_input_tokens
        )
        return UsageRecord(
            fixture="claude_stdout:modelUsage",
            input_tokens=input_tokens,
            logical_input_tokens=logical_input_tokens,
            cache_creation_input_tokens=cache_creation_input_tokens,
            cache_read_input_tokens=cache_read_input_tokens,
            cache_miss_input_tokens=input_tokens,
            output_tokens=output_tokens,
            total_tokens=logical_input_tokens + output_tokens,
        )

    usage = event.get("usage")
    if isinstance(usage, dict):
        return normalize_usage("claude_stdout:result", usage)
    return None


def astral_usage_record_from_event(usage: dict[str, Any]) -> UsageRecord:
    input_tokens = read_int(usage, "input_tokens")
    cache_read_input_tokens = read_int(usage, "cached_input_tokens")
    output_tokens = read_int(usage, "output_tokens")
    total_tokens = (
        input_tokens + (output_tokens or 0)
        if input_tokens is not None and output_tokens is not None
        else None
    )
    return UsageRecord(
        fixture="astral_stdout:turn.completed",
        input_tokens=input_tokens,
        logical_input_tokens=input_tokens,
        cache_creation_input_tokens=None,
        cache_read_input_tokens=cache_read_input_tokens,
        cache_miss_input_tokens=None,
        output_tokens=output_tokens,
        total_tokens=total_tokens,
    )


def format_metric(value: Any) -> str:
    if value is None:
        return "unknown"
    if isinstance(value, float):
        return f"{value:.1%}"
    return str(value)


def markdown_report(results: list[dict[str, Any]], metadata: dict[str, Any]) -> str:
    lines = [
        "# Astral Mini SWE Smoke",
        "",
        "This is a small local SWE-bench-style smoke, not the official SWE-bench harness. Token and cache metrics are parsed from captured model API responses.",
        "",
        "## Metadata",
        "",
        f"- model: `{metadata['model']}`",
        f"- generated_at: `{metadata['generated_at']}`",
        f"- report_dir: `{metadata['report_dir']}`",
        f"- usage_capture: `{metadata['usage_capture']}`",
        "",
        "## Results",
        "",
    ]
    for result in results:
        status = "PASS" if result["passed"] else "FAIL"
        lines.append(
            f"- {status}: {result['agent']} on `{result['task_id']}` "
            f"({result['elapsed_seconds']}s, agent_exit={result['agent_exit']}, "
            f"after_exit={result['after_exit']})"
        )
    lines.append("")
    lines.append("## Token and Cache Usage")
    lines.append("")
    lines.append(
        "| Agent | Task | Model requests | Usage responses | Logical input | Cache read | Cache hit rate | Output | Total |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for result in results:
        usage = result["usage"]
        lines.append(
            "| "
            f"{result['agent']} | "
            f"`{result['task_id']}` | "
            f"{format_metric(usage.get('model_request_count'))} ({usage.get('source')}) | "
            f"{format_metric(usage.get('usage_response_count'))} | "
            f"{format_metric(usage.get('logical_input_tokens'))} | "
            f"{format_metric(usage.get('cache_read_input_tokens'))} | "
            f"{format_metric(usage.get('cache_hit_rate'))} | "
            f"{format_metric(usage.get('output_tokens'))} | "
            f"{format_metric(usage.get('total_tokens'))} |"
        )
    lines.append("")
    lines.append("## Notes")
    lines.append("")
    lines.append(
        "Use the per-agent logs, capture fixtures, and git diffs in this report directory for trajectory review. `unknown` means the provider response did not include that usage field."
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    root = repo_root()
    astral_bin = Path(args.astral_bin)
    if not astral_bin.is_absolute():
        astral_bin = root / astral_bin

    key_env, api_key = api_key_from_env(args.api_key_env)

    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    report_dir = (
        Path(args.report_dir).resolve()
        if args.report_dir
        else root / ".cache" / f"swe-smoke-{timestamp}"
    )
    report_dir.mkdir(parents=True, exist_ok=True)
    work_root = report_dir / "work"
    tasks = task_specs()[: max(0, args.limit)]
    results: list[dict[str, Any]] = []

    for task in tasks:
        for agent in ("astral", "claude"):
            if agent == "astral" and args.skip_astral:
                continue
            if agent == "claude" and args.skip_claude:
                continue

            workdir = work_root / task.id / agent
            create_workspace(workdir, task)
            log_path = report_dir / f"{task.id}-{agent}.log"
            before = run(task.test_cmd, cwd=workdir, timeout=30)
            write_log(log_path, "before tests", before)

            capture: Optional[CaptureProxy] = None
            capture_dir: Optional[Path] = None
            base_url = (
                args.astral_base_url if agent == "astral" else args.claude_base_url
            )
            if not args.no_capture_usage:
                capture_dir = report_dir / "captures" / task.id / agent
                capture = CaptureProxy(base_url, capture_dir).start()
                if capture.client_base_url is None:
                    raise RuntimeError("capture proxy base URL missing")
                base_url = capture.client_base_url

            start = time.time()
            try:
                if agent == "astral":
                    if key_env is not None and api_key is not None:
                        astral_home: Optional[Path] = (
                            report_dir / "homes" / task.id / agent
                        )
                        write_astral_config(astral_home, base_url, key_env, args.model)
                        config_base_url = None
                    else:
                        astral_home = None
                        config_base_url = base_url
                    completed = run_astral(
                        astral_bin=astral_bin,
                        task=task,
                        workdir=workdir,
                        home=astral_home,
                        key_env=key_env,
                        api_key=api_key,
                        base_url=config_base_url,
                        provider=args.astral_provider,
                        model=args.model,
                        timeout=args.timeout_seconds,
                    )
                else:
                    completed = run_claude(
                        claude_bin=args.claude_bin,
                        task=task,
                        workdir=workdir,
                        api_key=api_key,
                        model=args.model,
                        base_url=base_url,
                        timeout=args.timeout_seconds,
                    )
            finally:
                if capture is not None:
                    with contextlib.suppress(Exception):
                        capture.stop()
            elapsed = time.time() - start
            write_log(log_path, f"{agent} run", completed)
            usage = summarize_usage_capture(capture_dir)
            usage = summarize_usage_output(agent, completed, usage)

            after = run(task.test_cmd, cwd=workdir, timeout=30)
            write_log(log_path, "after tests", after)
            result = summarize_agent_result(
                agent=agent,
                task=task,
                workdir=workdir,
                completed=completed,
                before=before,
                after=after,
                elapsed=elapsed,
                usage=usage,
            )
            results.append(result)
            (report_dir / f"{task.id}-{agent}.diff").write_text(
                result["diff"], encoding="utf-8"
            )

    metadata = {
        "model": args.model,
        "generated_at": datetime.now().isoformat(),
        "report_dir": str(report_dir),
        "official_swebench": False,
        "usage_capture": not args.no_capture_usage,
        "tasks": [task.id for task in tasks],
    }
    report = {"metadata": metadata, "results": results}
    (report_dir / "report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    (report_dir / "report.md").write_text(
        markdown_report(results, metadata), encoding="utf-8"
    )
    print(report_dir)
    return 1 if any(not result["passed"] for result in results) else 0


if __name__ == "__main__":
    raise SystemExit(main())
