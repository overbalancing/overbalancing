"""Assemble GitHub Pages without changing relative documentation links."""
from pathlib import Path
import shutil

root = Path(__file__).resolve().parents[1]
site = root / "target/site"
if site.exists():
    shutil.rmtree(site)
site.mkdir(parents=True, exist_ok=True)
for name in ("adr", "specs", "docs", "agents"):
    shutil.copytree(root / name, site / name, dirs_exist_ok=True)
for name in ("LICENSE.html", "AGENTS.md"):
    shutil.copy2(root / name, site / name)
shutil.copytree(root / "target/doc", site / "api", dirs_exist_ok=True)
(site / ".nojekyll").touch()
(site / "index.html").write_text('<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Overbalancing</title><meta http-equiv="refresh" content="0; url=docs/index.html"></head><body><a href="docs/index.html">Project documentation</a></body></html>\n')
print(f"Built {site}")
