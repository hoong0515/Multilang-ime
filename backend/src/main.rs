//! 백엔드 프로세스: 사용자당 하나. TIP(앱 안 입력 모듈)들이 이름 있는 파이프로 붙는다.
//!
//! 연결마다 스레드 하나와 조합 상태(Session) 하나. 모델은 한 번 읽어 모든 연결이 같이 쓴다.
//! 모델 파일은 실행 파일 옆의 engine_model.json (없으면 MULTILANG_IME_MODEL 환경 변수 경로).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod candhost;
mod hanja;
mod lock;
mod pipe_security;
mod session;
mod settings;
mod userdict;

use std::ffi::c_void;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use engine::detector::{History, SharedHistory};
use engine::model::Models;
use ipc::{Request, Response};
use session::Session;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::CreateMutexW;

pub(crate) fn log(msg: &str) {
    if cfg!(test) {
        return;
    }
    let dir = std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let dir = dir.join("multilang-ime");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("backend.log")) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{t}] {msg}");
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn model_path() -> PathBuf {
    if let Ok(p) = std::env::var("MULTILANG_IME_MODEL") {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("engine_model.json")))
        .unwrap_or_else(|| PathBuf::from("engine_model.json"))
}

/// HANDLE을 스레드로 넘기기 위한 포장.
struct PipeHandle(isize);
unsafe impl Send for PipeHandle {}

fn read_message(h: HANDLE) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let mut n = 0u32;
        let res = unsafe { ReadFile(h, Some(&mut buf), Some(&mut n), None) };
        out.extend_from_slice(&buf[..n as usize]);
        match res {
            Ok(()) => return Some(out),
            Err(_) if unsafe { GetLastError() } == ERROR_MORE_DATA => continue,
            Err(_) => return None,
        }
    }
}

fn data_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir()).join("multilang-ime")
}

fn debug_delay() -> Option<std::time::Duration> {
    let p = std::env::current_exe().ok()?.parent()?.join("BACKEND_DELAY_MS");
    let ms: u64 = std::fs::read_to_string(p).ok()?.trim().parse().ok()?;
    Some(std::time::Duration::from_millis(ms))
}

/// Mozc: 직접 빌드한 표준 입출력 도우미(mozc_stdio.exe)가 있으면 그것을, 없으면 공식 mozc_server.exe.
fn mozc_server_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("mozc");
    ["mozc_stdio.exe", "mozc_server.exe"].iter().map(|n| dir.join(n)).find(|p| p.exists())
}

/// 한/영 키 (한국어 키보드의 오른쪽 Alt 자리)
const VK_HANGUL: u16 = 0x15;

fn handle_client(h: PipeHandle, models: &'static Models, history: SharedHistory) {
    let h = HANDLE(h.0 as *mut c_void);
    let mut session = Session::new(models, history, Some(data_dir().join("history.json")), mozc_server_path());
    while let Some(msg) = read_message(h) {
        let t = std::time::Instant::now();
        let req = serde_json::from_slice::<Request>(&msg);
        let op = req.as_ref().map(|r| format!("{r:?}")).unwrap_or_default();
        let settings = settings::current();
        session.apply_settings(&settings);
        // English 고정: 조합이 없으면 키를 받지 않는다 (한/영 키만 받는다)
        let en_locked = lock::get() == Some(engine::detector::Lang::En) && !session.is_composing();
        let off = session.app_off(&settings);
        let mut resp = match req {
            // 한/영 키(VK_HANGUL): 언어 고정 순환
            Ok(Request::Test { key }) if key.vk == VK_HANGUL && !off => Response { eaten: true, ..Default::default() },
            Ok(Request::Key { key }) if key.vk == VK_HANGUL && !off => session.cycle_lock(&settings),
            Ok(Request::Test { .. } | Request::Key { .. }) if off || en_locked => Response::default(),
            Ok(Request::Test { key }) => session.test(&key),
            Ok(Request::Key { key }) => session.key(&key),
            Ok(Request::Hello { app }) => {
                session.app = app;
                Response::default()
            }
            Ok(Request::ClearLearning) => session.clear_learning(),
            Ok(Request::Status) => Response::default(),
            Ok(Request::Flush) => session.flush(),
            Ok(Request::Reset) => session.reset(),
            Ok(Request::ShiftTap) => session.shift_tap(),
            Err(e) => {
                log(&format!("bad request: {e}"));
                Response::default()
            }
        };
        // 시험용: exe 옆에 BACKEND_DELAY_MS 파일(숫자)이 있으면 글자 키 응답을 그만큼 늦춘다
        if op.starts_with("Key") {
            if let Some(d) = debug_delay() {
                std::thread::sleep(d);
            }
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        if ms > 50.0 {
            log(&format!("slow request {ms:.0} ms: {op}"));
        }
        resp.indicator = Some(session.indicator(&resp));
        let body = serde_json::to_vec(&resp).unwrap_or_default();
        let mut written = 0u32;
        if unsafe { WriteFile(h, Some(&body), Some(&mut written), None) }.is_err() {
            break;
        }
    }
    unsafe {
        let _ = FlushFileBuffers(h);
        let _ = DisconnectNamedPipe(h);
        let _ = CloseHandle(h);
    }
}

fn main() {
    // 사용자당 하나만
    let mutex_name = wide("Local\\multilang-ime.backend");
    let _mutex = unsafe { CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }

    let path = model_path();
    let models: &'static Models = match Models::load(&path) {
        Ok(m) => Box::leak(Box::new(m)),
        Err(e) => {
            log(&format!("model load failed ({}): {e}", path.display()));
            return;
        }
    };
    let _ = std::fs::create_dir_all(data_dir());
    let history: SharedHistory = Arc::new(Mutex::new(History::load(&data_dir().join("history.json"))));
    if let Ok(h) = history.lock() {
        log(&format!("history entries: {}", h.len()));
    }
    settings::init(data_dir().join("settings.json"));
    lock::init(data_dir().join("lock.txt"));
    userdict::init(data_dir().join("user_dict.json"));
    // 한자 사전은 시작할 때 백그라운드로 읽는다 (약 30만 항목)
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let learn = data_dir().join("hanja_learn.json");
        std::thread::spawn(move || hanja::load(&dir.join("hanja.txt"), learn));
    }
    // Mozc 서버는 시작할 때 미리 띄운다 (첫 일본어 키에서 띄우면 TIP가 시간 초과로 키를 놓친다)
    if let Some(server) = mozc_server_path() {
        session::start_mozc(&server);
    }
    // 셸 검색창용 후보창 대행 프로세스 (UIAccess, Program Files 설치본)
    candhost::start();
    let user = std::env::var("USERNAME").unwrap_or_default();
    let name = wide(&ipc::pipe_name(&user));
    log(&format!("started, pipe {}", ipc::pipe_name(&user)));

    // 샌드박스 앱(작업 표시줄 검색창 등) 안의 TIP도 접속할 수 있는 보안 설정
    let sa = pipe_security::pipe_security();
    log(if sa.is_some() { "pipe security: user + AppContainer, low integrity" } else { "pipe security: default (AppContainer apps cannot connect)" });
    loop {
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAGS_AND_ATTRIBUTES(0),
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                sa.as_ref().map(|a| a as *const _),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            log("CreateNamedPipeW failed");
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        let connected =
            unsafe { ConnectNamedPipe(h, None) }.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe {
                let _ = CloseHandle(h);
            }
            continue;
        }
        let ph = PipeHandle(h.0 as isize);
        let hist = history.clone();
        std::thread::spawn(move || handle_client(ph, models, hist));
    }
}
