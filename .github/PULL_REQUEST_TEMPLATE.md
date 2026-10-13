## What this changes

<!-- One or two sentences. -->

## Lane

Lane:
Closes:

<!-- Lane ID (e.g. WD2-D) and the issue(s) this PR closes. Lanes are tracked in the
     WRL Forge Development GitHub Project. Small fixes outside a lane: write "none". -->

## Scope delivered

## Architecture invariants

- [ ] Exact source text remains canonical
- [ ] No second editable document model
- [ ] No hidden source instrumentation / synthetic identity
- [ ] Unsupported or ambiguous identity fails closed
- [ ] One user-visible action = one coherent undo transaction

<!-- Mark N/A with a reason where a box does not apply (e.g. docs-only PRs). -->

## Candidate

Baseline:
Candidate commit:
QA candidate/report:

## Independent QA

Verdict:

- [ ] Independent QA complete where required
- [ ] Blocking findings resolved or explicitly accepted by owner
- [ ] Tests not run are listed

## Tests

- [ ] Focused tests for the area I touched pass
- [ ] `npm run check` passes in full

Focused:
Electron/visual:
`npm run check`:
Cross-platform CI:

<!-- Paste the totals, or describe manual verification. -->

## Documentation / project closeout

- [ ] Architecture docs updated if required
- [ ] Active roadmap updated if scope changed
- [ ] Deferred work has its own issue
- [ ] Parent/sub-issues match actual completion

## Third-party provenance

Does this PR include code, assets, or content from another project?

- [ ] No — everything here is my own work
- [ ] Yes — details below

<!-- If yes: upstream project, source URL, version/tag/commit, files or components used,
     the upstream license (SPDX id), and confirmation that upstream copyright and license
     headers are preserved. See OPEN_SOURCE_PROVENANCE.md. -->

## DCO

- [ ] My commits are signed off (`git commit -s`) under the
      [Developer Certificate of Origin](https://developercertificate.org/)

Contributions are accepted under `GPL-3.0-or-later`. You keep the copyright in your work —
there is no CLA and no copyright assignment. See [CONTRIBUTING.md](../CONTRIBUTING.md).

## Merge readiness

- [ ] Owner review complete
- [ ] Required CI checks green
- [ ] No unresolved review conversations
- [ ] No unrelated changes
