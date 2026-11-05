#!/usr/bin/env python3

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path


PLATFORMS = {
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--assets-dir", required=True, type=Path)
    parser.add_argument("--base-url")
    args = parser.parse_args()

    base_url = args.base_url or (
        f"https://github.com/{args.repo}/releases/download/{args.tag}"
    )
    base_url = base_url.rstrip("/")
    platforms = {}
    missing = set()
    for platform, (asset_name, signature_name) in PLATFORMS.items():
        asset_path = args.assets_dir / asset_name
        signature_path = args.assets_dir / signature_name
        if not asset_path.is_file():
            missing.add(asset_name)
        if not signature_path.is_file():
            missing.add(signature_name)
        if asset_path.is_file() and signature_path.is_file():
            platforms[platform] = {
                "signature": signature_path.read_text().rstrip(),
                "url": f"{base_url}/{asset_name}",
            }

    if missing:
        raise SystemExit(
            "Missing updater release assets: " + ", ".join(sorted(missing))
        )

    manifest = {
        "version": args.version,
        "notes": f"See https://github.com/{args.repo}/releases/tag/{args.tag}",
        "pub_date": datetime.now(timezone.utc).isoformat(timespec="seconds").replace(
            "+00:00", "Z"
        ),
        "platforms": platforms,
    }
    (args.assets_dir / "latest.json").write_text(
        json.dumps(manifest, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
