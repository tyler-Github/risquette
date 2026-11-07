//! The actual install / update / uninstall work. Per-user, no admin rights:
//! files go to `%LOCALAPPDATA%\Programs\Risquette`, the uninstall entry to
//! `HKCU\...\Uninstall\Risquette`, shortcuts to the user's Start menu and desktop.

use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const APP_EXE: &str = "risquette.exe";
pub const UNINSTALL_EXE: &str = "uninstall.exe";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(windows)]
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Risquette";

/// The app executable built into this installer (empty in development builds).
static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));

#[derive(Debug, Clone)]
pub struct Options {
    pub dir: PathBuf,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
    /// Write the uninstall entry and shortcuts. Off only for tests / portable installs.
    pub register: bool,
}

impl Options {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, desktop_shortcut: true, start_menu_shortcut: true, register: true }
    }
}

/// Progress reported while working: fraction done and what's happening.
pub type Progress<'a> = &'a dyn Fn(f32, &str);

pub fn default_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Programs").join("Risquette")
}

/// Where Risquette is installed now, per the uninstall entry.
pub fn installed_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL_KEY).ok()?;
        let dir: String = key.get_value("InstallLocation").ok()?;
        let dir = PathBuf::from(dir);
        return dir.join(APP_EXE).exists().then_some(dir);
    }
    #[allow(unreachable_code)]
    None
}

/// The installed version, per the uninstall entry.
pub fn installed_version() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL_KEY).ok()?;
        return key.get_value("DisplayVersion").ok();
    }
    #[allow(unreachable_code)]
    None
}

/// The app bytes: embedded payload, or (development) `risquette.exe` beside the installer.
fn payload() -> Result<std::borrow::Cow<'static, [u8]>> {
    if !PAYLOAD.is_empty() {
        return Ok(PAYLOAD.into());
    }
    let beside = std::env::current_exe()?.with_file_name(APP_EXE);
    if beside.exists() {
        return Ok(std::fs::read(&beside).with_context(|| format!("reading {}", beside.display()))?.into());
    }
    bail!("This installer doesn't contain Risquette. Build it with scripts/package.ps1.")
}

pub fn payload_size() -> u64 {
    if !PAYLOAD.is_empty() {
        return PAYLOAD.len() as u64;
    }
    std::env::current_exe().ok().and_then(|p| std::fs::metadata(p.with_file_name(APP_EXE)).ok()).map(|m| m.len()).unwrap_or(0)
}

/// Retry `f` for a while: the old app may take a moment to release its files.
fn retry<T>(what: &str, mut f: impl FnMut() -> std::io::Result<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if start.elapsed() < Duration::from_secs(10) => {
                log::debug!("{what}: {e}, retrying");
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(32) {
                    bail!("Risquette seems to be running. Close it and try again.");
                }
                return Err(e).context(what.to_string());
            }
        }
    }
}

/// Install (or reinstall / update) into `opts.dir`.
pub fn install(opts: &Options, progress: Progress) -> Result<()> {
    progress(0.05, "Preparing…");
    let bytes = payload()?;
    std::fs::create_dir_all(&opts.dir).with_context(|| format!("creating {}", opts.dir.display()))?;

    progress(0.15, "Copying Risquette…");
    let target = opts.dir.join(APP_EXE);
    let staged = opts.dir.join("risquette.exe.new");
    std::fs::write(&staged, &bytes).with_context(|| format!("writing {}", staged.display()))?;
    progress(0.55, "Replacing the old version…");
    retry("replacing risquette.exe", || std::fs::rename(&staged, &target))?;

    progress(0.7, "Adding the uninstaller…");
    let me = std::env::current_exe()?;
    let uninstaller = opts.dir.join(UNINSTALL_EXE);
    if !same_file(&me, &uninstaller) {
        retry("copying the uninstaller", || std::fs::copy(&me, &uninstaller))?;
    }

    if opts.register {
        progress(0.8, "Creating shortcuts…");
        shortcuts(opts)?;
        progress(0.9, "Registering with Windows…");
        register(opts, bytes.len() as u64)?;
    }
    progress(1.0, "Done");
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn start_menu_link() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(r"Microsoft\Windows\Start Menu\Programs\Risquette.lnk"))
}

fn desktop_link() -> Option<PathBuf> {
    Some(dirs::desktop_dir()?.join("Risquette.lnk"))
}

fn shortcuts(opts: &Options) -> Result<()> {
    let target = opts.dir.join(APP_EXE);
    let wanted = [(opts.start_menu_shortcut, start_menu_link()), (opts.desktop_shortcut, desktop_link())];
    for (want, link) in wanted {
        let Some(link) = link else { continue };
        if want {
            create_shortcut(&target, &opts.dir, &link).with_context(|| format!("creating {}", link.display()))?;
        } else {
            let _ = std::fs::remove_file(&link);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn create_shortcut(target: &Path, workdir: &Path, link: &Path) -> Result<()> {
    use windows::{
        Win32::{
            System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile},
            UI::Shell::{IShellLinkW, ShellLink},
        },
        core::{HSTRING, Interface},
    };
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            shell.SetPath(&HSTRING::from(target.as_os_str()))?;
            shell.SetWorkingDirectory(&HSTRING::from(workdir.as_os_str()))?;
            shell.SetDescription(&HSTRING::from("A desktop client for Discuit"))?;
            shell.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
            shell.cast::<IPersistFile>()?.Save(&HSTRING::from(link.as_os_str()), true)?;
            Ok(())
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_shortcut(_target: &Path, _workdir: &Path, _link: &Path) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn register(opts: &Options, app_size: u64) -> Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNINSTALL_KEY)?;
    let dir = opts.dir.to_string_lossy().to_string();
    let uninstaller = opts.dir.join(UNINSTALL_EXE).to_string_lossy().to_string();
    key.set_value("DisplayName", &"Risquette")?;
    key.set_value("DisplayVersion", &VERSION)?;
    key.set_value("Publisher", &"Risquette contributors")?;
    key.set_value("DisplayIcon", &opts.dir.join(APP_EXE).to_string_lossy().to_string())?;
    key.set_value("InstallLocation", &dir)?;
    key.set_value("UninstallString", &format!("\"{uninstaller}\" --uninstall"))?;
    key.set_value("QuietUninstallString", &format!("\"{uninstaller}\" --uninstall --silent"))?;
    key.set_value("URLInfoAbout", &"https://github.com/tyler-Github/risquette")?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    key.set_value("EstimatedSize", &((app_size / 1024) as u32 * 2))?;
    Ok(())
}

#[cfg(not(windows))]
fn register(_opts: &Options, _app_size: u64) -> Result<()> {
    Ok(())
}

/// Remove Risquette: files, shortcuts, uninstall entry, and optionally the user's data.
pub fn uninstall(dir: &Path, remove_data: bool, progress: Progress) -> Result<()> {
    progress(0.1, "Removing Risquette…");
    let app = dir.join(APP_EXE);
    if app.exists() {
        retry("removing risquette.exe", || std::fs::remove_file(&app))?;
    }
    progress(0.4, "Removing shortcuts…");
    for link in [start_menu_link(), desktop_link()].into_iter().flatten() {
        let _ = std::fs::remove_file(link);
    }
    progress(0.6, "Unregistering…");
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
    }
    if remove_data {
        progress(0.8, "Removing your settings and saved logins…");
        if let Some(cfg) = dirs::config_dir() {
            let _ = std::fs::remove_dir_all(cfg.join("Risquette"));
        }
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("Risquette"));
    }
    progress(0.95, "Cleaning up…");
    remove_dir_later(dir);
    progress(1.0, "Done");
    Ok(())
}

/// The uninstaller lives in the folder it removes, so the folder is deleted by
/// a short-lived hidden helper after this process exits.
fn remove_dir_later(dir: &Path) {
    let me = std::env::current_exe().ok();
    let inside = me.as_deref().is_some_and(|m| m.parent().is_some_and(|p| same_file(p, dir)));
    if !inside {
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = format!("ping 127.0.0.1 -n 3 > NUL & rmdir /s /q \"{}\"", dir.display());
        let _ = std::process::Command::new("cmd.exe").args(["/C", &script]).creation_flags(CREATE_NO_WINDOW).spawn();
    }
}

/// Wait (up to a minute) for the app that launched an update to exit.
pub fn wait_for_pid(pid: u32) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(handle, 60_000);
            let _ = CloseHandle(handle);
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}

pub fn launch(dir: &Path) -> Result<()> {
    std::process::Command::new(dir.join(APP_EXE)).current_dir(dir).spawn().context("starting Risquette")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_into_a_folder_without_registering() {
        let dir = std::env::temp_dir().join(format!("risquette-installer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Development builds have no payload; fake one beside the test binary.
        let beside = std::env::current_exe().unwrap().with_file_name(APP_EXE);
        let created = PAYLOAD.is_empty() && !beside.exists();
        if created {
            std::fs::write(&beside, b"fake app").unwrap();
        }
        let opts = Options { register: false, ..Options::new(dir.clone()) };
        let steps = std::cell::RefCell::new(Vec::new());
        install(&opts, &|f, m| steps.borrow_mut().push((f, m.to_string()))).unwrap();
        assert!(dir.join(APP_EXE).exists());
        assert!(dir.join(UNINSTALL_EXE).exists());
        assert!(!dir.join("risquette.exe.new").exists());
        assert_eq!(steps.borrow().last().unwrap().0, 1.0);
        // Reinstalling over an existing copy works too.
        install(&opts, &|_, _| {}).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        if created {
            std::fs::remove_file(beside).unwrap();
        }
    }
}
