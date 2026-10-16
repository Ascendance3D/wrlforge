# Jules to Claude review gate — staged rollout

**Status: Live Claude review, feedback, approvals and merges are OFF.**

The Jules Claude Review (staged) workflow is a read-only GitHub inventory. It does not run Claude or post corrections. This is the first tested component, not the full active loop.

## Implemented

- The inventory reads only public Ascendance3D/wrlforge pull requests, changed paths, issue comments, and check runs.
- A Jules trailer does not prove identity. The inventory also requires an owner-authored JULES issue and its GitHub Actions session-start comment for the same task ID.
- The session must match exactly in three places: the PR description task ID, the last token of the branch name, and the session ID in the Actions comment on the owner JULES issue. The branch number must be that issue number (`issue-163-...`, `jules-160-...`) or the JULES task number with three digits (`jules-009-...`). Branches of other formats are refused. A session ID found on more than one issue is refused.
- Missing proof, changed SHA, unrelated repository, changed base, missing API data and sensitive paths stop low-risk eligibility.
- Only ordinary Markdown files in docs/ can become low-risk *review candidates*. Files about security, audits, dependencies, licenses, credentials, or releases stay under owner review.
- The result is never an approval. The implementation always sets merge_authorized to false.
- The script does not fetch or execute PR code. It only sends GitHub REST GET requests through GitHub CLI.
- Offline tests cover source identity, path scope, missing CI, re-review by SHA, feedback attempts, no-op PRs, and no merge.
- The review workflow uses trusted main checkout on hosted Ubuntu runners. Its Claude job is permanently disabled by a literal false condition.
- No secrets, PR comments, labels, reviews, merges, settings changes or releases are made by this workflow.

## Activation requires a new owner-approved PR

1. Verify this staged workflow and its tests through independent QA. Test real GitHub pagination, a full PR inventory, re-review after new commits, and API errors.
2. Configure the official Claude GitHub App plus an approved authentication method. Prefer workload identity federation when available. Alternatively use an Actions repository secret named ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN. Never show a key in a comment or log.
3. Pin the reviewed Claude Code GitHub Action to its exact commit SHA. Verify tool permissions and Claude Code Action behavior against the current upstream version.
4. Add a separate restricted feedback writer. It can post one schema-checked correction for each exact PR head SHA. It must stop after three failed correction attempts and preserve owner control. The review runner must never execute PR code or push changes.
5. Pilot Claude reviews in dry-run mode. Check API usage, cost, review findings and repeat-comment suppression. Require independent security QA before removing the literal false guard.
6. Verify branch protection, CODEOWNERS and GitHub review requirements with an owner/admin token. This integration could not read the protected branch settings.

## Conditional merge policy — planning only

| PR category | Future proposal | Current action |
| --- | --- | --- |
| Plain non-sensitive docs, four or fewer files | Low-risk review and possible conditional merge | Review only, owner merge |
| Security, audit, dependencies, licenses, releases | Not eligible | Owner review |
| Source identity, parser, recovery, VRML editing, tests | Not eligible | Owner review |
| CI, workflows, GitHub permissions, secrets, signing | Not eligible | Owner review |
| No-op with zero changed files | Verify existing tests | Recommend owner closure |

No automatic approval or merge code is enabled. A separate owner authorization is needed for the exact merge categories and activation procedure.

Future acceptance must independently re-fetch the PR and require the same head SHA that passed review; the correct base SHA; verified Jules session; permitted file scope; complete required CI at that SHA; valid independent review; no unresolved comments; and a permitted merge state. Do not use Claude PASS alone. The PR author cannot provide its own independent approval.

## Constraints

- Do not use pull_request_target. GitHub advises against checking out untrusted PR code with this privileged event.
- Never give a privileged job access to code execution from an untrusted PR.
- Keep Jules dispatcher and the private native Rust X_ITE port unchanged.
- Linux comes first, macOS 15+ second, Windows 11 is a future target only, and mobile is out of scope.
- A passing legacy Node CI job does not prove native Rust/Tauri macOS builds or GUI QA.
- Scheduled inventory jobs may be delayed by GitHub.
- The staged inventory uses no Anthropic tokens. Future Claude costs require measured pilot usage.
- Disable the Jules Claude Review (staged) workflow to stop inventories. Jules Overnight Queue is separate.

## Local validation

Run python3 .github/scripts/test_jules_claude_review.py and python3 -m py_compile .github/scripts/jules_claude_review.py.

## Official guidance

- https://github.com/anthropics/claude-code-action
- https://github.com/anthropics/claude-code-action/blob/main/docs/setup.md
- https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target
