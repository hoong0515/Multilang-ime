//! 후보창 대행 프로세스(`MultilangIME-CandWin.exe`)를 띄우고 살아 있게 한다.
//!
//! 대행 프로세스는 UIAccess로 실행되어야 셸 검색창 위에 후보창을 띄울 수 있다. Windows는 서명되고
//! 보안 위치(Program Files)에 설치된 실행 파일만 UIAccess로 실행하며, CreateProcess가 아니라
//! ShellExecute로 띄워야 한다. 그래서 Program Files 설치본을 먼저 찾고, 없으면 백엔드 옆 파일을 쓴다
//! (서명이 없거나 보안 위치가 아니면 실행이 거부되어 로그만 남긴다. 그때 TIP는 직접 그린다).

use std::path::PathBuf;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_HIDE};

const EXE: &str = "MultilangIME-CandWin.exe";
const HOST_CLASS: PCWSTR = w!("MultilangImeCandidateHost");

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(pf) = std::env::var("ProgramFiles") {
        out.push(PathBuf::from(pf).join("MultilangIME").join(EXE));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        out.push(dir.join(EXE));
    }
    out.into_iter().filter(|p| p.exists()).collect()
}

fn running() -> bool {
    unsafe { FindWindowW(HOST_CLASS, PCWSTR::null()) }.is_ok_and(|h| !h.0.is_null())
}

fn launch(path: &PathBuf) -> bool {
    let file: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let r = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_HIDE) };
    // 32 이하는 실패 (UIAccess 조건 미충족 등)
    let code = r.0 as isize;
    crate::log(&format!("candhost: launch {} -> {code}", path.display()));
    code > 32
}

/// 백그라운드에서 대행 프로세스를 띄우고, 꺼지면 다시 띄운다 (실패가 이어지면 간격을 늘린다).
pub fn start() {
    let paths = candidates();
    if paths.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let mut wait = Duration::from_secs(30);
        loop {
            if !running() {
                if paths.iter().any(launch) {
                    wait = Duration::from_secs(30);
                } else {
                    wait = (wait * 2).min(Duration::from_secs(1800));
                }
            }
            std::thread::sleep(wait);
        }
    });
}
