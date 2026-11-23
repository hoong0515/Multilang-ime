//! Multilang IME candidate host (`MultilangIME-CandWin.exe`).
//!
//! The taskbar search box (SearchHost) draws above normal windows, so a candidate window created by the
//! TIP inside it stays hidden. This process runs with UIAccess and draws the same candidate window
//! (tip/src/candwin.rs) for the TIP, which sends WM_COPYDATA with a JSON command:
//! `{"op":"show","list":{..},"header":null,"anchor":[l,t,r,b]}` or `{"op":"hide"}`.
//! Started by the backend (ShellExecute). One per user session.

#![windows_subsystem = "windows"]

#[path = "../../tip/src/candwin.rs"]
mod candwin;

use serde::Deserialize;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Window class the TIP looks for (keep in sync with tip/src/host.rs).
const HOST_CLASS: PCWSTR = w!("MultilangImeCandidateHost");
/// WM_COPYDATA tag ("MLCW").
const COPYDATA_TAG: usize = 0x4D4C_4357;

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

fn handle(cmd: Command) {
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
        if cds.dwData == COPYDATA_TAG && !cds.lpData.is_null() && cds.cbData < (1 << 20) {
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
