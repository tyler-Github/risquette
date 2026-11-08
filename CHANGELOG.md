# Changelog

All notable changes to Risquette. Each release's section becomes its release notes on GitHub and the "What's new" text in the app's update dialog, so entries are written for people using the app.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Risquette now runs on Linux. Download the AppImage, make it executable and open it: it works on most distributions and updates itself like the Windows version. Desktop notifications use your desktop's notification system, and videos open in your web browser.

### Changed

- Opening a video in your web browser now opens its normal page (on YouTube, Vimeo, Streamable or Dailymotion) instead of a bare player.

## [0.2.0-beta.1] - 2026-11-07

### Added

- Use more than one account: add another login from the account menu or Settings, and switch between them without logging out. Logging out of one account switches to the next.
- Windows notifications for new replies, mentions and other notifications while Risquette isn't the window you're using. Click one to open it. They're off by default: turn them on in Settings. While they're on, Risquette shows an icon in the system tray.
- If Risquette crashes, it now shows a window explaining what happened, with a button that opens a bug report on GitHub with the details already filled in. You can also restart the app or copy the details from there.
- Risquette keeps a log file and saves crash reports, which help us track down problems.
- A Help & troubleshooting section in Settings, with links to report a problem and to open the logs folder.

### Changed

- Opening Risquette while it's already running now brings the existing window to the front instead of starting a second copy.
- Risquette now opens maximized.
- The setup wizard now says "Reinstall" rather than "Update" when the same version is already installed.
- Choosing "Also remove my settings and saved logins" when uninstalling now removes logs and crash reports too.

### Fixed

- Settings can now be opened while logged out, from the gear next to "Log in" in the sidebar.
- The "Log in" and "Create account" buttons in the sidebar, and a few buttons in side panels, no longer shrink to fit their text.
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

[Unreleased]: https://github.com/tyler-Github/risquette/compare/v0.2.0-beta.1...HEAD
[0.2.0-beta.1]: https://github.com/tyler-Github/risquette/compare/v0.1.0...v0.2.0-beta.1
[0.1.0]: https://github.com/tyler-Github/risquette/releases/tag/v0.1.0
