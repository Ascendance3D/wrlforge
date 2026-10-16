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
SESSION_COMMENT = re.compile(r"Jules session started: .?sessions/([0-9]+)")
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
    observed = {x.get("name") for x in relevant}
    expected = {"test (ubuntu)", "test (macos)", "test (windows)"}
    if not expected.issubset(observed):
        return False
    return all(
        sum(1 for x in relevant if x.get("name") == n and
            x.get("status") == "completed" and x.get("conclusion") == "success"
            and x.get("head_sha") == sha) == 1
        for n in expected
    )


def evaluate(pr, paths, confirmed_sessions, history, checks=None):
    """Return a *proposal*, never an approval or merge decision."""
    head = (pr.get("head") or {})
    base = (pr.get("base") or {})
    sha = head.get("sha", "")
    source = (head.get("repo") or {}).get("full_name")
    task_id = verified_task(pr.get("body"))
    ref = head.get("ref", "")
    author = (pr.get("user") or {}).get("login")
    result = {"pr": pr.get("number"), "head_sha": sha, "result": "OWNER_REVIEW",
              "category": classify_paths(paths), "reason": ""}
    if (pr.get("state") != "open" or not isinstance(sha, str) or not SHA.fullmatch(sha) or
            base.get("ref") != "main" or source != REPO):
        result.update(result="BLOCKED", reason="invalid PR state, SHA, base, or source repository")
    elif result["category"] == "BLOCKED":
        result.update(result="BLOCKED", reason="invalid file path")
    elif (not task_id or task_id not in confirmed_sessions or
          author != "DJAscendance" or
          not re.search(r"(?<![0-9])" + re.escape(task_id) + r"(?![0-9])", ref)):
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
    """Map session IDs proved by owner-authored JULES issues and Actions comments."""
    sessions = {}
    issues = all_pages("/repos/" + REPO + "/issues?state=all", limit=20)
    for issue in issues:
        if issue.get("pull_request") or not (issue.get("title") or "").startswith("JULES-"):
            continue
        if (issue.get("user") or {}).get("login") != "DJAscendance":
            continue
        number = issue.get("number")
        if not isinstance(number, int):
            continue
        comments = all_pages("/repos/" + REPO + "/issues/" + str(number) + "/comments")
        for entry in comments:
            if (entry.get("user") or {}).get("login") != "github-actions[bot]":
                continue
            m = SESSION_COMMENT.search(entry.get("body") or "")
            if m:
                sessions[m.group(1)] = number
    return sessions


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
        paths = [p.get("filename") for p in all_pages("/repos/" + REPO + "/pulls/" + str(n) + "/files")]
        comments = all_pages("/repos/" + REPO + "/issues/" + str(n) + "/comments")
        sha = (pr.get("head") or {}).get("sha")
        if not isinstance(sha, str) or not SHA.fullmatch(sha):
            # Invalid metadata blocks this PR but must not make a different
            # valid PR disappear from the read-only inventory.
            checks_result = {"check_runs": []}
        else:
            checks_result = api("/repos/" + REPO + "/commits/" + sha + "/check-runs?per_page=100")
        if not isinstance(checks_result, dict) or not isinstance(checks_result.get("check_runs"), list):
            raise ValueError("incomplete check-runs response")
        h = review_history(comments, sha)
        item = evaluate(pr, paths, confirmed, h, checks_result["check_runs"])
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
