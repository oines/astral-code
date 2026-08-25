#!/usr/bin/env python3

from pathlib import Path


CORE = Path(__file__).resolve().parents[1] / "core" / "src"
CODEX_ADAPTER = CORE / "provider_adapters" / "codex.rs"


def runtime_rust_files() -> list[Path]:
    return [
        path
        for path in CORE.rglob("*.rs")
        if "tests" not in path.parts and not path.name.endswith("_tests.rs")
    ]


def main() -> int:
    violations: list[str] = []
    codex_id_hot_paths = [
        CORE / "tools" / "spec_plan.rs",
        CORE / "responses_request.rs",
        *(
            path
            for path in (CORE / "session").rglob("*.rs")
            if not path.name.endswith("_tests.rs")
        ),
    ]
    for path in codex_id_hot_paths:
        if "CODEX_PROVIDER_ID" in path.read_text():
            violations.append(f"{path}: CODEX_PROVIDER_ID belongs in a provider adapter")

    for path in runtime_rust_files():
        text = path.read_text()
        if "use_responses_lite" in text and path != CODEX_ADAPTER:
            violations.append(f"{path}: use_responses_lite is Codex-adapter-only")
        if "responses_builtin_tools" in text:
            violations.append(f"{path}: responses_builtin_tools must not drive Core runtime")

    if violations:
        print("Provider boundary violations:")
        for violation in violations:
            print(f"- {violation}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
