import os
import subprocess
import unittest
from pathlib import Path


class UpdaterSyntaxTests(unittest.TestCase):
    def test_powershell_parser(self) -> None:
        root = Path(__file__).resolve().parents[1]
        script = root / "src" / "update" / "Test-Invoke-Update.ps1"
        shells = ("pwsh", "powershell.exe") if os.name == "nt" else ("pwsh",)
        for shell in shells:
            with self.subTest(shell=shell):
                subprocess.run([shell, "-NoProfile", "-File", str(script)], check=True, timeout=30)

    def test_bash_parser_and_shellcheck(self) -> None:
        root = Path(__file__).resolve().parents[1]
        prefix = (
            ["wsl", "--distribution", os.environ["LINT_WSL_DISTRO"], "--cd", str(root), "--"] if os.name == "nt" else []
        )
        arguments = [
            "42",
            "/opt/it's here/update.tar.gz",
            "/opt/it's here/staging",
            "/opt/it's here/install dir",
            "/opt/it's here/install dir/heartwire",
            "/opt/it's here/update.log",
        ]
        for command in [
            ["bash", "-n", "src/update/apply.sh", *arguments],
            ["shellcheck", "src/update/apply.sh"],
        ]:
            with self.subTest(command=command[0]):
                subprocess.run([*prefix, *command], cwd=root, check=True, timeout=30)
