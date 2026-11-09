//! Video playback: a pop-out player window on Windows, the web browser elsewhere.

#[cfg(windows)]
#[path = "player_windows.rs"]
mod windows_player;

use crate::embed::Player;
use gpui::App;

/// Play `player` (window titled `title` on Windows; opened in the browser on other platforms).
pub fn open(player: Player, title: String, cx: &mut App) {
    #[cfg(windows)]
    windows_player::open(player, title, cx);
    #[cfg(not(windows))]
    {
        let _ = title;
        cx.open_url(&player.browser_url());
    }
}
