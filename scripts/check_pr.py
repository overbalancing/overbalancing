"""Validate PR scope and reserve actual ADR approval transitions for owner merge.

Repository rulesets and the lead policy must enforce the eventual merger identity.
"""
from html.parser import HTMLParser
import os
import re
import subprocess
import sys

APPROVAL_STATUSES = {"accepted", "approved"}
STATUSES = {"proposed", "rejected", "superseded"} | APPROVAL_STATUSES

class StatusParser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.text = []
        self.statuses = []
        self.hidden = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag in {"script", "style"}:
            self.hidden += 1
        if tag == "meta" and attrs.get("name", "").lower() == "adr-status":
            self.statuses.append(attrs.get("content", "").lower().strip())

    def handle_endtag(self, tag):
        if tag in {"script", "style"}:
            self.hidden = max(0, self.hidden - 1)

    def handle_data(self, data):
        if not self.hidden:
            self.text.append(data)


def adr_status(content, path):
    parser = StatusParser()
    parser.feed(content)
    values = parser.statuses + re.findall(r"\bStatus\s*:\s*([a-z]+)\b", " ".join(parser.text), re.I)
    values = [value.lower() for value in values]
    if not values or any(value not in STATUSES for value in values) or len(set(values)) != 1:
        raise ValueError(f"{path}: missing or ambiguous ADR status (expected proposed/accepted/approved/rejected/superseded)")
    return values[0]


def git(*args):
    return subprocess.check_output(["git", *args], text=True)


def content_at(revision, path):
    result = subprocess.run(["git", "show", f"{revision}:{path}"], capture_output=True, text=True)
    return result.stdout if result.returncode == 0 else None


def main():
    base, head = os.environ["BASE_SHA"], os.environ["HEAD_SHA"]
    count = int(git("rev-list", "--count", f"{base}..{head}"))
    if not 1 <= count <= 6:
        sys.exit(f"PR must contain 1–6 commits; found {count}. Clean up the branch before review.")
    transition = False
    paths = git("diff", "--name-only", base, head, "--", "adr/").splitlines()
    for path in paths:
        if not re.fullmatch(r"adr/\d[^/]*\.html", path):
            continue
        before, after = content_at(base, path), content_at(head, path)
        previous = adr_status(before, path) if before is not None else None
        current = adr_status(after, path) if after is not None else None
        transition |= current in APPROVAL_STATUSES and previous not in APPROVAL_STATUSES
    if transition:
        print("ADR transitioned to accepted/approved: owner must inspect and merge personally; automated merge is prohibited.")
        if os.environ.get("PR_AUTHOR_TYPE") == "Bot":
            sys.exit("Bot-authored PR includes an ADR approval transition. Owner must take over this change.")
    print(f"PR scope accepted: {count} commits")

if __name__ == "__main__":
    try:
        main()
    except ValueError as error:
        sys.exit(str(error))
