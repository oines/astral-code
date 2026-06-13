#!/usr/bin/env python3
"""Run Astral-Code acceptance smokes and trajectory comparisons.

The runner keeps secrets out of repo files. It reads an API key from the
environment, writes only temporary provider config that references an env var,
and stores redacted request fixtures through trajectory_capture_proxy.py.
"""

import argparse
import contextlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
from pathlib import Path
from typing import Any
from typing import Optional


CHAT_UPSTREAM = "https://api.deepseek.com/v1"
ANTHROPIC_UPSTREAM = "https://api.deepseek.com/anthropic/v1"
ACCEPTANCE_KEY_ENV = "ASTRAL_ACCEPTANCE_API_KEY"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Astral-Code final acceptance smokes."
    )
    parser.add_argument(
        "--astral-bin",
        default="codex-rs/target/debug/astral",
        help="Path to the astral binary to test.",
    )
    parser.add_argument(
        "--report-dir",
        help="Directory for reports and redacted trajectory captures. Defaults to a temp dir.",
    )
    parser.add_argument("--chat-model", default="deepseek-v4-flash")
    parser.add_argument("--anthropic-model", default="deepseek-v4-flash")
    parser.add_argument(
        "--api-key-env",
        default="",
        help="Explicit environment variable holding the provider API key.",
    )
    parser.add_argument(
        "--skip-real",
        action="store_true",
        help="Skip real provider E2E even when a key is present.",
    )
    parser.add_argument(
        "--run-claude",
        action="store_true",
        help="Also try to capture a Claude Code trajectory through the Anthropic proxy.",
    )
    parser.add_argument(
        "--run-rust-smokes",
        action="store_true",
        help="Run selected Rust integration tests that support the acceptance claims.",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=int,
        default=300,
        help="Timeout for each real Astral task.",
    )
    return parser.parse_args()


class Step:
    def __init__(self, name: str) -> None:
        self.name = name
        self.status = "pending"
        self.details: dict[str, Any] = {}

    def pass_(self, **details: Any) -> None:
        self.status = "pass"
        self.details.update(details)

    def fail(self, **details: Any) -> None:
        self.status = "fail"
        self.details.update(details)

    def skip(self, **details: Any) -> None:
        self.status = "skip"
        self.details.update(details)

    def to_json(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "status": self.status,
            "details": self.details,
        }


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def resolve_astral_bin(raw: str) -> Path:
    path = Path(raw)
    if not path.is_absolute():
        path = repo_root() / path
    return path


def api_key_from_env(explicit_env: str) -> tuple[Optional[str], Optional[str]]:
    candidates = [explicit_env] if explicit_env else []
    candidates.extend(["ASTRAL_API_KEY", "DEEPSEEK_API_KEY"])
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
        stdout = exc.stdout if isinstance(exc.stdout, str) else (exc.stdout or b"").decode(
            "utf-8", errors="replace"
        )
        stderr = exc.stderr if isinstance(exc.stderr, str) else (exc.stderr or b"").decode(
            "utf-8", errors="replace"
        )
        stderr = f"{stderr}\nTimed out after {timeout} seconds.\n"
        return subprocess.CompletedProcess(
            argv,
            124,
            stdout=stdout,
            stderr=stderr,
        )


def append_log(path: Path, title: str, completed: subprocess.CompletedProcess[str]) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"\n## {title}\n")
        handle.write(f"$ {' '.join(completed.args)}\n")
        handle.write(f"exit={completed.returncode}\n")
        handle.write("\n### stdout\n")
        handle.write(completed.stdout)
        handle.write("\n### stderr\n")
        handle.write(completed.stderr)
        handle.write("\n")


class CaptureProxy:
    def __init__(self, upstream_base: str, dump_dir: Path, name: str) -> None:
        self.upstream_base = upstream_base
        self.dump_dir = dump_dir
        self.name = name
        self.info_path = dump_dir / "server-info.json"
        self.stdout_path = dump_dir / "proxy.stdout.log"
        self.stderr_path = dump_dir / "proxy.stderr.log"
        self.process: Optional[subprocess.Popen[str]] = None
        self.base_url: Optional[str] = None

    def start(self) -> "CaptureProxy":
        self.dump_dir.mkdir(parents=True, exist_ok=True)
        stdout = self.stdout_path.open("w", encoding="utf-8")
        stderr = self.stderr_path.open("w", encoding="utf-8")
        self.process = subprocess.Popen(
            [
                sys.executable,
                str(repo_root() / "scripts" / "trajectory_capture_proxy.py"),
                "--upstream-base",
                self.upstream_base,
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
                raise RuntimeError(f"{self.name} proxy exited early")
            if self.info_path.exists():
                info = json.loads(self.info_path.read_text(encoding="utf-8"))
                self.base_url = info["base_url"]
                return self
            time.sleep(0.05)
        raise RuntimeError(f"{self.name} proxy did not start")

    def stop(self) -> None:
        if self.process is None or self.process.poll() is not None:
            return
        self.process.send_signal(signal.SIGTERM)
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)


def write_config(home: Path, chat_base_url: str, anthropic_base_url: str) -> None:
    home.mkdir(parents=True, exist_ok=True)
    (home / "config.toml").write_text(
        textwrap.dedent(
            f"""
            model = "deepseek-v4-flash"
            model_provider = "acceptance_chat"
            model_context_window = 128000
            model_input_modalities = ["text"]

            [model_providers.acceptance_chat]
            name = "Acceptance Chat"
            base_url = "{chat_base_url}"
            env_key = "{ACCEPTANCE_KEY_ENV}"
            wire_api = "chat_completions"

            [model_providers.acceptance_anthropic]
            name = "Acceptance Anthropic"
            base_url = "{anthropic_base_url}"
            env_key = "{ACCEPTANCE_KEY_ENV}"
            wire_api = "anthropic_messages"
            """
        ).strip()
        + "\n",
        encoding="utf-8",
    )


def create_golden_repo(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "calculator.py").write_text(
        textwrap.dedent(
            """
            def median(values):
                ordered = sorted(values)
                middle = len(ordered) // 2
                return ordered[middle]
            """
        ).lstrip(),
        encoding="utf-8",
    )
    (root / "test_calculator.py").write_text(
        textwrap.dedent(
            """
            import unittest

            from calculator import median


            class MedianTests(unittest.TestCase):
                def test_odd_count(self):
                    self.assertEqual(median([3, 1, 2]), 2)

                def test_even_count(self):
                    self.assertEqual(median([4, 1, 2, 3]), 2.5)


            if __name__ == "__main__":
                unittest.main()
            """
        ).lstrip(),
        encoding="utf-8",
    )


def create_terminal_repo(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "long_output.sh").write_text(
        "#!/bin/sh\nfor i in 1 2 3; do echo progress-$i; sleep 1; done\n",
        encoding="utf-8",
    )
    (root / "silent_then_done.sh").write_text(
        "#!/bin/sh\nsleep 3\necho silent-done\n",
        encoding="utf-8",
    )
    (root / "ask_yes.py").write_text(
        textwrap.dedent(
            """
            answer = input("Proceed? [y/N] ")
            if answer.strip().lower().startswith("y"):
                open("interactive.txt", "w", encoding="utf-8").write("got-yes\\n")
                print("accepted")
            else:
                raise SystemExit("rejected")
            """
        ).lstrip(),
        encoding="utf-8",
    )
    for script in ["long_output.sh", "silent_then_done.sh"]:
        (root / script).chmod(0o755)


def astral_env(home: Path, api_key: str) -> dict[str, str]:
    env = os.environ.copy()
    env["ASTRAL_HOME"] = str(home)
    env[ACCEPTANCE_KEY_ENV] = api_key
    env.setdefault("NO_COLOR", "1")
    return env


def run_astral_exec(
    *,
    astral_bin: Path,
    home: Path,
    api_key: str,
    cwd: Path,
    prompt: str,
    provider: str,
    model: str,
    timeout: int,
) -> subprocess.CompletedProcess[str]:
    return run(
        [
            str(astral_bin),
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
            "-C",
            str(cwd),
            "-c",
            f'model_provider="{provider}"',
            "-m",
            model,
            prompt,
        ],
        cwd=cwd,
        env=astral_env(home, api_key),
        timeout=timeout,
    )


def run_golden_task(
    *,
    step: Step,
    astral_bin: Path,
    home: Path,
    api_key: str,
    work_root: Path,
    provider: str,
    model: str,
    log_path: Path,
    timeout: int,
) -> None:
    create_golden_repo(work_root)
    before = run([sys.executable, "-m", "unittest", "-q"], cwd=work_root, timeout=30)
    append_log(log_path, f"{step.name}: before unittest", before)
    prompt = (
        "This repository has a failing unittest. Run the test, inspect the files "
        "with the available tools, fix the median implementation, rerun the test, "
        "and finish only after the test passes."
    )
    completed = run_astral_exec(
        astral_bin=astral_bin,
        home=home,
        api_key=api_key,
        cwd=work_root,
        prompt=prompt,
        provider=provider,
        model=model,
        timeout=timeout,
    )
    append_log(log_path, f"{step.name}: astral exec", completed)
    after = run([sys.executable, "-m", "unittest", "-q"], cwd=work_root, timeout=30)
    append_log(log_path, f"{step.name}: after unittest", after)
    fixed_source = (work_root / "calculator.py").read_text(encoding="utf-8")
    if before.returncode == 0:
        step.fail(reason="fixture did not fail before Astral ran")
    elif completed.returncode != 0:
        step.fail(reason="astral exec failed", returncode=completed.returncode)
    elif after.returncode != 0:
        step.fail(reason="unittest still failing", stdout=after.stdout[-1000:])
    elif "2.5" not in fixed_source and "/ 2" not in fixed_source:
        step.fail(reason="test passed but source does not show an obvious even median fix")
    else:
        step.pass_(workdir=str(work_root))


def run_terminal_task(
    *,
    step: Step,
    astral_bin: Path,
    home: Path,
    api_key: str,
    work_root: Path,
    provider: str,
    model: str,
    log_path: Path,
    timeout: int,
) -> None:
    create_terminal_repo(work_root)
    prompt = (
        "Exercise terminal control in this temporary repo. "
        "1. Run ./long_output.sh and observe progress. "
        "2. Start ./silent_then_done.sh as a background task, use ReadTaskOutput "
        "until it finishes or prints silent-done. "
        "3. Start python3 ask_yes.py in a tty/background task, use "
        "ReadTaskOutput to see the prompt, SendTaskInput with y followed by a "
        "newline, and confirm interactive.txt contains got-yes. "
        "4. Start a long-running background shell loop, use ListBackgroundTasks "
        "to find it, then StopBackgroundTask to stop it. "
        "Write terminal_acceptance.txt with terminal-ok when all steps are done."
    )
    completed = run_astral_exec(
        astral_bin=astral_bin,
        home=home,
        api_key=api_key,
        cwd=work_root,
        prompt=prompt,
        provider=provider,
        model=model,
        timeout=timeout,
    )
    append_log(log_path, f"{step.name}: astral exec", completed)
    terminal_file = work_root / "terminal_acceptance.txt"
    interactive_file = work_root / "interactive.txt"
    if completed.returncode != 0:
        step.fail(reason="astral exec failed", returncode=completed.returncode)
        return
    if not terminal_file.exists():
        step.fail(reason="terminal_acceptance.txt was not created")
        return
    if "terminal-ok" not in terminal_file.read_text(encoding="utf-8"):
        step.fail(reason="terminal_acceptance.txt did not contain terminal-ok")
        return
    if not interactive_file.exists() or "got-yes" not in interactive_file.read_text(
        encoding="utf-8"
    ):
        step.fail(reason="interactive stdin fixture did not complete")
        return
    step.pass_(workdir=str(work_root))


def summarize_capture(dump_dir: Path, output_path: Path) -> subprocess.CompletedProcess[str]:
    return run(
        [
            sys.executable,
            str(repo_root() / "scripts" / "trajectory_summarize.py"),
            str(dump_dir),
            "--output",
            str(output_path),
        ],
        cwd=repo_root(),
        timeout=60,
    )


def run_claude_probe(
    *,
    step: Step,
    anthropic_proxy: CaptureProxy,
    api_key: str,
    model: str,
    report_dir: Path,
    log_path: Path,
    timeout: int,
) -> None:
    claude = shutil.which("claude") or "/opt/homebrew/bin/claude"
    if not Path(claude).exists():
        step.skip(reason="Claude Code CLI not found")
        return
    if anthropic_proxy.base_url is None:
        step.skip(reason="Anthropic proxy did not start")
        return
    env = os.environ.copy()
    env["ANTHROPIC_BASE_URL"] = anthropic_proxy.base_url
    env["ANTHROPIC_API_KEY"] = api_key
    prompt = (
        "Use Bash to print the current directory and then answer with one short "
        "sentence. Do not modify files."
    )
    completed = run(
        [
            claude,
            "--bare",
            "-p",
            "--output-format",
            "stream-json",
            "--tools",
            "Bash,Read,Edit,Grep,Glob",
            "--dangerously-skip-permissions",
            "--model",
            model,
            prompt,
        ],
        cwd=report_dir,
        env=env,
        timeout=timeout,
    )
    append_log(log_path, "claude trajectory probe", completed)
    if completed.returncode == 0:
        step.pass_(capture_dir=str(anthropic_proxy.dump_dir))
    else:
        step.fail(reason="Claude Code probe failed", returncode=completed.returncode)


def local_smokes(
    *,
    steps: list[Step],
    astral_bin: Path,
    report_dir: Path,
    log_path: Path,
    run_rust_smokes: bool,
) -> None:
    version_step = Step("local: astral binary and host commands")
    completed = run([str(astral_bin), "--version"], cwd=repo_root(), timeout=30)
    append_log(log_path, "astral --version", completed)
    if completed.returncode != 0:
        version_step.fail(reason="astral --version failed")
    else:
        mcp = run([str(astral_bin), "mcp-server", "--help"], cwd=repo_root(), timeout=30)
        plugin = run([str(astral_bin), "plugin", "list"], cwd=repo_root(), timeout=30)
        append_log(log_path, "astral mcp-server --help", mcp)
        append_log(log_path, "astral plugin list", plugin)
        if mcp.returncode == 0 and plugin.returncode == 0:
            version_step.pass_(version=completed.stdout.strip())
        else:
            version_step.fail(reason="mcp-server or plugin smoke failed")
    steps.append(version_step)

    py_step = Step("local: trajectory scripts compile")
    compile_result = run(
        [
            sys.executable,
            "-m",
            "py_compile",
            "scripts/trajectory_capture_proxy.py",
            "scripts/trajectory_summarize.py",
            "scripts/trajectory_diff.py",
            "scripts/astral_acceptance.py",
        ],
        cwd=repo_root(),
        timeout=60,
    )
    append_log(log_path, "py_compile trajectory scripts", compile_result)
    if compile_result.returncode == 0:
        py_step.pass_()
    else:
        py_step.fail(reason="py_compile failed")
    steps.append(py_step)

    if not run_rust_smokes:
        rust_step = Step("local: selected Rust smokes")
        rust_step.skip(reason="pass --run-rust-smokes to execute selected Rust tests")
        steps.append(rust_step)
        return

    rust_step = Step("local: selected Rust smokes")
    rust_result = run(
        [
            "just",
            "test",
            "-p",
            "codex-core",
            "astral_background_task_tools_round_trip_through_unified_exec",
        ],
        cwd=repo_root() / "codex-rs",
        timeout=240,
    )
    append_log(log_path, "codex-core background task smoke", rust_result)
    if rust_result.returncode == 0:
        rust_step.pass_()
    else:
        rust_step.fail(reason="selected Rust smoke failed")
    steps.append(rust_step)


def write_reports(report_dir: Path, steps: list[Step], metadata: dict[str, Any]) -> None:
    report = {
        "metadata": metadata,
        "steps": [step.to_json() for step in steps],
    }
    report_path = report_dir / "acceptance-report.json"
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

    lines = ["# Astral-Code Acceptance Report", ""]
    for step in steps:
        lines.append(f"- {step.status.upper()}: {step.name}")
        if step.details:
            detail = ", ".join(f"{key}={value}" for key, value in sorted(step.details.items()))
            lines.append(f"  {detail}")
    lines.append("")
    lines.append("## SFT Shape Notes")
    lines.append(
        "Astral should be judged by the model-visible request shape captured in "
        "trajectory summaries, not by client-side implementation names."
    )
    lines.append(
        "Expected similarity: Bash/Read/Edit/Grep/Glob tool loop, tool_use -> "
        "tool_result continuation, and coding task rhythm should match Claude Code closely."
    )
    lines.append(
        "Expected intentional differences: Codex local compact, Goal/Plan host modes, "
        "approval/sandbox events, and Astral's ReadTaskOutput/SendTaskInput/"
        "ListBackgroundTasks/StopBackgroundTask terminal controls."
    )
    (report_dir / "acceptance-report.md").write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    args = parse_args()
    astral_bin = resolve_astral_bin(args.astral_bin)
    report_dir = (
        Path(args.report_dir).resolve()
        if args.report_dir
        else Path(tempfile.mkdtemp(prefix="astral-acceptance-"))
    )
    report_dir.mkdir(parents=True, exist_ok=True)
    log_path = report_dir / "commands.log"
    steps: list[Step] = []
    metadata: dict[str, Any] = {
        "repo": str(repo_root()),
        "report_dir": str(report_dir),
        "astral_bin": str(astral_bin),
        "chat_upstream": CHAT_UPSTREAM,
        "anthropic_upstream": ANTHROPIC_UPSTREAM,
    }

    key_env, api_key = api_key_from_env(args.api_key_env)
    metadata["api_key_env"] = key_env
    metadata["real_provider_enabled"] = bool(api_key and not args.skip_real)

    local_smokes(
        steps=steps,
        astral_bin=astral_bin,
        report_dir=report_dir,
        log_path=log_path,
        run_rust_smokes=args.run_rust_smokes,
    )

    if args.skip_real or not api_key:
        skipped = Step("real provider P0")
        skipped.skip(
            reason="missing API key env or --skip-real set",
            expected_env="ASTRAL_API_KEY or DEEPSEEK_API_KEY",
        )
        steps.append(skipped)
        write_reports(report_dir, steps, metadata)
        print(report_dir)
        return 0

    chat_proxy = CaptureProxy(CHAT_UPSTREAM, report_dir / "captures" / "chat", "chat")
    anthropic_proxy = CaptureProxy(
        ANTHROPIC_UPSTREAM,
        report_dir / "captures" / "anthropic",
        "anthropic",
    )
    try:
        chat_proxy.start()
        anthropic_proxy.start()
        if chat_proxy.base_url is None or anthropic_proxy.base_url is None:
            raise RuntimeError("capture proxy base URL missing")

        home = report_dir / "astral-home"
        write_config(home, chat_proxy.base_url, anthropic_proxy.base_url)

        chat_step = Step("P0: golden code task via chat_completions")
        run_golden_task(
            step=chat_step,
            astral_bin=astral_bin,
            home=home,
            api_key=api_key,
            work_root=report_dir / "work" / "golden-chat",
            provider="acceptance_chat",
            model=args.chat_model,
            log_path=log_path,
            timeout=args.timeout_seconds,
        )
        steps.append(chat_step)

        anthropic_step = Step("P0: golden code task via anthropic_messages")
        run_golden_task(
            step=anthropic_step,
            astral_bin=astral_bin,
            home=home,
            api_key=api_key,
            work_root=report_dir / "work" / "golden-anthropic",
            provider="acceptance_anthropic",
            model=args.anthropic_model,
            log_path=log_path,
            timeout=args.timeout_seconds,
        )
        steps.append(anthropic_step)

        terminal_step = Step("P0: terminal agentic task")
        run_terminal_task(
            step=terminal_step,
            astral_bin=astral_bin,
            home=home,
            api_key=api_key,
            work_root=report_dir / "work" / "terminal-chat",
            provider="acceptance_chat",
            model=args.chat_model,
            log_path=log_path,
            timeout=args.timeout_seconds,
        )
        steps.append(terminal_step)

        chat_summary = report_dir / "chat-summary.json"
        anthropic_summary = report_dir / "anthropic-summary.json"
        summary_step = Step("P0: trajectory summaries")
        chat_summary_result = summarize_capture(chat_proxy.dump_dir, chat_summary)
        anthropic_summary_result = summarize_capture(anthropic_proxy.dump_dir, anthropic_summary)
        append_log(log_path, "summarize chat trajectory", chat_summary_result)
        append_log(log_path, "summarize anthropic trajectory", anthropic_summary_result)
        if chat_summary_result.returncode == 0 and anthropic_summary_result.returncode == 0:
            summary_step.pass_(
                chat_summary=str(chat_summary),
                anthropic_summary=str(anthropic_summary),
            )
        else:
            summary_step.fail(reason="trajectory summarize failed")
        steps.append(summary_step)

        if args.run_claude:
            claude_step = Step("P0: Claude Code trajectory probe")
            run_claude_probe(
                step=claude_step,
                anthropic_proxy=anthropic_proxy,
                api_key=api_key,
                model=args.anthropic_model,
                report_dir=report_dir,
                log_path=log_path,
                timeout=args.timeout_seconds,
            )
            steps.append(claude_step)
        else:
            claude_step = Step("P0: Claude Code trajectory probe")
            claude_step.skip(reason="pass --run-claude to capture Claude Code")
            steps.append(claude_step)
    finally:
        with contextlib.suppress(Exception):
            chat_proxy.stop()
        with contextlib.suppress(Exception):
            anthropic_proxy.stop()

    write_reports(report_dir, steps, metadata)
    print(report_dir)
    return 1 if any(step.status == "fail" for step in steps) else 0


if __name__ == "__main__":
    raise SystemExit(main())
