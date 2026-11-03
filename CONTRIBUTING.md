# Contributing

Use the toolchain in `rust-toolchain.toml` and the committed `Cargo.lock`. Run formatting, Clippy with warnings denied, the full Rust tests, `scripts/smoke.sh` against the built binary, and `scripts/check-plugin.py` before opening a change. Python is only a development metadata check; do not add it to the native runtime.

Preserve existing-thread ownership, source isolation, explicit permission boundaries, and durable unknown outcomes. Provider payloads are untrusted input. Do not derive executable commands or local destinations from them. A mock proves a local contract, not a real provider or host integration.

Add meaningful tests for a changed invariant or failure case. Use invented fixtures with no customer content, personal data, tokens, or internal endpoints. Never run write E2E against production discussions without explicit authorization. Report the exact environment and distinguish not-run checks from passes.

Keep skills concise and discover the installed machine-readable schema. Plugin lifecycle hooks must not perform hidden installation or start services. Update versioned manifests, installer defaults, release notes, and documentation together for a release.

Routine CI is deliberately one Linux job. Full native platform checks run for an unpublished version after trusted main-branch CI, or for tag/manual releases. Published versions skip repeat native matrices. Review dependency updates and retain `Cargo.lock`; do not expand the normal matrix without a concrete reason.

By contributing you agree that your contribution is provided under the repository's [Apache 2.0 license](LICENSE). See [SECURITY.md](SECURITY.md) for sensitive reports.
