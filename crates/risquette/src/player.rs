//! A pop-out video player: a native window hosting a web view (WebView2 on
//! Windows, WebKit elsewhere) with the provider's embedded player.

use crate::{embed::Player, icons::Icon, ui};
use gpui::{div, prelude::*, px, size, App, AppContext, Bounds, Context, Pixels, SharedString, Size, TitlebarOptions, Window, WindowBounds, WindowOptions};
use wry::raw_window_handle::HasWindowHandle as _;
use risquette_theme::ActiveTheme;
use wry::{dpi, Rect, WebContext, WebView, WebViewBuilder};

pub struct PlayerView {
    webview: Option<WebView>,
    /// Kept alive for the web view's lifetime.
    _context: Option<WebContext>,
    error: Option<SharedString>,
    source: String,
    last_size: Option<Size<Pixels>>,
}

/// Open `player` in a new window titled `title`.
///
/// The web view is created *after* the window exists and outside any GPUI
/// update: WebView2's setup pumps the Windows message loop, which runs other
/// GPUI tasks; doing it while the app is borrowed panics ("RefCell already borrowed").
pub fn open(player: Player, title: String, cx: &mut App) {
    // On Windows, GPUI draws with DirectComposition above child windows, which
    // hides a WebView2 child (audio plays, picture is black). Use a plain native
    // window instead.
    #[cfg(target_os = "windows")]
    {
        native::open(player, title, cx);
        return;
    }
    #[allow(unreachable_code)]
    let bounds = Bounds::centered(None, size(px(1024.), px(616.)), cx);
    let source = player.url().to_string();
    let window = match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some(format!("{title} — Risquette").into()), ..Default::default() }),
            window_min_size: Some(size(px(320.), px(200.))),
            app_id: Some("risquette".into()),
            ..Default::default()
        },
        move |_, cx| cx.new(|_| PlayerView { webview: None, _context: None, error: None, source, last_size: None }),
    ) {
        Ok(w) => w,
        Err(e) => {
            log::warn!("player window: {e}");
            cx.open_url(player.url());
            return;
        }
    };
    cx.spawn(async move |cx| {
        // Grab the native handle and size while briefly borrowing the window…
        let Ok(Some((raw, viewport))) = window.update(cx, |_, window, _| {
            let raw = window.window_handle().ok().map(|h| h.as_raw());
            raw.map(|r| (r, window.viewport_size()))
        }) else {
            return;
        };
        // …then build the web view with nothing borrowed.
        let mut context = WebContext::new(dirs::data_local_dir().map(|d| d.join("Risquette").join("webview")));
        let result = build(&player, &mut context, NativeHandle(raw), viewport);
        window
            .update(cx, |view, _, cx| {
                match result {
                    Ok(webview) => {
                        view.webview = Some(webview);
                        view.last_size = Some(viewport);
                    }
                    Err(e) => {
                        log::warn!("web view: {e}");
                        view.error = Some(format!("The in-app player couldn't start ({e}).").into());
                    }
                }
                view._context = Some(context);
                cx.notify();
            })
            .ok();
        // Diagnostics: log what the player page shows once it has had time to load.
        cx.background_executor().timer(std::time::Duration::from_secs(6)).await;
        window
            .update(cx, |view, _, _| {
                if let Some(webview) = &view.webview {
                    let js = "JSON.stringify({url: location.href, frame: (document.querySelector('iframe,video') || {}).src, events: (window.__events || []).filter(function (e, i, a) { return a.indexOf(e) === i; }).slice(-12)})";
                    let _ = webview.evaluate_script_with_callback(js, |state| log::debug!("player page: {state}"));
                }
            })
            .ok();
    })
    .detach();
}

/// A copied native window handle, usable after the GPUI borrow has ended.
struct NativeHandle(wry::raw_window_handle::RawWindowHandle);

impl wry::raw_window_handle::HasWindowHandle for NativeHandle {
    fn window_handle(&self) -> Result<wry::raw_window_handle::WindowHandle<'_>, wry::raw_window_handle::HandleError> {
        // SAFETY: the handle belongs to the player window, which outlives this call.
        Ok(unsafe { wry::raw_window_handle::WindowHandle::borrow_raw(self.0) })
    }
}

/// The local page the web view loads. Providers like YouTube refuse to play
/// (error 153) unless the embed sits in an iframe on a real page, so we serve
/// one from our own origin via a custom protocol.
#[cfg(target_os = "windows")]
const PLAYER_PAGE: &str = "https://risquette.player/";
#[cfg(not(target_os = "windows"))]
const PLAYER_PAGE: &str = "risquette://player/";

fn build(player: &Player, context: &mut WebContext, parent: NativeHandle, viewport: Size<Pixels>) -> wry::Result<WebView> {
    let html = player_page(player);
    let builder = WebViewBuilder::new_with_web_context(context)
        .with_autoplay(true)
        .with_background_color((0, 0, 0, 255))
        .with_bounds(rect(viewport))
        .with_custom_protocol("risquette".into(), move |_, _request| {
            wry::http::Response::builder()
                .header(wry::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(std::borrow::Cow::Owned(html.clone().into_bytes()))
                .unwrap_or_else(|_| wry::http::Response::new(std::borrow::Cow::Borrowed(&b""[..])))
        });
    #[cfg(target_os = "windows")]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_https_scheme(true)
    };
    builder.with_url(PLAYER_PAGE).build_as_child(&parent)
}

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

fn rect(viewport: Size<Pixels>) -> Rect {
    Rect {
        position: dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: dpi::LogicalSize::new(f64::from(f32::from(viewport.width)), f64::from(f32::from(viewport.height))).into(),
    }
}


impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        if let Some(webview) = &self.webview {
            if self.last_size != Some(viewport) {
                self.last_size = Some(viewport);
                if let Err(e) = webview.set_bounds(rect(viewport)) {
                    log::warn!("resize web view: {e}");
                }
            }
        }
        let t = *cx.theme();
        let source = self.source.clone();
        div().size_full().bg(gpui::black()).font_family(t.font_family).when_some(self.error.clone(), |d, err| {
            d.flex().flex_col().items_center().justify_center().gap(px(12.)).text_color(gpui::white()).child(err).child(
                ui::button_main_with_icon("player-browser", Icon::ExternalLink, "Open in browser", cx).on_click(move |_, window, cx| {
                    cx.open_url(&source);
                    window.remove_window();
                }),
            )
        })
    }
}


/// The Windows player: a plain Win32 window (no GPUI rendering) with WebView2
/// filling its client area. GPUI's message loop dispatches its messages.
#[cfg(target_os = "windows")]
mod native {
    use super::{player_page, Player, PLAYER_PAGE};
    use gpui::App;
    use std::{cell::RefCell, collections::HashMap, num::NonZeroIsize};
    use windows::{
        core::{w, HSTRING, PCWSTR},
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{GetStockObject, HBRUSH, BLACK_BRUSH},
            System::LibraryLoader::GetModuleHandleW,
            UI::{HiDpi::GetDpiForSystem, WindowsAndMessaging::*},
        },
    };
    use wry::{dpi, raw_window_handle as rwh, Rect, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

    const CLASS: PCWSTR = w!("RisquettePlayer");

    thread_local! {
        /// Live players by window handle (all on the UI thread).
        static PLAYERS: RefCell<HashMap<isize, (WebView, WebContext)>> = RefCell::new(HashMap::new());
        static REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    struct Handle(HWND);

    impl rwh::HasWindowHandle for Handle {
        fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
            let hwnd = NonZeroIsize::new(self.0 .0 as isize).ok_or(rwh::HandleError::Unavailable)?;
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
                    if let Ok(p) = p.try_borrow() {
                        if let Some((webview, _)) = p.get(&(hwnd.0 as isize)) {
                            let _ = webview.set_bounds(client_rect(hwnd));
                        }
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
}
