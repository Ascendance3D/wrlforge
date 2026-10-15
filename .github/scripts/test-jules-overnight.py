#!/usr/bin/env python3
"""Offline safety tests for the Jules overnight GitHub Actions shell step.

Runs only local mock commands. Never uses a real GitHub/Jules credential.
Usage: python3 .github/scripts/test-jules-overnight.py
"""
from __future__ import annotations
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/jules-overnight.yml"
PLACEHOLDER = "$" + "{{ inputs.dry_run }}"

GH_MOCK = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
case = os.environ["MOCK_CASE"]
log = Path(os.environ["MOCK_LOG"])
def log_action(s):
    with log.open("a") as f: f.write(s + "\n")
if args[:2] == ["issue", "list"]:
    valid = {"number": 161, "title": "JULES-TEST valid", "body": "Add one focused test", "author": {"login": "DJAscendance"}}
    bad = {"number": 160, "title": "JULES-TEST invalid", "body": "", "author": {"login": "DJAscendance"}}
    print(json.dumps([bad, valid] if case in ("invalid", "bad_actor") else ([valid] if case != "empty" else [])))
elif args[0] == "api":
    num = int(args[2].split("/")[3])
    who = "someone-else" if case == "bad_actor" and num == 160 else "DJAscendance"
    print(json.dumps([{"event": "labeled", "label": {"name": "jules-approved"}, "actor": {"login": who}}]))
elif args[:2] == ["issue", "edit"]:
    log_action("EDIT " + args[2])
elif args[:2] == ["issue", "comment"]:
    log_action("COMMENT " + args[2])
else:
    raise SystemExit("Unexpected gh command: " + repr(args))
'''

CURL_MOCK = r'''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
case = os.environ["MOCK_CASE"]
log = Path(os.environ["MOCK_LOG"])
with log.open("a") as f:
    if "-X" in args and args[args.index("-X") + 1] == "POST":
        f.write("POST\n")
        if case == "post_fail": raise SystemExit(22)
        print(json.dumps({"name": "sessions/12345"}))
    else:
        f.write("GET\n")
        repo = {"owner": "Ascendance3D", "repo": "wrlforge",
                "defaultBranch": {"displayName": "main"}, "branches": [{"displayName": "main"}]}
        src = {"name": "sources/github-Ascendance3D-wrlforge", "githubRepo": repo}
        if case == "missing_source": src["githubRepo"]["repo"] = "other"
        if "pageSize" in args[-1]:
            print(json.dumps({"sources": [src]}))
        else:
            print(json.dumps(src))
'''

def step_script(dry_run: bool) -> str:
    lines = WORKFLOW.read_text().splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == "run: |")
    except StopIteration as exc:
        raise AssertionError("No shell step found") from exc
    block = []
    for line in lines[start + 1:]:
        if line and not line.startswith("          "):
            break
        block.append(line[10:] if line.startswith("          ") else "")
    script = "\n".join(block).replace(PLACEHOLDER, "true" if dry_run else "false")
    assert PLACEHOLDER not in script
    return script + "\n"

def trial(case: str, dry_run: bool, key: bool = True, repo: str = "Ascendance3D/wrlforge"):
    with tempfile.TemporaryDirectory() as folder:
        folder = Path(folder)
        log = folder / "log"
        log.write_text("")
        for name, data in (("gh", GH_MOCK), ("curl", CURL_MOCK)):
            exe = folder / name
            exe.write_text(data)
            exe.chmod(0o700)
        env = os.environ.copy()
        env.update({
            "PATH": str(folder) + os.pathsep + env["PATH"],
            "MOCK_CASE": case, "MOCK_LOG": str(log),
            "GITHUB_REPOSITORY": repo, "GITHUB_REF": "refs/heads/main",
            "GITHUB_EVENT_NAME": "workflow_dispatch", "JULES_API_KEY": "dummy-test-value" if key else "",
        })
        result = subprocess.run(["bash", "-euo", "pipefail", "-c", step_script(dry_run)],
                                env=env, text=True, capture_output=True)
        return result, log.read_text().splitlines()

class SafetyTests(unittest.TestCase):
    def test_shell_syntax(self):
        for dry in (True, False):
            result = subprocess.run(["bash", "-n"], input=step_script(dry),
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_dry_run_has_no_writes_or_api_calls(self):
        result, log = trial("valid", True, key=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("DRY RUN", result.stdout)
        self.assertEqual(log, [])

    def test_live_dispatch_once(self):
        result, log = trial("valid", False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, ["GET", "GET", "EDIT 161", "POST", "COMMENT 161"])

    def test_invalid_task_does_not_block_next(self):
        result, log = trial("invalid", False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, ["EDIT 160", "COMMENT 160", "GET", "GET", "EDIT 161", "POST", "COMMENT 161"])

    def test_untrusted_approval_actor_is_skipped(self):
        result, log = trial("bad_actor", False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, ["EDIT 160", "COMMENT 160", "GET", "GET", "EDIT 161", "POST", "COMMENT 161"])

    def test_empty_queue_has_no_effect(self):
        result, log = trial("empty", False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(log, [])

    def test_missing_key_preserves_approval(self):
        result, log = trial("valid", False, key=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(log, [])

    def test_missing_source_preserves_approval(self):
        result, log = trial("missing_source", False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(log, ["GET"])

    def test_failed_post_is_not_retried(self):
        result, log = trial("post_fail", False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(log, ["GET", "GET", "EDIT 161", "POST"])

    def test_wrong_repository_refused(self):
        result, log = trial("valid", False, repo="other/repo")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(log, [])

if __name__ == "__main__":
    unittest.main(verbosity=2)
