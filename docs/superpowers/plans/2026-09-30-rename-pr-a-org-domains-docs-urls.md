# Rename PR (a): org, domains, and docs URLs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every link, host, path, and image name in the tree points at `messagecrate/message-crate`, `messagecrate.app/docs/…`, `my.messagecrate.app`, and `bitrealm/message-crate`, and the docs site builds at the new paths.

**Architecture:** The docs content directory moves from `vault/` to `docs/`, which changes every page URL. Everything else is a text replacement of four strings, followed by regenerating the two generated files (`openapi.json`, `vaultApi.types.ts`) from the server. No product words change in this PR; that is PR (b).

**Tech Stack:** Astro Starlight (`docs/`), utoipa (`crates/vault/server/src/openapi.rs`), openapi-typescript (`web`), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-30-rename-to-message-crate-design.md`

## Global Constraints

- Work on a branch, never `main`. PR description uses `.github/PULL_REQUEST_TEMPLATE/feature.md`.
- `web-next/` is untouched.
- Email addresses (`vault@bitrealm.io`) are not changed in this PR; there is no decision on a new address yet.
- Product copy ("Message Vault", "vault") is not changed in this PR.
- CHANGELOG.md links are rewritten; its entry text is not.
- No `biome-ignore`; no compatibility aliases or redirects inside the product.

---

### Task 1: Move the docs content and fix every path that names it

**Files:**
- Move: `docs/src/content/docs/vault/` → `docs/src/content/docs/docs/`
- Modify: `docs/astro.config.mjs` (sidebar slugs, `site`, `redirects`, `editLink`, `social`)
- Modify: `docs/public/404.html`, `docs/public/CNAME`, `docs/scripts/copy-http-api-reference.sh`, `.github/workflows/docs.yml`
- Modify: every `docs/src/**` file linking to `/vault/user/` or `/vault/developer/`

- [ ] **Step 1: Move the directory with git so history follows**

```bash
git mv docs/src/content/docs/vault docs/src/content/docs/docs
```

- [ ] **Step 2: Rewrite paths and hosts under `docs/` and the workflow**

```bash
grep -rIl "vault/user\|vault/developer\|bitrealm.io\|bitrealm-io/message-vault" docs/src docs/public docs/scripts docs/astro.config.mjs .github/workflows/docs.yml \
  | grep -v "docs/src/assets/openapi.json" \
  | xargs sed -i \
    -e 's#vault/user#docs/user#g' \
    -e 's#vault/developer#docs/developer#g' \
    -e 's#https://bitrealm\.io#https://messagecrate.app#g' \
    -e 's#bitrealm\.io#messagecrate.app#g' \
    -e 's#bitrealm-io/message-vault#messagecrate/message-crate#g'
printf 'messagecrate.app\n' > docs/public/CNAME
```

- [ ] **Step 3: Fix the 404 map by hand**

In `docs/public/404.html`, the map entry `'/vault': ''` used to strip an old prefix. Replace it with `'/vault': '/docs'` so a bookmarked `/vault/user/...` lands on `/docs/user/...`.

- [ ] **Step 4: Fix the Astro redirect by hand**

In `docs/astro.config.mjs`, the `redirects` entry becomes:

```js
redirects: {
  '/docs/developer/docker-compose/': '/docs/developer/docker/',
},
```

- [ ] **Step 5: Build the docs**

Run: `cd docs && npm ci && npm run check && npm run build`
Expected: both pass; `docs/dist/docs/user/index.html` exists.

- [ ] **Step 6: Commit**

```bash
git add -A docs .github/workflows/docs.yml
git commit -m "docs: move the site from bitrealm.io/vault to messagecrate.app/docs"
```

### Task 2: Rewrite hosts, org, and image name across the rest of the tree

**Files:**
- Modify: every tracked file outside `web-next/`, `docs/`, and the two generated files that contains `bitrealm.io`, `app.bitrealm.io`, `bitrealm-io/message-vault`, or `bitrealm/message-vault`
- Modify: `crates/vault/server/src/problem.rs` (`ERRORS_URL`), `crates/vault/server/src/openapi.rs` (license URL)
- Modify: `.github/workflows/ci.yml` (image name, release notes URLs and image)

- [ ] **Step 1: Run the replacement**

```bash
git ls-files \
  | grep -v "^web-next/\|^docs/\|^web/src/lib/vaultApi.types.ts$\|CHANGELOG.md" \
  | xargs grep -Il "bitrealm" \
  | xargs sed -i \
    -e 's#app\.bitrealm\.io#my.messagecrate.app#g' \
    -e 's#bitrealm\.io/vault/user#messagecrate.app/docs/user#g' \
    -e 's#bitrealm\.io/vault/developer#messagecrate.app/docs/developer#g' \
    -e 's#bitrealm\.io/vault/errors#messagecrate.app/docs/developer/reference/errors#g' \
    -e 's#bitrealm-io/message-vault#messagecrate/message-crate#g' \
    -e 's#bitrealm/message-vault#bitrealm/message-crate#g' \
    -e 's#https://bitrealm\.io#https://messagecrate.app#g'
sed -i -e 's#bitrealm-io/message-vault#messagecrate/message-crate#g' CHANGELOG.md
```

- [ ] **Step 2: Check what is left**

Run: `git ls-files | grep -v "^web-next/" | xargs grep -In "bitrealm" | grep -v "bitrealm/message-crate\|@bitrealm.io\|vaultApi.types.ts\|openapi.json"`
Expected: no output. Email addresses and the Docker namespace are the only permitted survivors.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore: point every link at messagecrate.app, messagecrate/message-crate and bitrealm/message-crate"
```

### Task 3: Regenerate the OpenAPI document and the web types

**Files:**
- Regenerate: `docs/src/assets/openapi.json`, `web/src/lib/vaultApi.types.ts`

- [ ] **Step 1: Dump the spec from the server**

Run: `cargo run -p message-vault-server -- dump-openapi --output docs/src/assets/openapi.json`
Expected: file rewritten; `grep -c bitrealm docs/src/assets/openapi.json` prints `0`.

- [ ] **Step 2: Regenerate the web types**

Run: `cd web && npm run gen:api`
Expected: `web/src/lib/vaultApi.types.ts` rewritten; `grep -c bitrealm web/src/lib/vaultApi.types.ts` prints `0`.

- [ ] **Step 3: Run the stale-spec test and the generated-types check**

Run: `cargo test -p message-vault-server committed_openapi_matches_dump && ./scripts/check-generated-api-types.sh`
Expected: both pass.

- [ ] **Step 4: Commit**

```bash
git add docs/src/assets/openapi.json web/src/lib/vaultApi.types.ts
git commit -m "chore: regenerate the OpenAPI document and web types for the new docs URLs"
```

### Task 4: Full check, PR, and the repository's own metadata

- [ ] **Step 1: Run the full pre-flight**

Run: `./scripts/check-all.sh`
Expected: pass.

- [ ] **Step 2: Add the changelog entry**

Under the unreleased heading in `CHANGELOG.md`, add:

```markdown
- The project moved: the repository is `messagecrate/message-crate`, the docs are at `https://messagecrate.app/docs/`, the hosted product is at `https://my.messagecrate.app`, and the Docker image is `bitrealm/message-crate`. Error `type` URLs now point at the new docs host.
```

- [ ] **Step 3: Commit, push, open the PR from the feature template**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): record the move to messagecrate.app"
git push -u origin HEAD
gh pr create --title "chore: move the project to messagecrate.app and messagecrate/message-crate" --body-file <filled template>
```

- [ ] **Step 4: Update the repository description and homepage**

Run: `gh repo edit messagecrate/message-crate --description "Pry digital conversations out of chat apps and store them in your own self-hosted archive." --homepage https://messagecrate.app`
