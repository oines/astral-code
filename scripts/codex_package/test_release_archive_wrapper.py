#!/usr/bin/env python3

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WRAPPER = REPO_ROOT / ".github" / "scripts" / "build-codex-package-archive.sh"


class ReleaseArchiveWrapperTest(unittest.TestCase):
    def test_primary_bundle_uses_astral_variant_and_entrypoint(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            fake_bin_dir = root / "fake-bin"
            fake_bin_dir.mkdir()
            captured_args = root / "python-args"
            fake_python = fake_bin_dir / "python3"
            fake_python.write_text(
                '#!/bin/sh\nprintf \'%s\\n\' "$@" > "$CAPTURED_ARGS"\n',
                encoding="utf-8",
            )
            fake_python.chmod(0o755)

            entrypoint_dir = root / "entrypoint"
            entrypoint_dir.mkdir()
            touch_executable(entrypoint_dir / "astral")
            code_mode_host = touch_executable(entrypoint_dir / "codex-code-mode-host")
            archive_dir = root / "archive"
            runner_temp = root / "runner-temp"
            env = os.environ.copy()
            env.update(
                {
                    "CAPTURED_ARGS": str(captured_args),
                    "GITHUB_WORKSPACE": str(REPO_ROOT),
                    "PATH": f"{fake_bin_dir}{os.pathsep}{env['PATH']}",
                    "RUNNER_TEMP": str(runner_temp),
                }
            )

            subprocess.run(
                [
                    "/bin/bash",
                    str(WRAPPER),
                    "--target",
                    "aarch64-apple-darwin",
                    "--bundle",
                    "primary",
                    "--entrypoint-dir",
                    str(entrypoint_dir),
                    "--archive-dir",
                    str(archive_dir),
                ],
                check=True,
                env=env,
            )

            self.assertEqual(
                captured_args.read_text(encoding="utf-8").splitlines(),
                [
                    str(REPO_ROOT / "scripts" / "build_codex_package.py"),
                    "--target",
                    "aarch64-apple-darwin",
                    "--variant",
                    "astral",
                    "--entrypoint-bin",
                    str(entrypoint_dir / "astral"),
                    "--cargo-profile",
                    "release",
                    "--package-dir",
                    str(runner_temp / "codex-package-aarch64-apple-darwin"),
                    "--archive-output",
                    str(archive_dir / "codex-package-aarch64-apple-darwin.tar.gz"),
                    "--archive-output",
                    str(archive_dir / "codex-package-aarch64-apple-darwin.tar.zst"),
                    "--code-mode-host-bin",
                    str(code_mode_host),
                    "--force",
                ],
            )


def touch_executable(path: Path) -> Path:
    path.touch(mode=0o755)
    return path


if __name__ == "__main__":
    unittest.main()
