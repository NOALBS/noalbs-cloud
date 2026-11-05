# NOALBS Cloud Connect

## Releasing

1. Bump the version in `Cargo.toml` under `[workspace.package]` and in
   `app/src-tauri/tauri.conf.json`.
2. Commit the version changes, create a `connect-vX.Y.Z` tag, and push the tag.
3. The Connect release workflow verifies both versions, builds installers for
   Windows, macOS and Linux, and publishes them with `SHA256SUMS` on GitHub.

Automatic in-app updates and code signing are not set up yet.
