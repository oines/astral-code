#!/usr/bin/env python3
"""Run a compact/trajectory gauntlet against Astral.

The gauntlet validates the model-visible request shape, not just final output:

- starts a local capture proxy in front of the provider API
- runs a multi-step coding and terminal task
- resumes the same session with a tiny auto-compact threshold
- inspects captured requests for compact, tools, tool calls, and context growth
- writes a report that calls out rough spots in the agent trajectory
"""

import argparse
import contextlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
from collections import Counter
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any
from typing import Optional


DEFAULT_UPSTREAM = "https://api.deepseek.com/v1"


@dataclass
class RunResult:
    name: str
    completed: subprocess.CompletedProcess[str]
    elapsed_seconds: float


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run an Astral long-task gauntlet with API trajectory capture."
    )
    parser.add_argument("--astral-bin", default="codex-rs/target/debug/astral")
    parser.add_argument("--model", default="deepseek-v4-pro")
    parser.add_argument("--api-key-env", default="")
    parser.add_argument("--upstream-base", default=DEFAULT_UPSTREAM)
    parser.add_argument("--report-dir")
    parser.add_argument("--timeout-seconds", type=int, default=540)
    parser.add_argument("--skip-resume", action="store_true")
    parser.add_argument(
        "--include-image",
        action="store_true",
        help="Attach the tiny PNG fixture to phase 1. Disabled by default so CLI prompt stdin is the primary path.",
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
    stdin: Optional[str] = None,
    timeout: int = 120,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            argv,
            cwd=str(cwd),
            env=env,
            input=stdin,
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


def append_log(
    path: Path, title: str, completed: subprocess.CompletedProcess[str]
) -> None:
    with path.open("a", encoding="utf-8") as handle:
        handle.write(f"\n## {title}\n")
        handle.write(f"$ {' '.join(str(arg) for arg in completed.args)}\n")
        handle.write(f"exit={completed.returncode}\n")
        handle.write("\n### stdout\n")
        handle.write(completed.stdout)
        handle.write("\n### stderr\n")
        handle.write(completed.stderr)
        handle.write("\n")


class CaptureProxy:
    def __init__(self, upstream_base: str, dump_dir: Path) -> None:
        self.upstream_base = upstream_base
        self.dump_dir = dump_dir
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
                raise RuntimeError("capture proxy exited early")
            if self.info_path.exists():
                info = json.loads(self.info_path.read_text(encoding="utf-8"))
                self.base_url = info["base_url"]
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


def write_config(home: Path, base_url: str, key_env: str, model: str) -> None:
    home.mkdir(parents=True, exist_ok=True)
    (home / "config.toml").write_text(
        textwrap.dedent(
            f"""
            model = "{model}"
            model_provider = "gauntlet_deepseek"
            model_context_window = 20000
            model_auto_compact_token_limit = 3500
            model_auto_compact_token_limit_scope = "body_after_prefix"
            model_input_modalities = ["text"]

            [model_providers.gauntlet_deepseek]
            name = "Gauntlet DeepSeek"
            base_url = "{base_url}"
            env_key = "{key_env}"
            wire_api = "chat_completions"
            """
        ).strip()
        + "\n",
        encoding="utf-8",
    )


def long_policy_text() -> str:
    sections: list[str] = []
    for idx in range(1, 46):
        sections.append(
            f"""
            ## Policy Section {idx}
            The agent must prefer small, reviewable code changes. For billing
            rules, a base charge is always applied before account-specific
            modifiers. Terminal checks must write durable marker files so a
            resumed agent can verify state without guessing. This section is
            intentionally repetitive to create realistic context pressure.
            """
        )
    sections.append(
        """
        ## Hidden Acceptance Rules
        The final report must include the exact phrase GAUNTLET_RULE_ALPHA.
        The base charge is 20.
        Enterprise accounts receive a 15 percent discount after the base charge.
        Retry accounts add a flat fee of 7 after all percentage modifiers.
        The final validation file must be named compact_survived.txt.
        """
    )
    return "\n".join(textwrap.dedent(section).strip() for section in sections) + "\n"


def create_workspace(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    (root / "docs").mkdir(exist_ok=True)
    (root / "scripts").mkdir(exist_ok=True)
    (root / "src").mkdir(exist_ok=True)
    (root / "assets").mkdir(exist_ok=True)
    (root / "docs" / "billing_policy.md").write_text(
        long_policy_text(), encoding="utf-8"
    )
    (root / "src" / "billing.py").write_text(
        textwrap.dedent(
            """
            from dataclasses import dataclass


            @dataclass
            class Invoice:
                account_type: str
                subtotal: float
                retry: bool = False


            def calculate_total(invoice: Invoice) -> float:
                total = invoice.subtotal
                if invoice.account_type == "enterprise":
                    total *= 0.85
                if invoice.retry:
                    total += 7
                return round(total, 2)


            def parse_duration(text: str) -> int:
                if text.endswith("m"):
                    return int(text[:-1])
                if text.endswith("h"):
                    return int(text[:-1]) * 60
                raise ValueError(f"invalid duration: {text}")
            """
        ).lstrip(),
        encoding="utf-8",
    )
    (root / "test_billing.py").write_text(
        textwrap.dedent(
            """
            import unittest

            from src.billing import Invoice, calculate_total, parse_duration


            class BillingTests(unittest.TestCase):
                def test_enterprise_base_charge_discount_and_retry_fee(self):
                    invoice = Invoice(account_type="enterprise", subtotal=100, retry=True)
                    self.assertEqual(calculate_total(invoice), 109.0)

                def test_standard_base_charge_and_retry_fee(self):
                    invoice = Invoice(account_type="standard", subtotal=100, retry=True)
                    self.assertEqual(calculate_total(invoice), 127.0)

                def test_compound_duration(self):
                    self.assertEqual(parse_duration("1h30m"), 90)


            if __name__ == "__main__":
                unittest.main()
            """
        ).lstrip(),
        encoding="utf-8",
    )
    (root / "scripts" / "progress.sh").write_text(
        "#!/bin/sh\nfor i in 1 2 3; do echo progress-$i; sleep 1; done\n",
        encoding="utf-8",
    )
    (root / "scripts" / "silent_then_done.sh").write_text(
        "#!/bin/sh\nsleep 3\necho silent-done\n",
        encoding="utf-8",
    )
    (root / "scripts" / "ask_yes.py").write_text(
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
    (root / "scripts" / "never_end.sh").write_text(
        "#!/bin/sh\nwhile true; do echo still-running; sleep 5; done\n",
        encoding="utf-8",
    )
    for script in [
        "progress.sh",
        "silent_then_done.sh",
        "never_end.sh",
    ]:
        (root / "scripts" / script).chmod(0o755)
    # Tiny PNG header-ish fixture. It only needs to exercise image attachment downgrade.
    (root / "assets" / "tiny.png").write_bytes(
        bytes.fromhex(
            "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de"
            "0000000c49444154789c63606060000000040001f61738550000000049454e44ae426082"
        )
    )
    run(["git", "init", "-q"], cwd=root, timeout=30)
    run(["git", "add", "."], cwd=root, timeout=30)
    run(["git", "commit", "-qm", "gauntlet fixture"], cwd=root, timeout=30)


def astral_env(home: Path, key_env: str, api_key: str) -> dict[str, str]:
    env = os.environ.copy()
    env["ASTRAL_HOME"] = str(home)
    env[key_env] = api_key
    env.setdefault("NO_COLOR", "1")
    return env


def extract_thread_id(stdout: str) -> Optional[str]:
    for line in stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "thread.started" and event.get("thread_id"):
            return str(event["thread_id"])
    return None


def phase_one_prompt() -> str:
    return textwrap.dedent(
        """
        You are running an Astral gauntlet acceptance task.

        Phase 1 only:
        - Read docs/billing_policy.md and identify the hidden acceptance rules.
        - Inspect the tests and source.
        - Fix only the billing total logic if needed.
        - Exercise terminal behavior:
          1. Run scripts/progress.sh and observe progress output.
          2. Start scripts/silent_then_done.sh as a background task and use ReadTaskOutput until silent-done appears.
          3. Start python3 scripts/ask_yes.py, read the prompt, use SendTaskInput with y followed by a newline, and verify interactive.txt contains got-yes.
          4. Start scripts/never_end.sh as a background task, use ListBackgroundTasks to find it, then StopBackgroundTask to stop it.
        - Write HANDOFF.md with the hidden phrase, remaining bug status, terminal evidence, and next steps.
        - Do not fix parse_duration yet. Leave it for the resumed phase.
        """
    ).strip()


def phase_two_prompt() -> str:
    return textwrap.dedent(
        """
        Continue the same gauntlet after resume.

        Phase 2:
        - Use HANDOFF.md and the docs summary from prior context.
        - Fix parse_duration so compound hour/minute values pass.
        - Run the full unittest suite.
        - Write compact_survived.txt containing GAUNTLET_RULE_ALPHA and final-pass.
        - Finish only after all tests pass and the marker files prove the terminal steps survived.
        """
    ).strip()


def run_astral_phase(
    *,
    name: str,
    astral_bin: Path,
    home: Path,
    key_env: str,
    api_key: str,
    workdir: Path,
    prompt: str,
    model: str,
    timeout: int,
    thread_id: Optional[str] = None,
    image: Optional[Path] = None,
) -> RunResult:
    shared_args = [
        "--json",
        "--skip-git-repo-check",
        "--dangerously-bypass-approvals-and-sandbox",
        "-m",
        model,
    ]
    if thread_id:
        argv = [str(astral_bin), "exec", "resume", *shared_args, thread_id]
    else:
        argv = [str(astral_bin), "exec", *shared_args, "-C", str(workdir)]
    if image is not None:
        argv.extend(["--image", str(image)])
    argv.append("-")
    start = time.time()
    completed = run(
        argv,
        cwd=workdir,
        env=astral_env(home, key_env, api_key),
        stdin=f"{prompt}\n",
        timeout=timeout,
    )
    return RunResult(
        name=name,
        completed=completed,
        elapsed_seconds=round(time.time() - start, 2),
    )


def run_tests(workdir: Path) -> subprocess.CompletedProcess[str]:
    return run([sys.executable, "-m", "unittest", "-q"], cwd=workdir, timeout=60)


def load_fixtures(capture_dir: Path) -> list[dict[str, Any]]:
    fixtures = []
    for path in sorted(capture_dir.glob("*.json")):
        if path.name == "server-info.json":
            continue
        try:
            fixtures.append(json.loads(path.read_text(encoding="utf-8")))
        except json.JSONDecodeError:
            continue
    return fixtures


def body_text(value: Any, limit: int = 20000) -> str:
    if value is None:
        return ""
    text = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return text[:limit]


def request_body(fixture: dict[str, Any]) -> dict[str, Any]:
    body = fixture.get("request", {}).get("body")
    return body if isinstance(body, dict) else {}


def response_body(fixture: dict[str, Any]) -> Any:
    return fixture.get("response", {}).get("body")


def tool_names_from_request(body: dict[str, Any]) -> list[str]:
    names = []
    for tool in body.get("tools") or []:
        if not isinstance(tool, dict):
            continue
        if isinstance(tool.get("function"), dict):
            name = tool["function"].get("name")
        else:
            name = tool.get("name")
        if name:
            names.append(str(name))
    return names


def message_roles(body: dict[str, Any]) -> list[str]:
    roles = []
    for message in body.get("messages") or []:
        if isinstance(message, dict) and message.get("role"):
            roles.append(str(message["role"]))
    return roles


def response_tool_names(body: Any) -> list[str]:
    if not isinstance(body, str):
        return []
    names = []
    for line in body.splitlines():
        line = line.strip()
        if not line.startswith("data:"):
            continue
        payload = line.split(":", 1)[1].strip()
        if not payload or payload == "[DONE]":
            continue
        try:
            data = json.loads(payload)
        except json.JSONDecodeError:
            continue
        for choice in data.get("choices") or []:
            delta = choice.get("delta") if isinstance(choice.get("delta"), dict) else {}
            for call in delta.get("tool_calls") or []:
                function = call.get("function") if isinstance(call, dict) else None
                if isinstance(function, dict) and function.get("name"):
                    names.append(str(function["name"]))
    return names


def analyze_trajectory(capture_dir: Path, exec_logs: list[RunResult]) -> dict[str, Any]:
    report_dir = capture_dir.parent
    fixtures = load_fixtures(capture_dir)
    model_fixtures = [
        item
        for item in fixtures
        if "/chat/completions" in str(item.get("client_path") or "")
    ]
    tool_names = Counter()
    response_tools = Counter()
    role_sequences = []
    body_key_counts = Counter()
    request_chars = []
    compact_requests = 0
    image_markers = 0
    status_counts = Counter()
    rough_spots: list[str] = []

    for fixture in model_fixtures:
        status_counts.update([str(fixture.get("response", {}).get("status"))])
        body = request_body(fixture)
        body_key_counts.update(str(key) for key in body.keys())
        request_chars.append(len(body_text(body, limit=10_000_000)))
        text = body_text(body, limit=10_000_000)
        if "CONTEXT CHECKPOINT COMPACTION" in text or "COMPACTION" in text:
            compact_requests += 1
        if "local image omitted" in text:
            image_markers += 1
        tool_names.update(tool_names_from_request(body))
        response_tools.update(response_tool_names(response_body(fixture)))
        roles = message_roles(body)
        if roles:
            role_sequences.append(roles)

    command_127 = 0
    pytest_missing = 0
    stream_reconnects = 0
    unknown_task = 0
    stop_failures = 0
    background_mentions = 0
    compact_host_events = 0
    for result in exec_logs:
        combined = f"{result.completed.stdout}\n{result.completed.stderr}"
        command_127 += combined.count('"exit_code":127') + combined.count(
            "Exit code 127"
        )
        pytest_missing += combined.count("No module named pytest")
        stream_reconnects += combined.count("stream disconnected") + combined.count(
            "Reconnecting..."
        )
        unknown_task += combined.count("unknown task_id")
        stop_failures += combined.count("StopBackgroundTask failed")
        background_mentions += combined.count("ReadTaskOutput") + combined.count(
            "ListBackgroundTasks"
        )
        compact_host_events += combined.lower().count("context compacted")

    session_text = ""
    sessions_dir = report_dir / "astral-home" / "sessions"
    if sessions_dir.exists():
        session_text = "\n".join(
            path.read_text(encoding="utf-8", errors="replace")
            for path in sorted(sessions_dir.rglob("*.jsonl"))
        )
    dsml_compact_summaries = session_text.count("<｜｜DSML｜｜tool_calls>")
    full_policy_reads = session_text.count("## Policy Section 1")

    if compact_requests == 0:
        rough_spots.append("No compact request was captured in model API traffic.")
    if "Edit" not in tool_names and "Edit" not in response_tools:
        rough_spots.append(
            "Edit tool was not visible in captured model tool trajectory."
        )
    if stream_reconnects:
        rough_spots.append(
            f"Provider stream required retry/reconnect handling {stream_reconnects} time(s)."
        )
    if pytest_missing:
        rough_spots.append(
            f"Model attempted pytest in a stdlib-only fixture {pytest_missing} time(s) before recovering."
        )
    if dsml_compact_summaries:
        rough_spots.append(
            f"Compact summary contained DSML-looking pseudo tool-call text {dsml_compact_summaries} time(s)."
        )
    if full_policy_reads > 4:
        rough_spots.append(
            f"Agent repeatedly read the long policy from the start {full_policy_reads} time(s); compact pressure may be too aggressive."
        )
    if command_127:
        rough_spots.append(
            f"Agent hit missing-command/127 path {command_127} time(s), usually python vs python3."
        )
    if unknown_task or stop_failures:
        rough_spots.append(
            f"Background task control had failures: unknown_task={unknown_task}, stop_failures={stop_failures}."
        )
    if not any(name in response_tools for name in ["ReadTaskOutput", "SendTaskInput"]):
        rough_spots.append(
            "Background task tools were not clearly called in streamed tool deltas."
        )
    if request_chars and max(request_chars) > 800_000:
        rough_spots.append(
            f"Large request body captured ({max(request_chars)} chars); inspect context bloat."
        )

    return {
        "model_request_count": len(model_fixtures),
        "response_status_counts": dict(sorted(status_counts.items())),
        "body_key_counts": dict(sorted(body_key_counts.items())),
        "tool_names": dict(sorted(tool_names.items())),
        "response_tool_names": dict(sorted(response_tools.items())),
        "role_sequences": role_sequences,
        "request_chars": request_chars,
        "max_request_chars": max(request_chars) if request_chars else 0,
        "compact_request_count": compact_requests,
        "image_marker_count": image_markers,
        "command_127_count": command_127,
        "pytest_missing_count": pytest_missing,
        "stream_reconnect_count": stream_reconnects,
        "unknown_task_count": unknown_task,
        "stop_failure_count": stop_failures,
        "dsml_compact_summary_count": dsml_compact_summaries,
        "full_policy_read_count": full_policy_reads,
        "background_tool_mentions_in_exec_json": background_mentions,
        "compact_host_event_mentions": compact_host_events,
        "rough_spots": rough_spots,
    }


def final_checks(workdir: Path) -> dict[str, Any]:
    tests = run_tests(workdir)
    diff = run(["git", "diff", "--", "."], cwd=workdir, timeout=30)
    marker = workdir / "compact_survived.txt"
    handoff = workdir / "HANDOFF.md"
    interactive = workdir / "interactive.txt"
    return {
        "tests_exit": tests.returncode,
        "tests_stdout": tests.stdout,
        "tests_stderr": tests.stderr,
        "diff": diff.stdout,
        "compact_survived_exists": marker.exists(),
        "compact_survived_text": marker.read_text(encoding="utf-8")
        if marker.exists()
        else "",
        "handoff_exists": handoff.exists(),
        "interactive_exists": interactive.exists(),
        "interactive_text": interactive.read_text(encoding="utf-8")
        if interactive.exists()
        else "",
    }


def write_reports(
    report_dir: Path,
    runs: list[RunResult],
    checks: dict[str, Any],
    analysis: dict[str, Any],
    metadata: dict[str, Any],
) -> None:
    payload = {
        "metadata": metadata,
        "runs": [
            {
                "name": run_result.name,
                "exit": run_result.completed.returncode,
                "elapsed_seconds": run_result.elapsed_seconds,
            }
            for run_result in runs
        ],
        "checks": checks,
        "trajectory_analysis": analysis,
    }
    (report_dir / "gauntlet-report.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    lines = [
        "# Astral Gauntlet Report",
        "",
        f"- generated_at: `{metadata['generated_at']}`",
        f"- model: `{metadata['model']}`",
        f"- report_dir: `{metadata['report_dir']}`",
        f"- model_requests: `{analysis['model_request_count']}`",
        f"- compact_requests: `{analysis['compact_request_count']}`",
        f"- tests_exit: `{checks['tests_exit']}`",
        "",
        "## Runs",
        "",
    ]
    for run_result in runs:
        lines.append(
            f"- `{run_result.name}` exit={run_result.completed.returncode} elapsed={run_result.elapsed_seconds}s"
        )
    lines.extend(
        [
            "",
            "## Tools Seen",
            "",
            f"- request tools: `{', '.join(analysis['tool_names'].keys())}`",
            f"- streamed tool calls: `{', '.join(analysis['response_tool_names'].keys())}`",
            "",
            "## Rough Spots",
            "",
        ]
    )
    if analysis["rough_spots"]:
        for spot in analysis["rough_spots"]:
            lines.append(f"- {spot}")
    else:
        lines.append("- None detected by the current analyzer.")
    lines.extend(
        [
            "",
            "## Final Diff",
            "",
            "```diff",
            checks["diff"].rstrip(),
            "```",
            "",
        ]
    )
    (report_dir / "gauntlet-report.md").write_text("\n".join(lines), encoding="utf-8")


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
        else root / ".cache" / f"gauntlet-{timestamp}"
    )
    report_dir.mkdir(parents=True, exist_ok=True)
    workdir = report_dir / "work"
    home = report_dir / "astral-home"
    capture = CaptureProxy(args.upstream_base, report_dir / "captures")
    log_path = report_dir / "commands.log"
    runs: list[RunResult] = []
    try:
        capture.start()
        if capture.base_url is None:
            raise RuntimeError("capture proxy base URL missing")
        create_workspace(workdir)
        write_config(home, capture.base_url, key_env, args.model)

        phase1 = run_astral_phase(
            name="phase1",
            astral_bin=astral_bin,
            home=home,
            key_env=key_env,
            api_key=api_key,
            workdir=workdir,
            prompt=phase_one_prompt(),
            model=args.model,
            timeout=args.timeout_seconds,
            image=workdir / "assets" / "tiny.png" if args.include_image else None,
        )
        runs.append(phase1)
        append_log(log_path, "phase1", phase1.completed)
        thread_id = extract_thread_id(phase1.completed.stdout)

        if not args.skip_resume and thread_id and phase1.completed.returncode == 0:
            phase2 = run_astral_phase(
                name="phase2-resume",
                astral_bin=astral_bin,
                home=home,
                key_env=key_env,
                api_key=api_key,
                workdir=workdir,
                prompt=phase_two_prompt(),
                model=args.model,
                timeout=args.timeout_seconds,
                thread_id=thread_id,
            )
            runs.append(phase2)
            append_log(log_path, "phase2-resume", phase2.completed)

        checks = final_checks(workdir)
        analysis = analyze_trajectory(report_dir / "captures", runs)
        metadata = {
            "generated_at": datetime.now().isoformat(),
            "repo": str(root),
            "report_dir": str(report_dir),
            "workdir": str(workdir),
            "astral_bin": str(astral_bin),
            "model": args.model,
            "upstream_base": args.upstream_base,
            "thread_id": thread_id,
        }
        write_reports(report_dir, runs, checks, analysis, metadata)
        print(report_dir)
        failed = (
            any(result.completed.returncode != 0 for result in runs)
            or checks["tests_exit"] != 0
            or not checks["compact_survived_exists"]
            or "GAUNTLET_RULE_ALPHA" not in checks["compact_survived_text"]
        )
        return 1 if failed else 0
    finally:
        with contextlib.suppress(Exception):
            capture.stop()


if __name__ == "__main__":
    raise SystemExit(main())
