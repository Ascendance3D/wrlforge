#!/usr/bin/env python3
"""Read-only Jules PR inventory and strict candidate policy.

This tool does not call Claude, post comments, approve, or merge anything.
Run only on trusted workflow source code. Treat all PR content as data.
"""
import argparse
import json
import os
import re
import subprocess
import sys

REPO = "Ascendance3D/wrlforge"
SHA = re.compile(r"^[0-9a-f]{40}$")
TASK_LINK = re.compile(
    r"PR created automatically by Jules for task \[(\d+)\]"
    r"\(https://jules\.google\.com/task/(\d+)\)"
)
# The dispatcher comment starts exactly like this; text elsewhere is not trusted.
SESSION_COMMENT = re.compile(r"\AJules session started: `sessions/([0-9]+)`")
JULES_TITLE = re.compile(r"\AJULES-([0-9]{1,6})(?![0-9])")
BRANCH_KINDS = ("issue", "jules")
MARKER = re.compile(
    r"WRL-CLAUDE-REVIEW sha=([0-9a-f]{40}) result="
    r"(PASS|CHANGES_REQUIRED|OWNER_REVIEW|BLOCKED|VERIFIED_NO_OP)"
)
FORBIDDEN_PREFIXES = (
    ".github/", "scripts/", "packaging/", "release/", "licenses/",
    "assets/entitlements", "crates/", "apps/", "src/", "tests/security/",
)
SENSITIVE_NAMES = {
    "package.json", "package-lock.json", "Cargo.toml", "Cargo.lock",
    "LICENSE", "SECURITY.md", "CONTRIBUTING.md", ".gitignore",
}
SENSITIVE_TERMS = ("SECURITY", "AUDIT", "AUTH", "LICENSE", "RELEASE", "CREDENTIAL", "DEPENDENC", "PROVENANCE")


def verified_task(body):
    match = TASK_LINK.search(body or "")
    return match.group(1) if match and match.group(1) == match.group(2) else None


def branch_link(ref):
    """Parse issue-<N>-...-<session> and jules-<N>-...-<session> branch names.

    Return (kind, number, session) with all parts as digit strings, or None.
    The session is always the last token, never a substring in the middle.
    """
    parts = (ref or "").split("-")
    if len(parts) < 3 or parts[0] not in BRANCH_KINDS:
        return None
    number, session = parts[1], parts[-1]
    if not (number.isascii() and number.isdigit() and len(number) <= 6):
        return None
    if not (session.isascii() and session.isdigit()):
        return None
    return parts[0], number, session


def session_is_proved(task_id, ref, record):
    """True only if PR task, branch and the recorded issue agree exactly.

    record is {"issue": <issue number>, "task": <JULES task number or None>}
    for the session, taken from the owner-authored JULES issue.
    """
    link = branch_link(ref)
    if not task_id or not link or not isinstance(record, dict):
        return False
    kind, number, session = link
    if session != task_id:
        return False
    issue, task = record.get("issue"), record.get("task")
    if not isinstance(issue, int):
        return False
    if number == str(issue):
        return True
    # A jules-NNN branch may name the JULES task number (JULES-009 -> 009).
    return (kind == "jules" and isinstance(task, int)
            and number == "%03d" % task)


def classify_paths(paths):
    """Only plain non-sensitive docs can be proposed for low-risk automation."""
    if not paths:
        return "VERIFIED_NO_OP_CANDIDATE"
    if len(paths) > 4:
        return "OWNER_REVIEW"
    for path in paths:
        if not isinstance(path, str) or not path or path.startswith("/") or ".." in path.split("/"):
            return "BLOCKED"
        base = path.rsplit("/", 1)[-1]
        if (path.startswith(FORBIDDEN_PREFIXES) or base in SENSITIVE_NAMES
                or any(term in path.upper() for term in SENSITIVE_TERMS)):
            return "OWNER_REVIEW"
        if not (path.startswith("docs/") and path.endswith(".md")):
            return "OWNER_REVIEW"
    return "LOW_RISK_DOC_CANDIDATE"


def review_history(comments, sha):
    results = []
    for item in comments:
        match = MARKER.search(item.get("body") or "")
        # Only trust a comment by the configured review bot.
        actor = (item.get("user") or {}).get("login", "")
        if match and actor in {"github-actions[bot]", "claude[bot]"}:
            results.append((match.group(1), match.group(2)))
    same_sha = [v for commit, v in results if commit == sha]
    corrections = sum(v == "CHANGES_REQUIRED" for _, v in results)
    return {"already_reviewed": bool(same_sha), "failed_attempts": corrections,
            "attempt_limit_reached": corrections >= 3}


def ci_gate(checks, sha):
    """Independent CI is required; action/CodeRabbit success is not CI proof."""
    if not SHA.fullmatch(sha or ""):
        return False
    relevant = [x for x in checks if x.get("name", "").startswith("test (")]
    # Only the Linux PR gate is required for the current product phase.
    expected = {"test (ubuntu)"}
    for name in expected:
        # Exactly one run of each required name, whatever its result: a failed
        # or duplicate run must never hide behind a successful one.
        runs = [x for x in relevant if x.get("name") == name]
        if len(runs) != 1:
            return False
        run = runs[0]
        if not (run.get("status") == "completed" and run.get("conclusion") == "success"
                and run.get("head_sha") == sha):
            return False
    return True


def evaluate(pr, paths, confirmed_sessions, history, checks=None):
    """Return a *proposal*, never an approval or merge decision."""
    head = (pr.get("head") or {})
    base = (pr.get("base") or {})
    sha = head.get("sha", "")
    source = (head.get("repo") or {}).get("full_name")
    task_id = verified_task(pr.get("body"))
    ref = head.get("ref", "")
    author = (pr.get("user") or {}).get("login")
    # The report always carries a printable head_sha, even for invalid metadata.
    shown = sha if isinstance(sha, str) and SHA.fullmatch(sha) else "invalid-head-sha"
    result = {"pr": pr.get("number"), "head_sha": shown, "result": "OWNER_REVIEW",
              "category": classify_paths(paths), "reason": ""}
    if (pr.get("state") != "open" or not isinstance(sha, str) or not SHA.fullmatch(sha) or
            base.get("ref") != "main" or source != REPO):
        result.update(result="BLOCKED", reason="invalid PR state, SHA, base, or source repository")
    elif result["category"] == "BLOCKED":
        result.update(result="BLOCKED", reason="invalid file path")
    elif (author != "DJAscendance" or
          not session_is_proved(task_id, ref, confirmed_sessions.get(task_id))):
        result["reason"] = "Jules identity not proved by owner issue, author and session branch"
    elif result["category"] == "VERIFIED_NO_OP_CANDIDATE":
        result["reason"] = "no changed files; inspect existing tests before closing"
    elif result["category"] != "LOW_RISK_DOC_CANDIDATE":
        result["reason"] = "sensitive or non-doc change requires owner review"
    elif history["attempt_limit_reached"]:
        result["reason"] = "three correction attempts reached"
    elif history["already_reviewed"]:
        result["reason"] = "current SHA already reviewed"
    else:
        result.update(result="REVIEW_CANDIDATE",
                      reason="verified Jules session and plain doc scope; Claude and CI still required")
    # CI is reported for information. It NEVER upgrades an unreviewed candidate.
    result["ci_pass"] = ci_gate(checks or [], sha)
    result["merge_authorized"] = False
    return result


def api(route):
    """Use only GitHub REST GET. No shell expansion of PR input."""
    if not route.startswith("/repos/" + REPO + "/"):
        raise ValueError("non-public repository resource refused")
    cmd = ["gh", "api", "-X", "GET", "-H", "Accept: application/vnd.github+json", route]
    value = subprocess.run(cmd, check=True, capture_output=True, text=True, timeout=35)
    return json.loads(value.stdout)


def all_pages(path, limit=12):
    records = []
    for page in range(1, limit + 1):
        chunk = api(path + ("&" if "?" in path else "?") + "per_page=100&page=" + str(page))
        if not isinstance(chunk, list):
            raise ValueError("GitHub API did not return a list")
        records.extend(chunk)
        if len(chunk) < 100:
            return records
    raise ValueError("pagination limit reached; refuse incomplete inventory")


def issue_sessions():
    """Map session IDs to the owner-authored JULES issue that recorded them.

    Each value is {"issue": number, "task": JULES task number}. A session ID
    recorded on more than one issue is ambiguous and is dropped.
    """
    sessions = {}
    ambiguous = set()
    issues = all_pages("/repos/" + REPO + "/issues?state=all", limit=20)
    for issue in issues:
        if issue.get("pull_request"):
            continue
        title = JULES_TITLE.match(issue.get("title") or "")
        if not title or (issue.get("user") or {}).get("login") != "DJAscendance":
            continue
        number = issue.get("number")
        if not isinstance(number, int):
            continue
        comments = all_pages("/repos/" + REPO + "/issues/" + str(number) + "/comments")
        for entry in comments:
            if (entry.get("user") or {}).get("login") != "github-actions[bot]":
                continue
            m = SESSION_COMMENT.match(entry.get("body") or "")
            if not m:
                continue
            record = {"issue": number, "task": int(title.group(1))}
            if sessions.get(m.group(1), record) != record:
                ambiguous.add(m.group(1))
            sessions[m.group(1)] = record
    for session in ambiguous:
        del sessions[session]
    return sessions


def changed_paths(files):
    """New and old path of every changed file, so a rename cannot hide a source."""
    paths = []
    for entry in files:
        paths.append(entry.get("filename"))
        if entry.get("previous_filename"):
            paths.append(entry.get("previous_filename"))
    return paths


def check_runs(sha, limit=5):
    """Return every check run of a commit, or refuse an incomplete answer."""
    runs = []
    for page in range(1, limit + 1):
        chunk = api("/repos/" + REPO + "/commits/" + sha + "/check-runs?per_page=100&page=" + str(page))
        items = chunk.get("check_runs") if isinstance(chunk, dict) else None
        total = chunk.get("total_count") if isinstance(chunk, dict) else None
        if not isinstance(items, list) or not isinstance(total, int):
            raise ValueError("incomplete check-runs response")
        runs.extend(items)
        if len(runs) >= total:
            return runs
        if not items:
            break
    raise ValueError("check-runs pagination incomplete; refuse partial CI evidence")


def scan(number):
    if not os.getenv("GH_TOKEN") and not os.getenv("GITHUB_TOKEN"):
        raise RuntimeError("GH_TOKEN or GITHUB_TOKEN is required for read-only scan")
    confirmed = issue_sessions()
    prs = all_pages("/repos/" + REPO + "/pulls?state=open")
    if number:
        prs = [p for p in prs if p.get("number") == number]
        if not prs:
            raise ValueError("selected PR is not open")
    report = []
    for pr in prs:
        n = pr.get("number")
        if not isinstance(n, int):
            continue
        body = pr.get("body") or ""
        # Only target PRs that declare a Jules session, with no trust yet in that claim.
        if "PR created automatically by Jules for task" not in body:
            continue
        paths = changed_paths(all_pages("/repos/" + REPO + "/pulls/" + str(n) + "/files"))
        comments = all_pages("/repos/" + REPO + "/issues/" + str(n) + "/comments")
        sha = (pr.get("head") or {}).get("sha")
        if not isinstance(sha, str) or not SHA.fullmatch(sha):
            # Invalid metadata blocks this PR but must not make a different
            # valid PR disappear from the read-only inventory.
            runs = []
        else:
            runs = check_runs(sha)
        h = review_history(comments, sha)
        item = evaluate(pr, paths, confirmed, h, runs)
        item["correction_attempts"] = h["failed_attempts"]
        item["files"] = paths
        report.append(item)
    return report


def main():
    parser = argparse.ArgumentParser(description="Read-only Jules review inventory")
    parser.add_argument("--pr", type=int, default=0, help="PR number, or zero for all open Jules PRs")
    args = parser.parse_args()
    try:
        result = scan(args.pr)
        print(json.dumps({"repo": REPO, "mode": "DRY_RUN_ONLY", "merge_enabled": False,
                          "reviews": result}, indent=2))
    except (ValueError, RuntimeError, subprocess.SubprocessError, json.JSONDecodeError) as exc:
        print("REFUSED: inventory incomplete or API failed: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
