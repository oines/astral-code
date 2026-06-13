#!/usr/bin/env python3
"""Run a tiny SWE-bench-style comparison for Astral and Claude Code.

This is intentionally not the official SWE-bench harness. The official harness
requires Docker images and dataset setup that are too heavy for a quick local
acceptance pass. This script creates small deterministic bugfix workspaces,
runs both agents against the same failing tests, and records pass/fail plus the
resulting diff.
"""

import argparse
import json
import os
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


DEFAULT_ASTRAL_BASE_URL = "https://api.deepseek.com/v1"
DEFAULT_CLAUDE_BASE_URL = "https://api.deepseek.com/anthropic/v1"


@dataclass(frozen=True)
class TaskSpec:
    id: str
    prompt: str
    files: dict[str, str]
    test_cmd: list[str]


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
    parser.add_argument("--report-dir")
    parser.add_argument("--limit", type=int, default=1)
    parser.add_argument("--timeout-seconds", type=int, default=420)
    parser.add_argument("--skip-claude", action="store_true")
    parser.add_argument("--skip-astral", action="store_true")
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
    home: Path,
    key_env: str,
    api_key: str,
    model: str,
    timeout: int,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["ASTRAL_HOME"] = str(home)
    env[key_env] = api_key
    env.setdefault("NO_COLOR", "1")
    return run(
        [
            str(astral_bin),
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
            "-C",
            str(workdir),
            "-m",
            model,
            task.prompt,
        ],
        cwd=workdir,
        env=env,
        timeout=timeout,
    )


def run_claude(
    *,
    claude_bin: str,
    task: TaskSpec,
    workdir: Path,
    api_key: str,
    model: str,
    base_url: str,
    timeout: int,
) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["ANTHROPIC_API_KEY"] = api_key
    env["ANTHROPIC_BASE_URL"] = base_url
    env.setdefault("NO_COLOR", "1")
    return run(
        [
            claude_bin,
            "--bare",
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--tools",
            "Bash,Read,Edit,Grep,Glob,Write",
            "--dangerously-skip-permissions",
            "--no-session-persistence",
            "--model",
            model,
            task.prompt,
        ],
        cwd=workdir,
        env=env,
        timeout=timeout,
    )


def summarize_agent_result(
    *,
    agent: str,
    task: TaskSpec,
    workdir: Path,
    completed: Optional[subprocess.CompletedProcess[str]],
    before: subprocess.CompletedProcess[str],
    after: Optional[subprocess.CompletedProcess[str]],
    elapsed: float,
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
        "diff": diff.stdout,
    }


def markdown_report(results: list[dict[str, Any]], metadata: dict[str, Any]) -> str:
    lines = [
        "# Astral Mini SWE Smoke",
        "",
        "This is a small local SWE-bench-style smoke, not the official SWE-bench harness.",
        "",
        "## Metadata",
        "",
        f"- model: `{metadata['model']}`",
        f"- generated_at: `{metadata['generated_at']}`",
        f"- report_dir: `{metadata['report_dir']}`",
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
    lines.append("## Notes")
    lines.append("")
    lines.append(
        "Use the per-agent logs and git diffs in this report directory for trajectory review."
    )
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    root = repo_root()
    astral_bin = Path(args.astral_bin)
    if not astral_bin.is_absolute():
        astral_bin = root / astral_bin

    key_env, api_key = api_key_from_env(args.api_key_env)
    if not key_env or not api_key:
        print(
            "Missing API key. Set ASTRAL_ACCEPTANCE_API_KEY, ASTRAL_API_KEY, or DEEPSEEK_API_KEY.",
            file=sys.stderr,
        )
        return 2

    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    report_dir = (
        Path(args.report_dir).resolve()
        if args.report_dir
        else root / ".cache" / f"swe-smoke-{timestamp}"
    )
    report_dir.mkdir(parents=True, exist_ok=True)
    work_root = report_dir / "work"
    astral_home = report_dir / "astral-home"
    write_astral_config(astral_home, args.astral_base_url, key_env, args.model)

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

            start = time.time()
            if agent == "astral":
                completed = run_astral(
                    astral_bin=astral_bin,
                    task=task,
                    workdir=workdir,
                    home=astral_home,
                    key_env=key_env,
                    api_key=api_key,
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
                    base_url=args.claude_base_url,
                    timeout=args.timeout_seconds,
                )
            elapsed = time.time() - start
            write_log(log_path, f"{agent} run", completed)

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
