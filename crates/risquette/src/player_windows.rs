//! The pop-out video player on Windows: a plain native window hosting
//! WebView2 with the provider's embedded player.
//!
//! GPUI draws with DirectComposition above child windows, which hides a WebView2
//! child (audio plays, picture is black), so the player gets its own Win32 window.

use crate::embed::Player;
use gpui::App;
use std::{cell::RefCell, collections::HashMap, num::NonZeroIsize};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH},
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::GetDpiForSystem, WindowsAndMessaging::*},
    },
    core::{HSTRING, PCWSTR, w},
};
use wry::{Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, dpi, raw_window_handle as rwh};

const CLASS: PCWSTR = w!("RisquettePlayer");

thread_local! {
    /// Live players by window handle (all on the UI thread).
    static PLAYERS: RefCell<HashMap<isize, (WebView, WebContext)>> = RefCell::new(HashMap::new());
    static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct Handle(HWND);

impl rwh::HasWindowHandle for Handle {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let hwnd = NonZeroIsize::new(self.0.0 as isize).ok_or(rwh::HandleError::Unavailable)?;
        let raw = rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(hwnd));
        // SAFETY: the window lives until WM_DESTROY, after which the web view is dropped.
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw) })
    }
}

fn client_rect(hwnd: HWND) -> Rect {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    Rect {
        position: dpi::PhysicalPosition::new(0, 0).into(),
        size: dpi::PhysicalSize::new((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32).into(),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            PLAYERS.with(|p| {
                if let Ok(p) = p.try_borrow()
                    && let Some((webview, _)) = p.get(&(hwnd.0 as isize))
                {
                    let _ = webview.set_bounds(client_rect(hwnd));
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            // Drop the web view (stops playback) once the window goes away.
            let removed = PLAYERS.with(|p| p.try_borrow_mut().ok().and_then(|mut p| p.remove(&(hwnd.0 as isize))));
            drop(removed);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn register() -> windows::core::Result<()> {
    if REGISTERED.with(|r| r.get()) {
        return Ok(());
    }
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_win32());
        }
    }
    REGISTERED.with(|r| r.set(true));
    Ok(())
}

fn create(player: &Player, title: &str) -> Result<isize, String> {
    register().map_err(|e| e.to_string())?;
    // 16:9-ish, at most 70% of the screen, centered.
    let scale = unsafe { GetDpiForSystem() } as i32;
    let (screen_w, screen_h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let w = (960 * scale / 96).min(screen_w * 7 / 10);
    let h = (w * 9 / 16 + 40 * scale / 96).min(screen_h * 7 / 10);
    let (x, y) = ((screen_w - w) / 2, (screen_h - h) / 2);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            &HSTRING::from(format!("{title} — Risquette")),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            w,
            h,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|e| e.to_string())?;
    // Show explicitly as a normal (not maximized) window at the size above.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
    }
    log::debug!("player window {w}x{h} at {x},{y} (screen {screen_w}x{screen_h}, dpi {scale})");

    let html = player_page(player);
    let mut context = WebContext::new(dirs::data_local_dir().map(|d| d.join("Risquette").join("webview")));
    let result = WebViewBuilder::new_with_web_context(&mut context)
        .with_autoplay(true)
        .with_background_color((0, 0, 0, 255))
        .with_bounds(client_rect(hwnd))
        .with_https_scheme(true)
        .with_custom_protocol("risquette".into(), move |_, _| {
            wry::http::Response::builder()
                .header(wry::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(std::borrow::Cow::Owned(html.clone().into_bytes()))
                .unwrap_or_else(|_| wry::http::Response::new(std::borrow::Cow::Borrowed(&b""[..])))
        })
        .with_url(PLAYER_PAGE)
        .build_as_child(&Handle(hwnd));
    match result {
        Ok(webview) => {
            let _ = webview.set_bounds(client_rect(hwnd));
            PLAYERS.with(|p| p.borrow_mut().insert(hwnd.0 as isize, (webview, context)));
            Ok(hwnd.0 as isize)
        }
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            Err(e.to_string())
        }
    }
}

pub fn open(player: Player, title: String, cx: &mut App) {
    // Built from a task so nothing in GPUI is borrowed while WebView2's
    // setup pumps the message loop.
    cx.spawn(async move |cx| {
        match create(&player, &title) {
            Ok(key) => {
                // Diagnostics: log the player's state once it has had time to load.
                cx.background_executor().timer(std::time::Duration::from_secs(6)).await;
                PLAYERS.with(|p| {
                    if let Some((webview, _)) = p.try_borrow().ok().as_ref().and_then(|p| p.get(&key)) {
                        let js = "JSON.stringify({url: location.href, frame: (document.querySelector('iframe,video') || {}).src, events: (window.__events || []).filter(function (e, i, a) { return a.indexOf(e) === i; }).slice(-12)})";
                        let _ = webview.evaluate_script_with_callback(js, |state| log::debug!("player page: {state}"));
                    }
                });
            }
            Err(e) => {
                log::warn!("player: {e}; opening in the browser instead");
                let url = player.url().to_string();
                cx.update(|cx| cx.open_url(&url)).ok();
            }
        }
    })
    .detach();
}

/// The local page the web view loads (served by a custom protocol), which embeds the player.
const PLAYER_PAGE: &str = "https://risquette.player/";

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn player_page(player: &Player) -> String {
    let media = match player {
        Player::Page(url) if url.contains("youtube-nocookie.com/embed/") => {
            // Enable YouTube's postMessage API (state/error events) for our origin.
            let origin = PLAYER_PAGE.trim_end_matches('/');
            let url = format!("{url}&enablejsapi=1&origin={}", url::form_urlencoded::byte_serialize(origin.as_bytes()).collect::<String>());
            format!(
                r#"<iframe id="yt" src="{}" allow="autoplay; encrypted-media; picture-in-picture; fullscreen; clipboard-write" allowfullscreen referrerpolicy="strict-origin-when-cross-origin" frameborder="0"></iframe>
<script>
window.__events = [];
addEventListener('message', function (e) {{ try {{ var d = typeof e.data === 'string' ? JSON.parse(e.data) : e.data; if (d && d.event) if (d.event === 'infoDelivery') {{ var i = d.info || {{}}; if (i.playerState !== undefined) window.__events.push('state:' + i.playerState); if (i.currentTime !== undefined) window.__events.push('t:' + Math.round(i.currentTime)); }} else window.__events.push(d.event + (d.info !== undefined && typeof d.info !== 'object' ? ':' + d.info : '')); }} catch (_) {{}} }});
document.getElementById('yt').addEventListener('load', function () {{ this.contentWindow.postMessage(JSON.stringify({{event: 'listening', id: 1, channel: 'widget'}}), '*'); }});
</script>"#,
                escape_attr(&url)
            )
        }
        Player::Page(url) => format!(
            r#"<iframe src="{}" allow="autoplay; encrypted-media; picture-in-picture; fullscreen; clipboard-write" allowfullscreen referrerpolicy="strict-origin-when-cross-origin" frameborder="0"></iframe>"#,
            escape_attr(url)
        ),
        Player::File(url) => format!(r#"<video src="{}" controls autoplay playsinline></video>"#, escape_attr(url)),
    };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="referrer" content="strict-origin-when-cross-origin"><title>Risquette player</title><style>html,body{{margin:0;height:100%;background:#000;overflow:hidden}}iframe,video{{border:0;width:100%;height:100%;display:block;object-fit:contain}}</style></head><body>{media}</body></html>"#
    )
}
