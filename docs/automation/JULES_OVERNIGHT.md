# Jules overnight queue

This automation belongs only to the public `Ascendance3D/wrlforge`
repository. It does not access the separate private native X_ITE Rust port.

## Activation

1. Create an API key in <https://jules.google.com/settings>.
2. In the WRL Forge repository, open **Settings > Secrets and variables > Actions**.
3. Create the repository secret `JULES_API_KEY`. Never put the key in
   GitHub issues, source files, logs, or chat.
4. Create the repository issue label `jules-approved`.
5. After reviewing and merging the workflow PR, create a small issue
   yourself. Start its title with `JULES-` and put the full task in its body.
6. Apply `jules-approved` to that issue to allow unattended dispatch.

The issue must be authored by `DJAscendance`. Another author's issue
will not be dispatched, even if it has the label.

## When it runs

GitHub Actions starts this workflow at 04:17 and 07:17 UTC each day.
These are approximately 00:17 and 03:17 EDT, or 23:17 and 02:17 EST.
GitHub scheduled runs can be delayed. Manual `workflow_dispatch` is
also available.

Each run selects the lowest-numbered eligible issue and starts **at
most one** Jules session. Thus the normal ceiling is two new Jules
tasks per UTC day, not a guarantee of task completion or concurrency.

Jules uses the public repository `main` branch. API sessions use
`AUTO_CREATE_PR` and do not require separate plan approval. PRs may
be created automatically. The prompt asks Jules to create a draft PR,
but the workflow does not depend on that request being enforced.

## Safety

- The workflow never merges a PR or runs the release workflow.
- The GitHub token has only `contents: read` and `issues: write`.
- Only owner-authored issues with the exact approval label qualify.
- It removes the label before requesting a Jules session.
- If the API response fails or times out, it does **not** silently
  retry. Check the Jules sessions before manually restoring the label.
- Jules has no signing keys or production secrets from this workflow.
- GitHub's normal PR CI and independent reviews remain required.
- CodeRabbit currently skips draft PRs by default. Set up draft review
  separately if you want reviews to run without changing PR status.
- Do not mark already scheduled Jules tasks as approved queue items;
  that would launch duplicate work.

## Owner review

The task starts automatically after you approve its issue for the queue.
This **does not** approve its code. Every PR still needs an owner GO
before it can merge. No automatic merge, publication, or release is allowed.

## Stop

Disable the **Jules Overnight Queue** workflow in GitHub Actions to
stop future dispatches. Existing Jules sessions are not stopped by
disabling this workflow.

The Jules REST API is currently an alpha interface. Check official
documentation before changing its payload fields:
<https://jules.google/docs/api/reference/>.
