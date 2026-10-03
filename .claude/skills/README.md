# Vendored skills

Matt Pocock's agent skills, copied into this repo so every session — desktop or
Claude Code on the web — has them without a per-machine install.

- Source: https://github.com/mattpocock/skills (MIT, see `LICENSE`)
- Plugin version: 1.2.3
- Commit: 3cca18b368ae95cdbdebbff572ccafa662551015

These are the stable sets only: `skills/engineering`, `skills/productivity` and
`skills/misc` from upstream, flattened one directory per skill. The upstream
`deprecated/` and `in-progress/` sets are not included.

## Updating

Re-copy from a fresh clone of the upstream repo and update the commit above:

```bash
git clone --depth 1 https://github.com/mattpocock/skills /tmp/mp-skills
for cat in engineering productivity misc; do
  cp -r /tmp/mp-skills/skills/$cat/*/ .claude/skills/
done
rm -rf .claude/skills/code-review   # replaced by pr-review, see Local edits
```

Local edits to these files are fine — that is the point of vendoring rather than
subscribing to the plugin — but note them here so an update does not silently
revert them.

## Local edits

- `code-review` is replaced by `pr-review`: the review is posted on the pull
  request, fixed, and queued, following this repo's process (`AGENTS.md`,
  "Review on the pull request"). The update loop above deletes upstream's
  `code-review` again. `implement`, `tdd` and `ask-matt` name `pr-review`
  instead of `code-review`.
