# Releases

## Writing release notes

- `CHANGELOG.md` is the only place. Add changes under `## Unreleased`. Format (summary paragraph, `### Highlights` with **bold headlines**) is in CONTRIBUTING.md.

## Cutting a release (only on the owner's go)

1. Bump the version in `synccrate/package.json`, `synccrate/src-tauri/tauri.conf.json` and `synccrate/src-tauri/Cargo.toml` (then `cargo check` so `Cargo.lock` follows).
2. Rename `## Unreleased` to `## X.Y.Z — YYYY-MM-DD`.
3. Merge into `main`, push, then push the tag `vX.Y.Z`. `release.yml` does the rest.

## What release.yml does with the CHANGELOG

- `create-release` reads the version's section once and passes it on:
  - to the GitHub release (the full section),
  - to tauri-action's `releaseBody` as plain text (summary + `• headline` lines). That becomes the updater's `latest.json` notes, which the update prompt shows. It stays plain because apps before 0.8.7 show it raw.
  - to the `discord` job (summary + headlines) for #announcements.
- Keep its parsing rules in step with `synccrate/src/lib/releaseNotes.ts`, which parses the same format in the app.
- The workflow holds the updater signing key: every action is pinned to a commit and jobs get no permissions they don't need. Keep it that way.
- Secrets: `TAURI_SIGNING_PRIVATE_KEY` (+ password), `WINGET_TOKEN` (winget PR), `DISCORD_RELEASE_WEBHOOK`.
