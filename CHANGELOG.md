# Changelog

All notable changes to Risquette. Each release's section becomes its release notes on GitHub and the "What's new" text in the app's update dialog, so entries are written for people using the app.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed

- Clicking the "new posts" button at the top of a feed now loads the new posts, instead of opening the post underneath it.

## [0.3.0] - 2026-11-10

### Added

- Risquette now runs on Linux. Download the AppImage, make it executable and open it: it works on most distributions and updates itself like the Windows version. Desktop notifications use your desktop's notification system, and videos open in your web browser.
- Custom feeds: combine several communities into one feed, newest first. Make one from **New custom feed** in the sidebar or a community's menu, and add or remove communities from the feed's page.
- Favourite communities: star a community to keep it at the top of the sidebar, and drag favourites into your own order.
- Keyword filters in Settings → Reading: hide posts whose title or text contains certain words or phrases.
- High contrast for both the light and dark themes, in Settings → App.
- Reading options in Settings → Reading: put a gallery's arrows, dots and counter under the picture instead of on it, colour-code reply lines by depth, and remember the sort for each feed separately. All three are off unless you turn them on.
- Drafts: unsent comments, replies and posts are kept as you type and come back when you return, even after closing Risquette, with a **Discard** link if you don't want them. They're removed once sent.
- While you're reading a feed, Risquette checks for new posts once a minute and shows a "new posts" button at the top. Click it to see them; the feed doesn't move under you otherwise.
- When Discuit can't be reached, Risquette shows a reconnecting screen that counts down to its next attempt, with **Try now** and a way into Settings if you need a different server. Once the connection is back it picks up where you left off: pages that failed to load, missing images and your account load without a restart. Nothing you send is repeated.

### Changed

- Opening a video in your web browser now opens its normal page (on YouTube, Vimeo, Streamable or Dailymotion) instead of a bare player.
- Connection problems now show a plain message such as "Couldn't reach the server" instead of a technical error, and an unreachable server is given up on after 10 seconds instead of a minute.

### Fixed

- Starting Risquette without an internet connection no longer leaves it looking logged out with no communities until the next restart.

## [0.2.0] - 2026-11-08

### Added

- Use more than one account: add another login from the account menu or Settings, and switch between them without logging out. Logging out of one account switches to the next.
- Windows notifications for new replies, mentions and other notifications while Risquette isn't the window you're using. Click one to open it. They're off by default: turn them on in Settings. While they're on, Risquette shows an icon in the system tray.
- If Risquette crashes, it now shows a window explaining what happened, with a button that opens a bug report on GitHub with the details already filled in. You can also restart the app or copy the details from there.
- Risquette keeps a log file and saves crash reports, which help us track down problems.
- A Help & troubleshooting section in Settings, with links to report a problem and to open the logs folder.
- Posts and comments now show tables, task lists with checkboxes, footnotes and smaller headings (levels 4 to 6), as on the Discuit website.

### Changed

- Opening Risquette while it's already running now brings the existing window to the front instead of starting a second copy.
- Risquette now opens maximized.
- The setup wizard now says "Reinstall" rather than "Update" when the same version is already installed.
- Choosing "Also remove my settings and saved logins" when uninstalling now removes logs and crash reports too.
- HTML tags in posts and comments are now hidden instead of shown as text, and images written in markdown show their address as a link, as on the Discuit website.
- The member count in the Discuit side panel now reads as one line, such as "12k people on Discuit".

### Fixed

- Settings can now be opened while logged out, from the gear next to "Log in" in the sidebar.
- The "Log in" and "Create account" buttons in the sidebar, and a few buttons in side panels, no longer shrink to fit their text.
- Opening `uninstall.exe` from the install folder now opens the uninstaller instead of the setup wizard.
- Text boxes in Settings and the "Save to list" dialog no longer shrink to a sliver after the page refreshes.
- Text in Settings, Communities, Lists, Mod tools and Admin no longer runs past the edge of its card when the window is wide.

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

[Unreleased]: https://github.com/tyler-Github/risquette/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/tyler-Github/risquette/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/tyler-Github/risquette/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tyler-Github/risquette/releases/tag/v0.1.0
