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

/// 대행 프로세스가 없을 때 다시 띄워 보는 간격. 실패해도 늘리지 않는다: 서명이 나중에 생기면
/// (local-signing.ps1 -Enable) 백엔드를 다시 시작하지 않아도 이 시간 안에 뜨게.
const RETRY: Duration = Duration::from_secs(20);

/// ShellExecute 결과 코드 (32 이하는 실패: 5 = 접근 거부, 서명·UIAccess 조건 미충족 등).
fn launch(path: &PathBuf) -> isize {
    let file: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let r = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(file.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_HIDE) };
    r.0 as isize
}

/// 백그라운드에서 대행 프로세스를 띄우고, 꺼지면 다시 띄운다.
/// 실패는 결과가 바뀔 때만 기록한다 (같은 실패를 RETRY마다 쓰지 않게).
pub fn start() {
    let paths = candidates();
    if paths.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let mut last: Option<isize> = None;
        loop {
            if !running() {
                let mut code = 0;
                for p in &paths {
                    code = launch(p);
                    if code > 32 {
                        break;
                    }
                }
                if last != Some(code) || code > 32 {
                    crate::log(&format!("candhost: launch {} -> {code}", paths[0].display()));
                }
                last = Some(code);
            }
            std::thread::sleep(RETRY);
        }
    });
}
