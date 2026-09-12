# Pull Request Template

## Title

[Provide a succinct and descriptive title for the pull request, e.g., "Improve caching mechanism for API calls"]

## Type of Change

- [ ] New feature
- [ ] Bug fix
- [ ] Documentation update
- [ ] Refactoring
- [ ] Security patch
- [ ] UI/UX improvement
- [ ] Performance improvement

## Description

[Provide a detailed explanation of the changes you have made. Include the reasons behind these changes and any relevant context. Link any related issues.]

## Testing

[Detail the testing you have performed to ensure that these changes function as intended. Include information about any added tests.]

## Impact

[Discuss the impact of your changes on the project. This might include effects on performance, new dependencies, or changes in behaviour.]

## Additional Information

[Any additional information that reviewers should be aware of.]

## Checklist

- [ ] `./scripts/check-pr.sh` passes (rustfmt, workspace build and test,
      `src-tauri` build, Biome, Vitest). Clippy is not in CI — run
      `./scripts/lint-all.sh` too.
- [ ] User-visible changes have a `CHANGELOG.md` bullet under `[Unreleased]`,
      starting with an ISO date.
- [ ] New names match `CONTEXT.md`; nothing uses a word the glossary avoids.
- [ ] Docs under `docs/` updated if behaviour a person can see changed.
- [ ] No personal backups or real message data added to `tests/fixtures/`.
- [ ] No product-version bump and no `v*` tag unless the release was asked for.
