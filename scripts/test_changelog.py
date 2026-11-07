"""Tests for scripts/changelog.py.  Run: python scripts/test_changelog.py"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import changelog as cl  # noqa: E402

SAMPLE = """# Changelog

Intro text.

## [Unreleased]

### Fixed

- A thing.

## [0.2.0] - 2026-12-01

### Added

- Feature two.

## [0.1.0] - 2026-11-07

### Added

- Feature one.

[Unreleased]: https://github.com/tyler-Github/risquette/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tyler-Github/risquette/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tyler-Github/risquette/releases/tag/v0.1.0
"""


class ChangelogTests(unittest.TestCase):
    def test_round_trip_and_valid(self):
        log = cl.parse(SAMPLE)
        self.assertEqual([s.name for s in log.sections], ["Unreleased", "0.2.0", "0.1.0"])
        self.assertEqual(cl.check(log), [])
        self.assertEqual(cl.render(cl.parse(SAMPLE)), SAMPLE)

    def test_notes_include_body_and_compare_link(self):
        notes = cl.notes(cl.parse(SAMPLE), "v0.2.0")
        self.assertTrue(notes.startswith("### Added\n\n- Feature two."))
        self.assertIn("compare/v0.1.0...v0.2.0", notes)
        # The first release has nothing to compare against.
        self.assertNotIn("compare", cl.notes(cl.parse(SAMPLE), "0.1.0"))
        with self.assertRaises(cl.ChangelogError):
            cl.notes(cl.parse(SAMPLE), "9.9.9")

    def test_prepare_moves_unreleased_and_updates_links(self):
        log = cl.prepare(cl.parse(SAMPLE), "0.3.0", "2026-12-24")
        self.assertEqual([s.name for s in log.sections], ["Unreleased", "0.3.0", "0.2.0", "0.1.0"])
        self.assertEqual(log.section("Unreleased").body, "")
        self.assertIn("- A thing.", log.section("0.3.0").body)
        self.assertTrue(log.links["Unreleased"].endswith("compare/v0.3.0...HEAD"))
        self.assertTrue(log.links["0.3.0"].endswith("compare/v0.2.0...v0.3.0"))
        self.assertEqual(cl.check(cl.parse(cl.render(log))), [])

    def test_prepare_refuses_bad_versions_and_empty_notes(self):
        for bad in ["0.2.0", "0.1.5", "banana"]:
            with self.assertRaises(cl.ChangelogError, msg=bad):
                cl.prepare(cl.parse(SAMPLE), bad, "2026-12-24")
        empty = cl.prepare(cl.parse(SAMPLE), "0.3.0", "2026-12-24")
        with self.assertRaises(cl.ChangelogError):
            cl.prepare(empty, "0.4.0", "2026-12-25")

    def test_prereleases_order_before_their_release(self):
        self.assertLess(cl.version_key("0.3.0-beta.1"), cl.version_key("0.3.0"))
        self.assertLess(cl.version_key("0.3.0-beta.1"), cl.version_key("0.3.0-beta.2"))
        self.assertLess(cl.version_key("0.2.9"), cl.version_key("0.3.0-beta.1"))
        log = cl.prepare(cl.parse(SAMPLE), "0.3.0-beta.1", "2026-12-24")
        self.assertEqual(cl.check(cl.parse(cl.render(log))), [])

    def test_check_reports_problems(self):
        broken = SAMPLE.replace("## [0.2.0] - 2026-12-01", "## [0.2.0]").replace("### Fixed", "### Misc")
        problems = " ".join(cl.check(cl.parse(broken)))
        self.assertIn("needs a date", problems)
        self.assertIn("unknown heading", problems)
        swapped = SAMPLE.replace("[0.2.0] - 2026-12-01", "[0.0.9] - 2026-12-01")
        self.assertIn("newest first", " ".join(cl.check(cl.parse(swapped))))

    def test_workspace_version_bump(self):
        toml = '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\n'
        self.assertIn('version = "0.2.0"', cl.set_workspace_version(toml, "0.2.0"))
        with self.assertRaises(cl.ChangelogError):
            cl.set_workspace_version("[package]\nversion = \"1.0.0\"\n", "2.0.0")

    def test_real_changelog_is_valid(self):
        log = cl.parse(cl.read(cl.CHANGELOG))
        self.assertEqual(cl.check(log), [])
        self.assertTrue(cl.notes(log, "0.1.0").strip())


if __name__ == "__main__":
    unittest.main()
