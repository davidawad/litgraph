<!-- Title should follow Conventional Commits: <type>(<scope>)!: <description> -->

## Summary

<!-- 1-3 bullet points. What changed, why, who benefits. -->
-
-

## Related issue

<!-- Link an issue if one exists. Delete this section if not. -->
Closes: #

## Test plan

<!-- Bulleted checklist. CI must be green before merge. -->
- [ ] `just test` passes locally
- [ ] `just lint` passes locally (clippy -D warnings)
- [ ] `just fmt-check` passes locally
- [ ] No new secrets committed (gitleaks clean)

## SemVer impact

<!-- Mark one. release-please reads commit messages, not this box --
     but flag it so reviewers know what to expect. -->
- [ ] Patch (`fix:`) — backward-compatible bug fix
- [ ] Minor (`feat:`) — backward-compatible feature
- [ ] Major (`feat!:` or `BREAKING CHANGE:` footer) — backward-incompatible
- [ ] None (`docs:` / `chore:` / `refactor:` / etc.)
