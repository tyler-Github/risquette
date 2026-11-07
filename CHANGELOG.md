# Changelog

All notable changes to Risquette. Each release's section becomes its release notes on GitHub and the "What's new" text in the app's update dialog, so entries are written for people using the app.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- If Risquette crashes, it now shows a window explaining what happened, with a button that opens a bug report on GitHub with the details already filled in. You can also restart the app or copy the details from there.
- Risquette keeps a log file and saves crash reports, which help us track down problems.
- A Help & troubleshooting section in Settings, with links to report a problem and to open the logs folder.

### Changed

- The setup wizard now says "Reinstall" rather than "Update" when the same version is already installed.
- Choosing "Also remove my settings and saved logins" when uninstalling now removes logs and crash reports too.

### Fixed

- Opening `uninstall.exe` from the install folder now opens the uninstaller instead of the setup wizard.
- Text boxes in Settings and the "Save to list" dialog no longer shrink to a sliver after the page refreshes.

## [0.1.0] - 2026-11-07

The first release of Risquette, a desktop client for Discuit.

### Added

- Home and All feeds with Hot, New, Activity and Top sorting.
- Posts with image carousels, and a pop-out player for YouTube, Vimeo and other videos.
- Threaded comments with voting, replies, editing and deleting.
- Communities, profiles and saved lists.
- Posting text, image and link posts, with a markdown toolbar and preview.
- Notifications, with an unread counter on the bell in the toolbar.
- Mod tools and site admin.
- Support for any Discuit instance, with a separate login for each.
- Light and dark themes.
- A setup wizard that installs without administrator rights, and automatic updates.

[Unreleased]: https://github.com/tyler-Github/risquette/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tyler-Github/risquette/releases/tag/v0.1.0
