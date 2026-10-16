#!/usr/bin/env python3
"""Offline safety tests for the staged Jules/Claude review gate."""
import importlib.util
import os
from pathlib import Path
import unittest
from unittest.mock import patch

MODULE_PATH = Path(__file__).with_name("jules_claude_review.py")
spec = importlib.util.spec_from_file_location("jules_claude_review", MODULE_PATH)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

S = "a" * 40
TASK = "23456789"
BODY = ("PR created automatically by Jules for task "
        "[23456789](https://jules.google.com/task/23456789)")
DOC = ["docs/reviews/NEW_FEATURE.md"]


def pr(sha=S, body=BODY, branch="main", repo=gate.REPO, state="open"):
    return {"number": 192, "state": state, "body": body,
            "user": {"login": "DJAscendance"},
            "head": {"sha": sha, "ref": "jules-task-" + TASK,
                     "repo": {"full_name": repo}},
            "base": {"ref": branch}}


def history(reviewed=False, attempts=0):
    return {"already_reviewed": reviewed, "failed_attempts": attempts,
            "attempt_limit_reached": attempts >= 3}


def checks(sha=S):
    return [{"name": "test (" + x + ")", "status": "completed",
             "conclusion": "success", "head_sha": sha}
            for x in ("ubuntu", "macos", "windows")]


class GateTests(unittest.TestCase):
    def test_linked_session_and_plain_doc_proposes_review_only(self):
        result = gate.evaluate(pr(), DOC, {TASK: 160}, history(), checks())
        self.assertEqual(result["result"], "REVIEW_CANDIDATE")
        self.assertTrue(result["ci_pass"])
        self.assertFalse(result["merge_authorized"])

    def test_forged_task_link_does_not_prove_identity(self):
        result = gate.evaluate(pr(), DOC, {}, history(), checks())
        self.assertEqual(result["result"], "OWNER_REVIEW")

    def test_bad_task_link_is_refused(self):
        bad = BODY.replace("/task/23456789", "/task/99999999")
        self.assertIsNone(gate.verified_task(bad))
        self.assertEqual(gate.evaluate(pr(body=bad), DOC, {TASK: 160}, history())["result"], "OWNER_REVIEW")

    def test_fork_and_wrong_base_are_blocked(self):
        for case in (pr(repo="attacker/repo"), pr(branch="staging"), pr(state="closed")):
            with self.subTest(case=case):
                self.assertEqual(gate.evaluate(case, DOC, {TASK: 160}, history())["result"], "BLOCKED")

    def test_other_author_or_missing_session_branch_not_eligible(self):
        other = pr()
        other["user"]["login"] = "unverified"
        self.assertEqual(gate.evaluate(other, DOC, {TASK: 160}, history())["result"], "OWNER_REVIEW")
        other = pr()
        other["head"]["ref"] = "unlinked-branch"
        self.assertEqual(gate.evaluate(other, DOC, {TASK: 160}, history())["result"], "OWNER_REVIEW")

    def test_task_id_must_be_full_branch_numeric_token(self):
        other = pr()
        other["head"]["ref"] = "jules-task-9" + TASK
        self.assertEqual(
            gate.evaluate(other, DOC, {TASK: 160}, history())["result"],
            "OWNER_REVIEW"
        )
        other["head"]["ref"] = "jules-task-" + TASK + "9"
        self.assertEqual(
            gate.evaluate(other, DOC, {TASK: 160}, history())["result"],
            "OWNER_REVIEW"
        )
        # The real session ID may be bracketed by non-digit branch parts.
        other["head"]["ref"] = "jules-" + TASK + "-fix"
        self.assertEqual(
            gate.evaluate(other, DOC, {TASK: 160}, history())["result"],
            "REVIEW_CANDIDATE"
        )

    def test_missing_sha_does_not_call_check_runs_endpoint(self):
        bad = pr(sha=None)
        with patch.object(gate, "issue_sessions", return_value={TASK: 160}), (
             patch.object(gate, "all_pages") as pages), (
             patch.object(gate, "api") as api_mock), (
             patch.dict(os.environ, {"GH_TOKEN": "test-token"})):
            def page(route, limit=12):
                if "/pulls?state=open" in route:
                    return [bad]
                if "/pulls/192/files" in route:
                    return [{"filename": DOC[0]}]
                if "/issues/192/comments" in route:
                    return []
                raise AssertionError("unexpected route: " + route)
            pages.side_effect = page
            got = gate.scan(0)
            self.assertEqual(got[0]["result"], "BLOCKED")
            self.assertFalse(got[0]["ci_pass"])
            api_mock.assert_not_called()

    def test_ci_from_another_commit_never_counts(self):
        self.assertFalse(gate.ci_gate(checks("b" * 40), S))

    def test_head_sha_is_required(self):
        self.assertEqual(gate.evaluate(pr(sha="moving"), DOC, {TASK: 160}, history())["result"], "BLOCKED")

    def test_new_head_needs_new_review(self):
        marker = [{"body": "WRL-CLAUDE-REVIEW sha=" + S + " result=PASS",
                   "user": {"login": "github-actions[bot]"}}]
        newer = "b" * 40
        self.assertTrue(gate.review_history(marker, S)["already_reviewed"])
        self.assertFalse(gate.review_history(marker, newer)["already_reviewed"])

    def test_cannot_spoof_review_marker_from_pr_author(self):
        marker = [{"body": "WRL-CLAUDE-REVIEW sha=" + S + " result=PASS",
                   "user": {"login": "DJAscendance"}}]
        self.assertFalse(gate.review_history(marker, S)["already_reviewed"])

    def test_duplicate_comment_is_suppressed(self):
        got = gate.evaluate(pr(), DOC, {TASK: 160}, history(reviewed=True))
        self.assertEqual(got["result"], "OWNER_REVIEW")
        self.assertIn("already reviewed", got["reason"])

    def test_three_attempts_hold(self):
        got = gate.evaluate(pr(), DOC, {TASK: 160}, history(attempts=3))
        self.assertIn("three correction attempts", got["reason"])

    def test_no_op_never_auto_merges(self):
        got = gate.evaluate(pr(), [], {TASK: 160}, history(), checks())
        self.assertEqual(got["result"], "OWNER_REVIEW")
        self.assertFalse(got["merge_authorized"])

    def test_security_policy_and_src_are_owner_review(self):
        for p in ["docs/reviews/SECURITY_AUDIT.md", ".github/workflows/ci.yml",
                  "crates/wrlforge-vrml/src/scope/tests.rs", "README.md",
                  "apps/desktop-tauri/src-tauri/src/recovery.rs", "package.json",
                  "docs/reviews/DEPENDENCIES.md"]:
            with self.subTest(path=p):
                self.assertEqual(gate.classify_paths([p]), "OWNER_REVIEW")

    def test_scope_limits(self):
        self.assertEqual(gate.classify_paths(["docs/a.md"] * 5), "OWNER_REVIEW")
        self.assertEqual(gate.classify_paths(["../.env.local"]), "BLOCKED")
        self.assertEqual(gate.classify_paths(["/home/owner/key"]), "BLOCKED")

    def test_mixed_paths_require_owner(self):
        self.assertEqual(gate.classify_paths(DOC + ["crates/wrlforge-vrml/src/scope/tests.rs"]), "OWNER_REVIEW")

    def test_missing_and_failed_ci_are_holds(self):
        self.assertFalse(gate.ci_gate([], S))
        altered = checks()[:2]
        self.assertFalse(gate.ci_gate(altered, S))
        fail = checks()
        fail[0]["conclusion"] = "failure"
        self.assertFalse(gate.ci_gate(fail, S))

    def test_api_calls_use_get_and_fixed_repo_only(self):
        with patch.object(gate.subprocess, "run") as runner:
            runner.return_value.stdout = "{}"
            gate.api("/repos/" + gate.REPO + "/issues/160")
            args = runner.call_args[0][0]
            self.assertEqual(args[1:4], ["api", "-X", "GET"])
            self.assertNotIn("POST", args)
        with self.assertRaises(ValueError):
            gate.api("/repos/private/other/issues/2")

    def test_failure_of_inventory_is_not_reported_as_empty_success(self):
        with patch.object(gate, "api", return_value={"error": "rate limited"}):
            with self.assertRaises(ValueError):
                gate.all_pages("/repos/" + gate.REPO + "/pulls?state=open")

    def test_review_history_counts_failures(self):
        comments = [{"user": {"login": "claude[bot]"},
                     "body": "WRL-CLAUDE-REVIEW sha=" + c + " result=CHANGES_REQUIRED"}
                    for c in ("1" * 40, "2" * 40, "3" * 40)]
        h = gate.review_history(comments, S)
        self.assertEqual(h["failed_attempts"], 3)
        self.assertTrue(h["attempt_limit_reached"])

    def test_missing_key_stops_scan(self):
        with patch.dict(os.environ, {"GH_TOKEN": "", "GITHUB_TOKEN": ""}):
            with self.assertRaises(RuntimeError):
                gate.scan(0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
