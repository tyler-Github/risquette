"""Work with CHANGELOG.md, the source of Risquette's release notes.

    python scripts/changelog.py check              # validate the file (CI runs this)
    python scripts/changelog.py notes 0.2.0        # print the release notes for 0.2.0
    python scripts/changelog.py notes 0.2.0 --out notes.md   # ...or write them (UTF-8) to a file
    python scripts/changelog.py prepare 0.2.0      # turn "Unreleased" into 0.2.0 and bump the version
    python scripts/changelog.py prepare 0.2.0-beta.1   # ...or into a beta; a later `prepare 0.2.0`
                                                       # folds the betas' notes into the stable release
    python scripts/changelog.py prepare 0.2.1      # a bug-fix release: only fixes, no Added notes
    python scripts/changelog.py kind 0.2.1         # print the kind of release: feature, bugfix or beta

Kinds of release: a version with a suffix (0.3.0-beta.1) is a beta; a stable
version ending in .0 (0.3.0) is a feature release; any other stable version
(0.3.1) is a bug-fix release. Bug-fix releases may only fix things, so
`prepare` refuses one whose notes have "Added" entries, and the release
workflow skips re-running the tests for them (CI already passed on the commit).

`prepare` edits CHANGELOG.md, Cargo.toml and Cargo.lock but doesn't commit or
tag anything; it prints the next steps instead.

The file follows Keep a Changelog: an "## [Unreleased]" section, then one
"## [x.y.z] - YYYY-MM-DD" section per release (newest first) using the
headings Added / Changed / Deprecated / Removed / Fixed / Security, and link
references at the bottom.
"""
from __future__ import annotations

import datetime
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHANGELOG = os.path.join(ROOT, "CHANGELOG.md")
CARGO_TOML = os.path.join(ROOT, "Cargo.toml")
REPO = "https://github.com/tyler-Github/risquette"
SUBHEADINGS = {"Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"}

VERSION_RE = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?"
SECTION_RE = re.compile(rf"^## \[(Unreleased|{VERSION_RE})\](?: - (\d{{4}}-\d{{2}}-\d{{2}}))?\s*$")
LINK_RE = re.compile(r"^\[([^\]]+)\]: (\S+)\s*$")


class ChangelogError(Exception):
    pass


@dataclass
class Section:
    name: str  # "Unreleased" or a version
    date: str | None
    body: str  # everything under the heading, without surrounding blank lines


@dataclass
class Changelog:
    intro: str
    sections: list[Section] = field(default_factory=list)
    links: dict[str, str] = field(default_factory=dict)

    @property
    def releases(self) -> list[Section]:
        return [s for s in self.sections if s.name != "Unreleased"]

    def section(self, name: str) -> Section | None:
        return next((s for s in self.sections if s.name == name), None)


def version_key(version: str):
    """Sort key for semantic versions; a pre-release sorts before its release."""
    core, _, pre = version.partition("-")
    nums = tuple(int(p) for p in core.split("."))
    if not pre:
        return nums + ((1,),)
    parts = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split("."))
    return nums + ((0,) + parts,)


def parse(text: str) -> Changelog:
    lines = text.replace("\r\n", "\n").split("\n")
    intro: list[str] = []
    sections: list[Section] = []
    links: dict[str, str] = {}
    current: tuple[str, str | None] | None = None
    body: list[str] = []

    def finish():
        if current is not None:
            sections.append(Section(current[0], current[1], "\n".join(body).strip("\n")))

    for line in lines:
        heading = SECTION_RE.match(line)
        link = LINK_RE.match(line)
        if heading:
            finish()
            current, body = (heading.group(1), heading.group(2)), []
        elif link:
            links[link.group(1)] = link.group(2)
        elif line.startswith("## "):
            raise ChangelogError(f"Unrecognised section heading: {line!r} (expected '## [Unreleased]' or '## [x.y.z] - YYYY-MM-DD')")
        elif current is None:
            intro.append(line)
        else:
            body.append(line)
    finish()
    return Changelog("\n".join(intro).strip("\n"), sections, links)


def render(log: Changelog) -> str:
    out = [log.intro, ""]
    for s in log.sections:
        out.append(f"## [{s.name}]" + (f" - {s.date}" if s.date else ""))
        out.append("")
        if s.body:
            out.append(s.body)
            out.append("")
    for name, url in log.links.items():
        out.append(f"[{name}]: {url}")
    return "\n".join(out).rstrip("\n") + "\n"


def check(log: Changelog) -> list[str]:
    """Problems with the changelog, as messages (empty when it's fine)."""
    problems = []
    if not log.sections or log.sections[0].name != "Unreleased":
        problems.append("The first section must be '## [Unreleased]'.")
    if sum(1 for s in log.sections if s.name == "Unreleased") > 1:
        problems.append("There's more than one Unreleased section.")
    releases = log.releases
    for s in releases:
        if not s.date:
            problems.append(f"Release {s.name} needs a date: '## [{s.name}] - YYYY-MM-DD'.")
        else:
            try:
                datetime.date.fromisoformat(s.date)
            except ValueError:
                problems.append(f"Release {s.name} has an invalid date {s.date!r}.")
        if not s.body.strip():
            problems.append(f"Release {s.name} has no notes.")
    for newer, older in zip(releases, releases[1:]):
        if version_key(newer.name) <= version_key(older.name):
            problems.append(f"Releases must be newest first: {newer.name} is listed above {older.name}.")
    for s in log.sections:
        for line in s.body.split("\n"):
            if line.startswith("### ") and line[4:].strip() not in SUBHEADINGS:
                problems.append(f"In {s.name}: unknown heading {line!r}; use one of {', '.join(sorted(SUBHEADINGS))}.")
        if s.name not in log.links:
            problems.append(f"Missing link reference for [{s.name}] at the bottom of the file.")
    return problems


def notes(log: Changelog, version: str) -> str:
    """The release notes for `version`: its section plus a link to the full diff."""
    version = version.lstrip("v")
    section = log.section(version)
    if section is None:
        raise ChangelogError(f"CHANGELOG.md has no section for {version}. Run: python scripts/changelog.py prepare {version}")
    if not section.body.strip():
        raise ChangelogError(f"The {version} section in CHANGELOG.md is empty.")
    releases = log.releases
    index = next(i for i, s in enumerate(releases) if s.name == version)
    text = section.body.strip()
    if index + 1 < len(releases):
        previous = releases[index + 1].name
        text += f"\n\n**Full changelog:** [v{previous}...v{version}]({REPO}/compare/v{previous}...v{version})"
    return text + "\n"


def is_prerelease(version: str) -> bool:
    return "-" in version


def kind(version: str) -> str:
    """`beta`, `bugfix` (a stable x.y.z with z > 0) or `feature` (a stable x.y.0)."""
    version = version.lstrip("v")
    if not re.fullmatch(VERSION_RE, version):
        raise ChangelogError(f"{version!r} isn't a version like 1.2.3 or 1.2.3-beta.1.")
    if is_prerelease(version):
        return "beta"
    return "bugfix" if int(version.split(".")[2]) > 0 else "feature"


def merge_notes(bodies: list[str]) -> str:
    """Combine several sections' notes into one, oldest first: free text, then each
    heading (Added, Changed, …) with its bullets in order, duplicates dropped."""
    bodies = [b.strip() for b in bodies if b.strip()]
    if len(bodies) <= 1:
        return bodies[0] if bodies else ""
    intro: list[str] = []
    groups: dict[str, list[str]] = {}
    for body in bodies:
        heading = None
        paragraph: list[str] = []
        for line in body.split("\n") + [""]:
            if line.startswith("### "):
                heading = line[4:].strip()
                groups.setdefault(heading, [])
            elif heading is None:
                # Text before the first heading, kept paragraph by paragraph.
                if line.strip():
                    paragraph.append(line)
                elif paragraph:
                    text = "\n".join(paragraph)
                    if text not in intro:
                        intro.append(text)
                    paragraph = []
            elif line.startswith("- "):
                groups[heading].append(line)
            elif line.strip() and groups[heading]:
                # A wrapped continuation of the previous bullet.
                groups[heading][-1] += "\n" + line
    order = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]
    headings = [h for h in order if h in groups] + [h for h in groups if h not in order]
    parts = list(intro)
    for heading in headings:
        items: list[str] = []
        for item in groups[heading]:
            if item not in items:
                items.append(item)
        if items:
            parts.append(f"### {heading}\n\n" + "\n".join(items))
    return "\n\n".join(parts)


def prepare(log: Changelog, version: str, date: str) -> Changelog:
    """Move the Unreleased notes into a new `version` section dated `date`.

    Releasing a stable version after betas of it (0.2.0 after 0.2.0-beta.1, …)
    folds those betas' notes into the new section, so stable users see
    everything that changed since the previous stable release.
    """
    version = version.lstrip("v")
    if not re.fullmatch(VERSION_RE, version):
        raise ChangelogError(f"{version!r} isn't a version like 1.2.3 or 1.2.3-beta.1.")
    if log.section(version):
        raise ChangelogError(f"CHANGELOG.md already has a {version} section.")
    unreleased = log.section("Unreleased")
    if unreleased is None:
        raise ChangelogError("CHANGELOG.md has no Unreleased section.")
    releases = log.releases
    if releases and version_key(version) <= version_key(releases[0].name):
        raise ChangelogError(f"{version} must be newer than the latest release, {releases[0].name}.")

    # Betas of this version, oldest first, when promoting to stable.
    betas = [] if is_prerelease(version) else [s for s in reversed(releases) if is_prerelease(s.name) and s.name.split("-")[0] == version]
    body = merge_notes([b.body for b in betas] + [unreleased.body])
    if not body.strip():
        raise ChangelogError("There's nothing under Unreleased to release. Add the changes there first.")
    if kind(version) == "bugfix" and re.search(r"^### Added\s*$", body, re.M):
        major, minor, _ = version.split(".")
        raise ChangelogError(
            f"{version} is a bug-fix release, but the notes have Added entries. New features go in a "
            f"feature release ({major}.{int(minor) + 1}.0); a bug-fix release only fixes things."
        )

    beta_names = {b.name for b in betas}
    log.sections = [s for s in log.sections if s.name not in beta_names]
    new = Section(version, date, body)
    unreleased.body = ""
    log.sections.insert(log.sections.index(unreleased) + 1, new)

    # Compare against the previous release that's still listed (betas folded away).
    previous = next((s.name for s in log.releases if s.name != version), None)
    links = {"Unreleased": f"{REPO}/compare/v{version}...HEAD"}
    links[version] = f"{REPO}/compare/v{previous}...v{version}" if previous else f"{REPO}/releases/tag/v{version}"
    links.update({k: v for k, v in log.links.items() if k not in links and k not in beta_names})
    log.links = links
    return log


def set_workspace_version(cargo_toml: str, version: str) -> str:
    """Replace `version = "..."` in Cargo.toml's [workspace.package] table."""
    pattern = re.compile(r'(\[workspace\.package\][^\[]*?\nversion\s*=\s*")[^"]*(")', re.S)
    updated, count = pattern.subn(rf"\g<1>{version}\g<2>", cargo_toml, count=1)
    if count != 1:
        raise ChangelogError("Couldn't find version in [workspace.package] in Cargo.toml.")
    return updated


def read(path: str) -> str:
    with open(path, encoding="utf-8") as f:
        return f.read()


def write(path: str, text: str) -> None:
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def main(argv: list[str]) -> int:
    if len(argv) < 2 or argv[1] not in {"check", "notes", "prepare", "kind"}:
        print(__doc__.strip())
        return 2
    command = argv[1]
    try:
        log = parse(read(CHANGELOG))
        if command == "check":
            problems = check(log)
            for p in problems:
                print(f"CHANGELOG.md: {p}")
            if not problems:
                n = len(log.releases)
                print(f"CHANGELOG.md is valid ({n} release{'' if n == 1 else 's'}).")
            return 1 if problems else 0

        if len(argv) < 3:
            print(f"Usage: python scripts/changelog.py {command} <version>")
            return 2
        version = argv[2].lstrip("v")

        if command == "kind":
            print(kind(version))
            return 0

        if command == "notes":
            text = notes(log, version)
            if "--out" in argv:
                write(argv[argv.index("--out") + 1], text)
            else:
                sys.stdout.reconfigure(encoding="utf-8")
                sys.stdout.write(text)
            return 0

        today = datetime.date.today().isoformat()
        write(CHANGELOG, render(prepare(log, version, today)))
        write(CARGO_TOML, set_workspace_version(read(CARGO_TOML), version))
        # Refresh Cargo.lock so `--locked` builds see the new version.
        subprocess.run(["cargo", "update", "--workspace", "--quiet"], cwd=ROOT, check=True)
        label = {"bugfix": "bug-fix release", "feature": "feature release", "beta": "beta"}[kind(version)]
        print(f"Prepared {version} ({label}): CHANGELOG.md, Cargo.toml and Cargo.lock are updated.\n")
        print("Next steps:")
        print("  1. Review the changes (git diff) and edit the notes if you like.")
        print(f'  2. Commit them, e.g. "Release v{version}", and merge to main.')
        print(f"  3. Tag and push: git tag v{version} && git push origin v{version}")
        return 0
    except ChangelogError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
