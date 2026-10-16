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
# The owner JULES issue that recorded this session: issue 160 = JULES-007.
REC = {"issue": 160, "task": 7}
BRANCH = "jules-160-scope-tests-" + TASK


def pr(sha=S, body=BODY, branch="main", repo=gate.REPO, state="open", ref=BRANCH):
    return {"number": 192, "state": state, "body": body,
            "user": {"login": "DJAscendance"},
            "head": {"sha": sha, "ref": ref,
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
        result = gate.evaluate(pr(), DOC, {TASK: REC}, history(), checks())
        self.assertEqual(result["result"], "REVIEW_CANDIDATE")
        self.assertTrue(result["ci_pass"])
        self.assertFalse(result["merge_authorized"])

    def test_forged_task_link_does_not_prove_identity(self):
        result = gate.evaluate(pr(), DOC, {}, history(), checks())
        self.assertEqual(result["result"], "OWNER_REVIEW")

    def test_bad_task_link_is_refused(self):
        bad = BODY.replace("/task/23456789", "/task/99999999")
        self.assertIsNone(gate.verified_task(bad))
        self.assertEqual(gate.evaluate(pr(body=bad), DOC, {TASK: REC}, history())["result"], "OWNER_REVIEW")

    def test_fork_and_wrong_base_are_blocked(self):
        for case in (pr(repo="attacker/repo"), pr(branch="staging"), pr(state="closed")):
            with self.subTest(case=case):
                self.assertEqual(gate.evaluate(case, DOC, {TASK: REC}, history())["result"], "BLOCKED")

    def test_other_author_or_missing_session_branch_not_eligible(self):
        other = pr()
        other["user"]["login"] = "unverified"
        self.assertEqual(gate.evaluate(other, DOC, {TASK: REC}, history())["result"], "OWNER_REVIEW")
        other = pr()
        other["head"]["ref"] = "unlinked-branch"
        self.assertEqual(gate.evaluate(other, DOC, {TASK: REC}, history())["result"], "OWNER_REVIEW")

    def verdict(self, ref, sessions=None, body=BODY):
        sessions = {TASK: REC} if sessions is None else sessions
        return gate.evaluate(pr(ref=ref, body=body), DOC, sessions, history())["result"]

    def test_known_branch_formats_with_valid_links(self):
        cases = [
            ("issue-160-scope-tests-" + TASK, REC),
            ("jules-160-scope-tests-" + TASK, REC),
            ("jules-007-scope-tests-" + TASK, REC),
            ("issue-169-linux-smoke-" + TASK, {"issue": 169, "task": 16}),
            ("jules-009-crash-recovery-tests-" + TASK, {"issue": 162, "task": 9}),
        ]
        for ref, rec in cases:
            with self.subTest(ref=ref):
                self.assertEqual(self.verdict(ref, {TASK: rec}), "REVIEW_CANDIDATE")

    def test_wrong_issue_number_is_refused(self):
        for ref in ("issue-161-x-" + TASK, "jules-161-x-" + TASK, "jules-008-x-" + TASK,
                    "issue-007-x-" + TASK):
            with self.subTest(ref=ref):
                self.assertEqual(self.verdict(ref), "OWNER_REVIEW")

    def test_wrong_session_is_refused(self):
        other = "98765432"
        # Branch names another session than the PR body.
        self.assertEqual(self.verdict("jules-160-x-" + other, {TASK: REC, other: REC}), "OWNER_REVIEW")
        # PR body and branch agree, but the recorded session is another one.
        self.assertEqual(self.verdict(BRANCH, {other: REC}), "OWNER_REVIEW")

    def test_missing_issue_record_is_refused(self):
        self.assertEqual(self.verdict(BRANCH, {}), "OWNER_REVIEW")
        self.assertEqual(self.verdict(BRANCH, {TASK: None}), "OWNER_REVIEW")
        self.assertEqual(self.verdict(BRANCH, {TASK: {"issue": None, "task": 7}}), "OWNER_REVIEW")

    def test_false_numeric_matches_are_refused(self):
        refs = [
            "issue-1160-x-" + TASK,           # issue number with extra digit
            "issue-16-x-" + TASK,             # issue number with a digit missing
            "jules-0160-x-" + TASK,           # padded issue number
            "jules-07-x-" + TASK,             # task number with wrong padding
            "jules-0007-x-" + TASK,
            "jules-160-x-9" + TASK,           # session with an extra leading digit
            "jules-160-x-" + TASK + "9",      # session with an extra trailing digit
            "jules-160-" + TASK + "-x",       # session not the last token
            "jules-160-x" + TASK,             # session glued to a word
            "jules-task-" + TASK,
            "jules-" + TASK + "-fix",
            "feature/jules-160-x-" + TASK,    # unknown branch prefix
            "docs/x-" + TASK,
        ]
        for ref in refs:
            with self.subTest(ref=ref):
                self.assertEqual(self.verdict(ref), "OWNER_REVIEW")

    def test_branch_link_parser(self):
        self.assertEqual(gate.branch_link("issue-169-a-b-18344116598473231395"),
                         ("issue", "169", "18344116598473231395"))
        for bad in ("", None, "issue-169", "issue-x-1-2", "issue-169-a-b", "main", "issue-1234567-a-1"):
            with self.subTest(ref=bad):
                self.assertIsNone(gate.branch_link(bad))

    def test_body_task_must_match_the_recorded_session(self):
        other_body = BODY.replace("23456789", "98765432")
        self.assertEqual(self.verdict(BRANCH, body=other_body), "OWNER_REVIEW")

    def test_issue_sessions_bind_comment_to_its_own_issue(self):
        def issue(n, title, user="DJAscendance", pull=False):
            item = {"number": n, "title": title, "user": {"login": user}}
            if pull:
                item["pull_request"] = {"url": "https://api.github.com/x"}
            return item

        def comment(session, user="github-actions[bot]", text=None):
            return {"user": {"login": user},
                    "body": text or "Jules session started: `sessions/" + session + "`. Wait for a reviewed PR."}

        issues = [issue(160, "JULES-007 — Scope tests"), issue(162, "JULES-009 — Recovery"),
                  issue(170, "JULES-017 — wrong author", user="someone"),
                  issue(171, "Not a Jules issue"), issue(172, "JULES-018 — PR", pull=True),
                  issue(173, "JULES-019 — comment by owner"), issue(174, "JULES-020 — no comment"),
                  issue(175, "JULES-021 — quoted text"),
                  issue(176, "JULES-022 — dup A"), issue(177, "JULES-023 — dup B")]
        comments = {160: [comment("111")], 162: [comment("222")], 170: [comment("333")],
                    171: [comment("444")], 172: [comment("555")],
                    173: [comment("666", user="DJAscendance")], 174: [],
                    175: [comment("777", text="note: Jules session started: `sessions/777`")],
                    176: [comment("888")], 177: [comment("888")]}

        def pages(route, limit=12):
            if "/issues?state=all" in route:
                return issues
            return comments[int(route.split("/issues/")[1].split("/")[0])]

        with patch.object(gate, "all_pages", side_effect=pages):
            got = gate.issue_sessions()
        self.assertEqual(got, {"111": {"issue": 160, "task": 7},
                               "222": {"issue": 162, "task": 9}})

    def test_missing_sha_does_not_call_check_runs_endpoint(self):
        bad = pr(sha=None)
        with (patch.object(gate, "issue_sessions", return_value={TASK: REC}),
              patch.object(gate, "all_pages") as pages,
              patch.object(gate, "api") as api_mock,
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
        self.assertEqual(gate.evaluate(pr(sha="moving"), DOC, {TASK: REC}, history())["result"], "BLOCKED")

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
        got = gate.evaluate(pr(), DOC, {TASK: REC}, history(reviewed=True))
        self.assertEqual(got["result"], "OWNER_REVIEW")
        self.assertIn("already reviewed", got["reason"])

    def test_three_attempts_hold(self):
        got = gate.evaluate(pr(), DOC, {TASK: REC}, history(attempts=3))
        self.assertIn("three correction attempts", got["reason"])

    def test_no_op_never_auto_merges(self):
        got = gate.evaluate(pr(), [], {TASK: REC}, history(), checks())
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
        altered = checks()[1:]
        self.assertFalse(gate.ci_gate(altered, S))
        self.assertTrue(gate.ci_gate(checks()[:1], S))
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
