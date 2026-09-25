"""Switch-over helper for the website move (branch chore/move-site-out):
replace the SITE_DOMAIN placeholder in the README and the docs/ redirect.

    python scripts/set_site_domain.py synccrate.app

Run it once the domain serves the site (see the private synccrate-site
repo's README), then commit and merge.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FILES = ["README.md", "docs/index.html"]

if len(sys.argv) != 2 or not re.fullmatch(r"[a-z0-9.-]+\.[a-z]{2,}", sys.argv[1].strip().lower()):
    sys.exit(__doc__)
domain = sys.argv[1].strip().lower()
for rel in FILES:
    path = ROOT / rel
    text = path.read_bytes().decode("utf-8")
    if "SITE_DOMAIN" not in text:
        print(f"{rel}: no placeholder (already set?)")
        continue
    path.write_bytes(text.replace("SITE_DOMAIN", domain).encode("utf-8"))
    print(f"{rel}: set to {domain}")
