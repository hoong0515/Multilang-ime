//! 후보창 대행 프로세스(`MultilangIME-CandWin.exe`, `candhost/`)에 후보창을 맡긴다.
//!
//! 작업 표시줄 검색창(SearchHost) 같은 셸 화면은 일반 창보다 높은 창 층(band)에 그려져서, 이 프로세스
//! 안에서 만든 후보창은 그 뒤에 가려진다. UIAccess로 실행된 대행 프로세스의 창은 그보다 위에 뜬다.
//! 셸 프로세스에서 대행 창이 떠 있을 때만 쓰고, 나머지는 지금처럼 직접 그린다 (candwin).

use std::sync::OnceLock;

use ipc::CandidateList;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_COPYDATA};

const HOST_CLASS: PCWSTR = w!("MultilangImeCandidateHost");
const COPYDATA_TAG: usize = 0x4D4C_4357;

/// 후보창이 가려지는 셸 프로세스 (실행 파일 이름, 소문자).
const SHELL_HOSTS: [&str; 4] = ["searchhost.exe", "searchapp.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe"];

fn in_shell_host() -> bool {
    static SHELL: OnceLock<bool> = OnceLock::new();
    *SHELL.get_or_init(|| SHELL_HOSTS.contains(&crate::app_name().to_lowercase().as_str()))
}

fn send(json: &str) -> bool {
    if !in_shell_host() {
        return false;
    }
    let Ok(hwnd) = (unsafe { FindWindowW(HOST_CLASS, PCWSTR::null()) }) else { return false };
    if hwnd.0.is_null() {
        return false;
    }
    let cds = COPYDATASTRUCT { dwData: COPYDATA_TAG, cbData: json.len() as u32, lpData: json.as_ptr() as *mut _ };
    let mut result = 0usize;
    let r = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const _ as isize),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            200,
            Some(&mut result),
        )
    };
    let ok = r.0 != 0 && result == 1;
    if !ok {
        crate::debug_note(&format!("host send failed r={} result={result}", r.0));
    }
    ok
}

/// 대행 프로세스에 후보창을 띄운다. 맡겼으면 true (그러면 이 프로세스의 창은 쓰지 않는다).
pub fn show(list: Option<&CandidateList>, header: Option<&str>, anchor: RECT) -> bool {
    let cmd = serde_json::json!({
        "op": "show",
        "list": list,
        "header": header,
        "anchor": [anchor.left, anchor.top, anchor.right, anchor.bottom],
    });
    send(&cmd.to_string())
}

pub fn hide() {
    let _ = send(r#"{"op":"hide"}"#);
}
