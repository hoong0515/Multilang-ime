//! Multilang IME candidate host (`MultilangIME-CandWin.exe`).
//!
//! The taskbar search box (SearchHost) draws above normal windows, so a candidate window created by the
//! TIP inside it stays hidden. This process runs with UIAccess and draws the same candidate window
//! (tip/src/candwin.rs) for the TIP, which sends WM_COPYDATA with a JSON command:
//! `{"op":"show","list":{..},"header":null,"anchor":[l,t,r,b]}` or `{"op":"hide"}`.
//! Started by the backend (ShellExecute). One per user session.
//!
//! This process has UIAccess, so any process on the desktop that can send it a message could draw over
//! other windows with it (security review R4). Window messages carry no trustworthy sender, so `show` is
//! only honoured while the foreground window belongs to one of the shell processes the TIP uses this host
//! for (tip/src/host.rs SHELL_HOSTS), and message size, candidate count and string lengths are capped.

#![windows_subsystem = "windows"]

#[path = "../../tip/src/candwin.rs"]
mod candwin;

use serde::Deserialize;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Window class the TIP looks for (keep in sync with tip/src/host.rs).
const HOST_CLASS: PCWSTR = w!("MultilangImeCandidateHost");
/// WM_COPYDATA tag ("MLCW").
const COPYDATA_TAG: usize = 0x4D4C_4357;
/// Largest accepted WM_COPYDATA body (a page of candidates is a few KiB).
const MAX_MESSAGE: u32 = 64 * 1024;
/// Candidate rows per page (the backend sends one page of 9).
const MAX_ITEMS: usize = 32;
/// Longest candidate, label or composition text (UTF-16 units are not counted; chars are).
const MAX_TEXT: usize = 1024;
/// Shell processes whose foreground windows the TIP draws for (keep in sync with tip/src/host.rs, lower case).
const SHELL_HOSTS: [&str; 4] = ["searchhost.exe", "searchapp.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe"];

pub(crate) fn debug_note(_what: &str) {}

pub(crate) fn module_handle() -> HMODULE {
    unsafe { GetModuleHandleW(None) }.unwrap_or_default()
}

#[derive(Deserialize)]
struct Command {
    op: String,
    #[serde(default)]
    list: Option<ipc::CandidateList>,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    anchor: [i32; 4],
}

/// Does a command stay within the caps?
fn within_limits(cmd: &Command) -> bool {
    let short = |s: &String| s.chars().count() <= MAX_TEXT;
    cmd.op.len() <= 16
        && cmd.header.as_ref().is_none_or(short)
        && cmd.list.as_ref().is_none_or(|l| {
            l.items.len() <= MAX_ITEMS && l.labels.len() <= MAX_ITEMS && l.items.iter().chain(&l.labels).all(short)
        })
}

/// Lower-case file name of an executable path.
fn exe_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
}

/// Is `path` one of the shell hosts in a folder only administrators can write?
/// `windir` is the system Windows folder (e.g. `C:\Windows`). The shell apps live in `<windir>\SystemApps\..`;
/// `<drive>\Program Files\WindowsApps\..` is allowed too in case a later Windows moves them into a package.
/// Comparing the name alone would let any program called SearchHost.exe pass (security review 2026-11-24 R3).
fn trusted_shell_path(path: &str, windir: &str) -> bool {
    let path = path.to_lowercase().replace('/', "\\");
    let windir = windir.to_lowercase();
    let windir = windir.trim_end_matches('\\');
    // a drive-letter Windows folder only ("c:\windows"); no "..", no UNC or device paths
    let b = windir.as_bytes();
    if b.len() < 4 || b[1] != b':' || b[2] != b'\\' || path.contains("\\..\\") || path.starts_with("\\\\") {
        return false;
    }
    let drive = &windir[..2];
    let roots = [format!("{windir}\\systemapps\\"), format!("{drive}\\program files\\windowsapps\\")];
    roots.iter().any(|r| path.starts_with(r.as_str())) && SHELL_HOSTS.contains(&exe_name(&path).as_str())
}

fn system_windows_dir() -> Option<String> {
    let mut buf = [0u16; 260];
    let n = unsafe { windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW(Some(&mut buf)) } as usize;
    (n > 0 && n < buf.len()).then(|| String::from_utf16_lossy(&buf[..n]))
}

/// Is the foreground window one of the shell hosts (by full path)?
fn shell_in_foreground() -> bool {
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    if pid == 0 {
        return false;
    }
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    unsafe {
        let _ = CloseHandle(process);
    }
    ok && system_windows_dir().is_some_and(|w| trusted_shell_path(&String::from_utf16_lossy(&buf[..len as usize]), &w))
}

fn handle(cmd: Command) {
    // hide is always allowed; show only for the shell in the foreground and within the caps
    if cmd.op == "show" && !(within_limits(&cmd) && shell_in_foreground()) {
        candwin::hide();
        return;
    }
    let anchor = RECT { left: cmd.anchor[0], top: cmd.anchor[1], right: cmd.anchor[2], bottom: cmd.anchor[3] };
    match (cmd.op.as_str(), cmd.header) {
        ("show", Some(h)) => candwin::show_floating(&h, cmd.list.as_ref(), anchor),
        ("show", None) => match cmd.list {
            Some(l) => candwin::show(&l, anchor),
            None => candwin::hide(),
        },
        _ => candwin::hide(),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA && lparam.0 != 0 {
        let cds = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if cds.dwData == COPYDATA_TAG && !cds.lpData.is_null() && cds.cbData <= MAX_MESSAGE {
            let bytes = unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) };
            if let Ok(cmd) = serde_json::from_slice::<Command>(bytes) {
                handle(cmd);
            }
            return LRESULT(1);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn main() {
    // one per session
    if let Ok(h) = unsafe { FindWindowW(HOST_CLASS, PCWSTR::null()) } {
        if !h.0.is_null() {
            return;
        }
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let module = module_handle();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: module.into(),
        lpszClassName: HOST_CLASS,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };
    // hidden top-level window (FindWindow does not see message-only windows)
    let Ok(hwnd) = (unsafe {
        CreateWindowExW(WS_EX_TOOLWINDOW, HOST_CLASS, w!("Multilang IME candidates"), WS_POPUP, 0, 0, 0, 0, None, None, Some(module.into()), None)
    }) else {
        return;
    };
    // the TIP runs in sandboxed / lower-integrity processes (SearchHost): let WM_COPYDATA through UIPI
    unsafe {
        let _ = ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, None);
    }
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Command {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn limits() {
        assert!(within_limits(&parse(r#"{"op":"hide"}"#)));
        let page = serde_json::json!({"op":"show","list":{"items":vec!["候補"; 9],"labels":vec!["ja"; 9],"selected":0},"anchor":[0,0,1,1]});
        assert!(within_limits(&parse(&page.to_string())));
        let many = serde_json::json!({"op":"show","list":{"items":vec!["a"; MAX_ITEMS + 1],"labels":[],"selected":0}});
        assert!(!within_limits(&parse(&many.to_string())));
        let long = serde_json::json!({"op":"show","list":{"items":["a".repeat(MAX_TEXT + 1)],"labels":[],"selected":0}});
        assert!(!within_limits(&parse(&long.to_string())));
        let header = serde_json::json!({"op":"show","header":"가".repeat(MAX_TEXT + 1)});
        assert!(!within_limits(&parse(&header.to_string())));
        let header_ok = serde_json::json!({"op":"show","header":"가".repeat(MAX_TEXT)});
        assert!(within_limits(&parse(&header_ok.to_string())));
    }

    #[test]
    fn exe_names() {
        assert_eq!(exe_name(r"C:\Windows\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe"), "searchhost.exe");
        assert_eq!(exe_name("notepad.exe"), "notepad.exe");
        assert!(!SHELL_HOSTS.contains(&exe_name(r"C:\evil\searchhost.exe.bat").as_str()));
    }

    #[test]
    fn shell_path_must_be_in_protected_folder() {
        let w = r"C:\Windows";
        assert!(trusted_shell_path(r"C:\WINDOWS\SystemApps\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\SearchHost.exe", w));
        assert!(trusted_shell_path(r"C:\Windows\SystemApps\ShellExperienceHost_cw5n1h2txyewy\ShellExperienceHost.exe", w));
        assert!(trusted_shell_path(r"C:\Program Files\WindowsApps\MicrosoftWindows.Client.CBS_1000.0_x64__cw5n1h2txyewy\SearchHost.exe", w));
        // the same name elsewhere, including user-writable folders under Windows
        assert!(!trusted_shell_path(r"C:\Users\me\Downloads\SearchHost.exe", w));
        assert!(!trusted_shell_path(r"C:\Windows\Temp\SearchHost.exe", w));
        assert!(!trusted_shell_path(r"C:\Windows\SystemApps\..\Temp\SearchHost.exe", w));
        assert!(!trusted_shell_path(r"\\server\share\Windows\SystemApps\x\SearchHost.exe", r"\\server\share\Windows"));
        assert!(!trusted_shell_path(r"D:\Program Files\WindowsApps\x\SearchHost.exe", w));
        // a protected folder but not a shell host
        assert!(!trusted_shell_path(r"C:\Windows\SystemApps\x\notepad.exe", w));
    }

    #[test]
    fn this_test_process_is_not_shell() {
        // the test runner is never a shell host, so show would be refused even if it were in the foreground
        assert!(!SHELL_HOSTS.contains(&exe_name(&std::env::current_exe().unwrap().to_string_lossy()).as_str()));
    }
}
