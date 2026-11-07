"""Check that every local link and image in the Markdown docs points at a file that exists.

    python scripts/check_docs.py

Used by CI. External links (http/https/mailto) aren't fetched.
"""
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SKIP_DIRS = {".git", "target", "dist", "node_modules", ".claude"}
# Markdown links/images, and HTML src/srcset/href attributes.
PATTERNS = [
    re.compile(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)"),
    re.compile(r"""(?:src|srcset|href)=["']([^"']+)["']"""),
]


def markdown_files():
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            if name.endswith(".md"):
                yield os.path.join(dirpath, name)


def local_target(link: str):
    link = link.strip()
    if not link or link.startswith(("http://", "https://", "mailto:", "#", "data:")):
        return None
    return link.split("#", 1)[0].split("?", 1)[0] or None


def main() -> int:
    problems = []
    checked = 0
    for md in markdown_files():
        text = open(md, encoding="utf-8").read()
        # Ignore fenced code blocks: they show examples, not links.
        text = re.sub(r"```.*?```", "", text, flags=re.S)
        base = os.path.dirname(md)
        for pattern in PATTERNS:
            for match in pattern.finditer(text):
                target = local_target(match.group(1))
                if target is None:
                    continue
                checked += 1
                path = os.path.normpath(os.path.join(ROOT if target.startswith("/") else base, target.lstrip("/")))
                if not os.path.exists(path):
                    problems.append(f"{os.path.relpath(md, ROOT)}: missing {target}")
    for p in problems:
        print(p)
    print(f"checked {checked} local links, {len(problems)} missing")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
