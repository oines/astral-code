#!/usr/bin/env python3

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import textwrap
import unittest


INSTALL_SCRIPT = Path(__file__).with_name("install.sh")
VERSION = "0.142.5"


class InstallShTest(unittest.TestCase):
    def test_macos_install_exposes_code_mode_host_beside_astral(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            archive_path, checksum_path, metadata_json = create_package_release(root)

            result = run_installer_in(
                root,
                metadata_json=metadata_json,
                archive_path=archive_path,
                checksum_path=checksum_path,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            install_bin = root / "install-bin"
            current = root / "astral-home" / "packages" / "standalone" / "current"
            astral_path = install_bin / "astral"
            host_path = install_bin / "codex-code-mode-host"
            self.assertEqual(os.readlink(astral_path), str(current / "bin" / "astral"))
            self.assertEqual(
                os.readlink(host_path),
                str(current / "bin" / "codex-code-mode-host"),
            )
            self.assertTrue(os.access(host_path, os.X_OK))


def run_installer_in(
    root: Path,
    *,
    metadata_json: str,
    archive_path: Path,
    checksum_path: Path,
) -> subprocess.CompletedProcess[str]:
    bin_dir = root / "bin"
    bin_dir.mkdir()
    fake_curl = bin_dir / "curl"
    fake_curl.write_text(
        textwrap.dedent(
            """\
            #!/bin/sh
            url=""
            output=""
            previous=""
            for arg in "$@"; do
              case "$arg" in
                https://*) url="$arg" ;;
              esac
              if [ "$previous" = "-o" ]; then
                output="$arg"
              fi
              previous="$arg"
            done

            case "$url" in
              https://api.github.com/*)
                printf '%s\n' "$ASTRAL_TEST_METADATA_JSON"
                ;;
              */codex-package_SHA256SUMS)
                cp "$ASTRAL_TEST_CHECKSUM_PATH" "$output"
                ;;
              */codex-package-*.tar.gz)
                cp "$ASTRAL_TEST_ARCHIVE_PATH" "$output"
                ;;
              *)
                exit 22
                ;;
            esac
            """
        ),
        encoding="utf-8",
    )
    fake_curl.chmod(0o755)
    fake_uname = bin_dir / "uname"
    fake_uname.write_text(
        "#!/bin/sh\n"
        'case "$1" in\n'
        "  -s) printf 'Darwin\\n' ;;\n"
        "  -m) printf 'arm64\\n' ;;\n"
        "esac\n",
        encoding="utf-8",
    )
    fake_uname.chmod(0o755)

    home = root / "home"
    home.mkdir()
    env = os.environ.copy()
    env.update(
        {
            "ASTRAL_HOME": str(root / "astral-home"),
            "ASTRAL_INSTALL_DIR": str(root / "install-bin"),
            "ASTRAL_NON_INTERACTIVE": "1",
            "ASTRAL_RELEASE": VERSION,
            "ASTRAL_TEST_ARCHIVE_PATH": str(archive_path),
            "ASTRAL_TEST_CHECKSUM_PATH": str(checksum_path),
            "ASTRAL_TEST_METADATA_JSON": metadata_json,
            "HOME": str(home),
            "PATH": f"{bin_dir}:/usr/bin:/bin",
            "SHELL": "/bin/sh",
        }
    )
    return subprocess.run(
        ["/bin/sh", str(INSTALL_SCRIPT)],
        capture_output=True,
        check=False,
        env=env,
        text=True,
    )


def create_package_release(root: Path) -> tuple[Path, Path, str]:
    package_dir = root / "package"
    (package_dir / "bin").mkdir(parents=True)
    (package_dir / "codex-path").mkdir()
    (package_dir / "codex-package.json").write_text("{}\n", encoding="utf-8")
    write_executable(
        package_dir / "bin" / "astral",
        f"#!/bin/sh\nprintf 'astral-cli {VERSION}\\n'\n",
    )
    write_executable(
        package_dir / "bin" / "codex-code-mode-host",
        "#!/bin/sh\nexit 0\n",
    )
    write_executable(package_dir / "codex-path" / "rg", "#!/bin/sh\nexit 0\n")

    asset = "codex-package-aarch64-apple-darwin.tar.gz"
    archive_path = root / asset
    with tarfile.open(archive_path, "w:gz") as archive:
        for path in package_dir.iterdir():
            archive.add(path, arcname=path.name)

    archive_digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path = root / "codex-package_SHA256SUMS"
    checksum_path.write_text(f"{archive_digest}  {asset}\n", encoding="utf-8")
    checksum_digest = hashlib.sha256(checksum_path.read_bytes()).hexdigest()
    metadata_json = json.dumps(
        {
            "assets": [
                {"name": asset, "digest": f"sha256:{archive_digest}"},
                {
                    "name": "codex-package_SHA256SUMS",
                    "digest": f"sha256:{checksum_digest}",
                },
            ],
            "tag_name": f"rust-v{VERSION}",
        },
        indent=2,
    )
    return archive_path, checksum_path, metadata_json


def write_executable(path: Path, contents: str) -> None:
    path.write_text(contents, encoding="utf-8")
    path.chmod(0o755)


if __name__ == "__main__":
    unittest.main()
