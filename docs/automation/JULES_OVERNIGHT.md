# Jules overnight queue

> **Status (2026-10-17): the scheduled workflow is disabled.** Jules work is now dispatched locally and reviewed before merge; see [Local dispatch and review](#local-dispatch-and-review-current). The **Jules Overnight Queue** workflow stays in the repository, disabled in GitHub Actions, as a fallback. Do not re-enable it while local dispatch is in use, or one approved issue can be dispatched twice.

This workflow starts **at most one Jules session per run** from approved GitHub issues in the public `Ascendance3D/wrlforge` repository. It never accesses the separate private native X_ITE Rust port.

## Local dispatch and review (current)

The owner's agent dispatches Jules sessions from a workstation with the [`jules-dispatch`](https://github.com/Yuuqq/jules-dispatch) CLI/MCP server. The approval contract below is unchanged: only owner-authored `JULES-` issues the owner approved are dispatched.

1. **Task.** Each task is an owner-authored GitHub issue titled `JULES-NNN — …`. It names one allowed file or test module, numbered cases, acceptance criteria, and the standard limits. Issues start as **"Queued for owner review"**. The owner approves dispatch explicitly, and the issue text is then changed to **"Owner-approved for dispatch"**.
2. **Dispatch.** Owner-approved issues go out in small waves of three to five independent tasks, each with **`requirePlanApproval: true`**. The prompt is the issue text plus delivery rules: branch `jules-<issue>-<slug>`, a PR description beginning `Fixes #<issue>`, the exact test command with pass/fail counts, no `Signed-off-by` trailers, a draft PR, and never merge. The agent applies `jules-approved` for the record, posts `Jules session started: sessions/<id>` on the issue, and removes the label. That keeps the review inventory's session proof working.
3. **Plan review.** Every Jules plan is checked against the issue and `WD.md` before approval. A plan that claims a case is "already covered" is approved only after the named test is read. A verified no-op closes the issue without a PR.
4. **PR review.** For each PR, in its own worktree under `.worktrees/wrlforge/`, the agent:
   - runs `npm run check` (JavaScript) or the crate's `cargo +1.95.0 test` locally (CI does not run the Rust tests);
   - checks file scope and rustfmt on the added code only;
   - reviews against the issue;
   - runs a scoped `codex exec` review with the requirements spelled out. A plain `codex review` missed real defects.

   Findings go back to the same Jules session. If a session fails a fix round, a fresh session starts from the PR branch, or the owner applies the fix-up.
5. **Merge.** Only after the owner authorizes it, the agent waits for CI on the final head, then squash-merges with `--match-head-commit` and the owner's `Signed-off-by` trailer. Jules commits carry no DCO sign-off of their own. The agent then tells the session the task is complete, so it does not push again.

Owner decisions (which issues, dependency exceptions, how to finish a failed session) are asked for explicitly. Nothing is merged, closed or re-labeled without that authorization.

## Owner platform plan

1. Linux is the current development platform.
2. macOS 15+ follows Linux.
3. Windows 11 is a future planned target. This queue does **not** authorize Windows development, CI, packaging, or release.
4. Mobile is out of scope.

## Setup (requires owner action)

1. Create a Jules API key at <https://jules.google.com/settings>. Do not paste it in chat or an issue.
2. Save the key in **GitHub > Settings > Secrets and variables > Actions** as the repository secret `JULES_API_KEY`.
3. Confirm the Jules GitHub app already has access to `Ascendance3D/wrlforge`. The dispatcher lists Jules sources and verifies the exact repository and the `main` branch. It does **not** guess the Jules source resource name.
4. Create the GitHub issue label `jules-approved`, if it does not already exist.
5. PR #159 was merged after safety QA. Any future dispatcher change still needs independent QA and owner approval.
6. Test the **Jules Overnight Queue** workflow manually from `main` with `dry_run=true`.
7. After verifying the dry run, have the owner apply `jules-approved` to an already-created, small, owner-authored issue with a title starting `JULES-`.

Do not re-approve a consumed issue without checking Jules sessions for duplicates. Current task and PR status lives in GitHub issues, not in this file.

## Scheduling and manual use

The schedule below applies only while the workflow is enabled; it was disabled on 2026-10-17. While enabled, GitHub Actions attempts dispatch **hourly at minute 17, 04:17–11:17 UTC daily**. This is approximately 00:17–07:17 EDT or 23:17–06:17 EST. GitHub scheduled runs can be delayed. The dispatcher attempts **at most one task per run**.

A **manual** workflow run defaults to `dry_run=true`. The dry run lists eligible work and checks its latest approval-label actor. It **does not** change labels, post comments, read the Jules API, or create a session. It does not need a Jules API key.

A scheduled run performs **live dispatch** when there is a valid owner-approved task and the key and Jules source are available. A manual run with `dry_run=false` also performs live dispatch. GitHub Actions concurrency prevents overlapping runs of this workflow. Hourly dispatch is a maximum rate, not a guarantee of completion.

## Approval policy

Only an **open GitHub issue** is eligible when all these conditions hold:

- It is authored by `DJAscendance`.
- Its title begins with `JULES-`.
- It currently carries the `jules-approved` label.
- GitHub's issue events show the **most recent application** of that label was by `DJAscendance`.
- The issue body is present and no longer than 8,000 characters.

If the approval actor is wrong or the body is invalid, the live run removes the label, leaves a refusal comment, and checks the next eligible task. A dry run only reports the refusal. If no valid task remains, the workflow stops without making a Jules API request.

The workflow checks the GitHub repository and current branch and refuses to run outside `Ascendance3D/wrlforge` on `main`.

## Jules source and API protection

Before removing an accepted label, the workflow checks that `JULES_API_KEY` exists and queries Jules's `GET /v1alpha/sources` endpoint. It follows pagination, requires exactly one matching connected GitHub repository, and uses `GET /v1alpha/sources/{sourceId}` to verify the repository and an available `main` branch.

The dispatcher queries the paginated `GET /v1alpha/sessions` API. If a nonterminal session in the list omits `sourceContext`, the dispatcher calls `GET /v1alpha/sessions/{sessionId}` and checks the full session before it counts that session. An API error or missing repository source stops dispatch without changing the approval label. The dispatcher counts only nonterminal sessions for the verified public repository source. **Four active sessions** is the initial safety limit, not a Google Jules account limit. `COMPLETED` and `FAILED` do not use a slot. All other states use a slot, including paused sessions. A full queue or sessions API failure leaves the approval label in place. The list response must include a `sessions` array, including when empty, with valid session names and states. The workflow rejects HTTP-success error bodies, missing session arrays, malformed session records, and incomplete pages. This conservative rule may stop dispatch if the Jules API omits its list field for an empty result. Existing GitHub draft PRs do not use active Jules session slots. Change the limit only after review.

Once the capacity check passes, it removes the issue's approval label **before** the external `POST /v1alpha/sessions`. This gives the task at-most-once automatic dispatch. If the network fails or the response is lost after removal, it **does not retry**. Check the Jules Sessions page before any owner re-approval.

The API uses `AUTO_CREATE_PR` and `requirePlanApproval=false` to let Jules proceed through an **already approved issue**. The task prompt requests a **draft PR**. The API does not guarantee draft status, so every Jules PR must remain under review and cannot be merged automatically.

The workflow token has `contents: read` and `issues: write` only. Jules's separate GitHub app permissions still apply. Do not place signing or deployment secrets in Jules.

## Checks required before merge

- Review workflow permissions, owner-label actor checking, and exact repo matching.
- Validate YAML parsing and Bash syntax.
- Run `python3 .github/scripts/test-jules-overnight.py` for offline mock checks.
- Test dry-run no-side-effects behavior.
- Test valid and invalid issue selection.
- Test missing API key, unavailable Jules source, session API failures, four active sessions, and the three-session dispatch path.
- Test duplicate protection and uncertain POST responses.
- Obtain independent security/logic QA and require the owner's merge approval.

GitHub's existing `ci.yml` runs legacy Node checks. Passing it does **not** prove that the Jules workflow's Bash or Jules API interaction works.

CodeRabbit can skip draft PRs by default. Request a manual review if necessary. This queue does not publish or merge releases.

## Stop and recovery

To stop **new** tasks, disable **Jules Overnight Queue** in GitHub Actions or remove `jules-approved` from all waiting issues. This does not stop Jules sessions that are already running.

With local dispatch, stopping new tasks means not dispatching. To stop a running session, use `jules-dispatch cancel <sessionId>`; the session ID is in the issue's `Jules session started` comment.

For a failed POST or missing session record, check the Jules session list before restoring any approval label. Do not assume a failed GitHub Actions run means that Jules did not create a session.

The Jules REST API is alpha and can change. See <https://jules.google/docs/api/reference/>.
