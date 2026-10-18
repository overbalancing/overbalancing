"""Exercise approval transitions against actual temporary Git histories."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest

CHECK = Path(__file__).with_name("check_pr.py").resolve()

class PullRequestChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        (self.root / "adr").mkdir()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True).strip()

    def commit(self, html):
        (self.root / "adr/0001-test.html").write_text(html)
        self.git("add", ".")
        self.git("commit", "-qm", "test")
        return self.git("rev-parse", "HEAD")

    def check(self, base, author="Bot"):
        return subprocess.run(["python3", str(CHECK)], cwd=self.root, capture_output=True, text=True,
                              env={**os.environ, "BASE_SHA": base, "HEAD_SHA": "HEAD", "PR_AUTHOR_TYPE": author})

    def test_bot_proposed_prose_mention_allowed(self):
        base = self.commit("<p>Status: proposed</p>")
        self.commit("<p>Status: <strong>proposed</strong>. Owner must merge approved ADRs.</p>")
        self.assertEqual(self.check(base).returncode, 0)

    def test_bot_approval_blocked(self):
        base = self.commit("<p>Status: proposed</p>")
        self.commit('<meta name="adr-status" content="approved"><p>Status: approved</p>')
        self.assertNotEqual(self.check(base).returncode, 0)

    def test_human_approval_warns(self):
        base = self.commit("<p>Status: proposed</p>")
        self.commit("<p>Status: approved</p>")
        result = self.check(base, "User")
        self.assertEqual(result.returncode, 0)
        self.assertIn("owner must inspect and merge personally", result.stdout)

    def test_existing_approved_edit_allowed(self):
        base = self.commit("<p>Status: approved</p>")
        self.commit("<p>Status: approved</p><p>Clarification</p>")
        self.assertEqual(self.check(base).returncode, 0)

    def test_seven_commits_rejected(self):
        base = self.commit("<p>Status: proposed</p>")
        for index in range(7):
            self.commit(f"<p>Status: proposed</p><p>{index}</p>")
        self.assertNotEqual(self.check(base).returncode, 0)

    def test_missing_and_ambiguous_rejected(self):
        base = self.commit("<p>Status: proposed</p>")
        for content in ("<p>No status</p>", '<meta name="adr-status" content="approved"><p>Status: proposed</p>'):
            with self.subTest(content=content):
                self.commit(content)
                self.assertNotEqual(self.check(base).returncode, 0)

if __name__ == "__main__":
    unittest.main()
