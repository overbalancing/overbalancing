"""Check authored HTML local links without network access or dependencies."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import sys

root = Path(__file__).resolve().parents[1]
generated_site = "--site" in sys.argv
if generated_site:
    root = root / "target/site"
errors = []

class Links(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.links = []
        self.ids = set()

    def handle_starttag(self, tag, attrs):
        values = dict(attrs)
        if "id" in values:
            self.ids.add(values["id"])
        for attr in ("href", "src"):
            if values.get(attr):
                self.links.append(values[attr])

files = [root / "LICENSE.html"]
for folder in ("docs", "adr", "specs", "agents"):
    files.extend((root / folder).rglob("*.html"))
parsed = {}
for path in files:
    parser = Links()
    parser.feed(path.read_text())
    parsed[path.resolve()] = parser
for path, parser in parsed.items():
    for link in parser.links:
        url = urlsplit(link)
        if url.scheme or url.netloc:
            continue
        if url.path.startswith("/"):
            errors.append(f"{path.relative_to(root)}: site-relative URL breaks repository Pages: {link}")
            continue
        target = (path.parent / unquote(url.path)).resolve() if url.path else path
        if target.is_dir():
            target = target / "index.html"
        # API docs are generated separately by cargo doc.
        if not generated_site and target.is_relative_to(root / "api"):
            continue
        if not target.is_relative_to(root) or not target.exists():
            errors.append(f"{path.relative_to(root)}: missing local target {link}")
        elif url.fragment and target in parsed and unquote(url.fragment) not in parsed[target].ids:
            errors.append(f"{path.relative_to(root)}: missing anchor {link}")
if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)
print(f"Validated local links in {len(parsed)} HTML documents")
