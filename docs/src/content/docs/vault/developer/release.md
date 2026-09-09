---
title: Release
description: "How Message Vault versions ship: version lockstep, git tags, and the artifacts CI builds."
---

Releasing is a maintainer task. One product version ([Semantic Versioning](https://semver.org/spec/v2.0.0.html): `MAJOR.MINOR.PATCH`) ships as two artifacts:

- The vault image `bitrealm/message-vault:<version>` on Docker Hub (also `<major>.<minor>`, `latest`, and `sha-…`). The Docker tag has no `v` prefix (`0.8.0`, not `v0.8.0`).
- Unsigned desktop installers on [GitHub Releases](https://github.com/bitrealm-io/message-vault/releases): Linux `.deb` and AppImage, Windows `.msi`, macOS `.dmg`.

The same tag publishes this documentation site to bitrealm.io, so the site always describes the released version.

Nothing is published to npm or PyPI. Pushing the git tag `v<version>` is what runs the release jobs. A merge to `main` does not ship.

The JSONL schema version 4 is independent of the product version. Version 3 is refused, never upgraded. Leave other `Cargo.toml` files at `0.1.0`, and don't bump `web-next/` for a product release.

## Before tagging

1. Merge the work to `main`. Wait until CI on `main` is green (`fmt`, workspace tests, `web` tests). `./scripts/check-pr.sh` is optional locally.
2. In `CHANGELOG.md`, change the in-development heading to the dated form (`## [0.9.0] - 2026-09-08`) and drop the per-bullet dates, which the heading now carries. Add the next in-development heading above it when work resumes.

   The changelog is written for the people who use Message Vault. Every entry is a **Feature** (something a person can now do), a **Fix** (something that was wrong and now behaves correctly), or a **Design** change (how the product works, including internal rework, said in one sentence). Keep route paths, status codes, schema versions and type names out of it — those belong in the pull request and the ADRs. Anything a reader has to act on goes under an **Upgrading** heading in that release.
3. Set these four files to the same number (for example `0.8.0`):
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
   - `web/package.json`
   - `crates/vault/server/Cargo.toml`
4. Commit and push that bump on `main`.
5. Tag that commit `v0.8.0` and push the tag.

## After tagging

GitHub Actions builds the image and the installers, opens a GitHub Release named `Message Vault v0.8.0`, and publishes the documentation site. The installers are not code-signed, so users may see SmartScreen or Gatekeeper warnings.

Don't create or push a tag unless a release should ship.
