import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("connect-update-manifest.py")
ASSETS = {
    "windows-x86_64-nsis": (
        "NOALBS-Cloud-Connect-windows-x64-setup.exe",
        "NOALBS-Cloud-Connect-windows-x64-setup.exe.sig",
    ),
    "windows-x86_64": (
        "NOALBS-Cloud-Connect-windows-x64-setup.exe",
        "NOALBS-Cloud-Connect-windows-x64-setup.exe.sig",
    ),
    "darwin-aarch64-app": (
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz",
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz.sig",
    ),
    "darwin-x86_64-app": (
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz",
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz.sig",
    ),
    "darwin-aarch64": (
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz",
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz.sig",
    ),
    "darwin-x86_64": (
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz",
        "NOALBS-Cloud-Connect-macos-universal.app.tar.gz.sig",
    ),
    "linux-x86_64-appimage": (
        "NOALBS-Cloud-Connect-linux-x86_64.AppImage",
        "NOALBS-Cloud-Connect-linux-x86_64.AppImage.sig",
    ),
    "linux-x86_64-deb": (
        "NOALBS-Cloud-Connect-linux-amd64.deb",
        "NOALBS-Cloud-Connect-linux-amd64.deb.sig",
    ),
    "linux-x86_64-rpm": (
        "NOALBS-Cloud-Connect-linux-x86_64.rpm",
        "NOALBS-Cloud-Connect-linux-x86_64.rpm.sig",
    ),
}


class ConnectUpdateManifestTest(unittest.TestCase):
    def test_generates_signed_platform_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            assets_dir = Path(temporary)
            for _, (asset, signature) in ASSETS.items():
                (assets_dir / asset).write_text("artifact")
                (assets_dir / signature).write_text("signature\n \t")

            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--version",
                    "0.1.1",
                    "--tag",
                    "connect-v0.1.1",
                    "--repo",
                    "NOALBS/noalbs-cloud",
                    "--assets-dir",
                    str(assets_dir),
                    "--base-url",
                    "http://127.0.0.1:8765",
                ],
                check=True,
            )
            manifest = json.loads((assets_dir / "latest.json").read_text())

        self.assertEqual(manifest["version"], "0.1.1")
        self.assertEqual(
            manifest["notes"],
            "See https://github.com/NOALBS/noalbs-cloud/releases/tag/connect-v0.1.1",
        )
        self.assertTrue(manifest["pub_date"].endswith("Z"))
        self.assertEqual(set(manifest["platforms"]), set(ASSETS))
        self.assertNotIn("linux-x86_64", manifest["platforms"])
        for platform, (asset, _) in ASSETS.items():
            self.assertEqual(
                manifest["platforms"][platform],
                {
                    "signature": "signature",
                    "url": f"http://127.0.0.1:8765/{asset}",
                },
            )

    def test_missing_assets_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--version",
                    "0.1.1",
                    "--tag",
                    "connect-v0.1.1",
                    "--repo",
                    "NOALBS/noalbs-cloud",
                    "--assets-dir",
                    temporary,
                ],
                capture_output=True,
                text=True,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing updater release assets", result.stderr)


if __name__ == "__main__":
    unittest.main()
