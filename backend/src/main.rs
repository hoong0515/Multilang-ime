//! 백엔드 프로세스: 사용자당 하나. TIP(앱 안 입력 모듈)들이 이름 있는 파이프로 붙는다.
//!
//! 연결마다 스레드 하나와 조합 상태(Session) 하나. 모델은 한 번 읽어 모든 연결이 같이 쓴다.
//! 모델 파일은 실행 파일 옆의 engine_model.json (없으면 MULTILANG_IME_MODEL 환경 변수 경로).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod candhost;
mod hanja;
mod lock;
mod pipe;
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
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, HANDLE};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, WriteFile};
use windows::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe};
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

/// 같은 내용의 로그가 쏟아지지 않게: 마지막으로 남긴 뒤 1분이 지났을 때만 남긴다.
fn log_throttled(last: &Mutex<Option<std::time::Instant>>, msg: &str) {
    let Ok(mut last) = last.lock() else { return };
    if last.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(60)) {
        *last = Some(std::time::Instant::now());
        log(msg);
    }
}

static BAD_REQUEST_LOG: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static REJECT_LOG: Mutex<Option<std::time::Instant>> = Mutex::new(None);
static CONNECTIONS: pipe::Slots = pipe::Slots::new(pipe::MAX_CONNECTIONS);
static PER_PROCESS: std::sync::OnceLock<pipe::PerProcess> = std::sync::OnceLock::new();

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

/// `identity`: 건너편 프로세스의 (샌드박스 여부, 무결성 RID). `may_manage`: 그 기준으로 믿을 수 있는 연결인가.
fn handle_client(h: PipeHandle, models: &'static Models, history: SharedHistory, identity: Option<(bool, u32)>, may_manage: bool) {
    let h = HANDLE(h.0 as *mut c_void);
    let mut session = Session::new(models, history, Some(data_dir().join("history.json")), mozc_server_path());
    // 관리 명령(학습 지우기)은 샌드박스가 아닌 보통 무결성 이상 프로세스만 (확인 못 하면 거부)
    // 같은 기준으로 학습도: 샌드박스·낮은 무결성 연결의 확정은 공용 학습(판정·한자·Mozc)에 남기지 않는다
    session.set_learning(may_manage);
    loop {
        let msg = match pipe::read_message(h, pipe::MAX_REQUEST) {
            pipe::Read::Message(m) => m,
            pipe::Read::TooLarge => {
                log_throttled(&BAD_REQUEST_LOG, &format!("request over {} bytes: connection closed", pipe::MAX_REQUEST));
                break;
            }
            pipe::Read::Closed => break,
        };
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
                if pipe::valid_app_name(&app) {
                    session.app = app;
                } else {
                    log_throttled(&BAD_REQUEST_LOG, &format!("bad app name ({} chars) ignored", app.chars().count()));
                }
                Response::default()
            }
            Ok(Request::ClearLearning) if may_manage => session.clear_learning(),
            Ok(Request::ClearLearning) => {
                log(&format!("clear_learning denied (client {identity:?}, app {:?})", session.app));
                Response { error: Some("denied".into()), ..Default::default() }
            }
            Ok(Request::Status) => Response::default(),
            Ok(Request::Flush) => session.flush(),
            Ok(Request::Reset) => session.reset(),
            Ok(Request::ShiftTap) => session.shift_tap(),
            Err(e) => {
                log_throttled(&BAD_REQUEST_LOG, &format!("bad request: {e}"));
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
    let Some(pipe_name) = ipc::win::user_pipe_name() else {
        log("cannot read user SID: not starting");
        return;
    };
    let name = wide(&pipe_name);
    log(&format!("started, pipe {pipe_name}"));

    // 샌드박스 앱(작업 표시줄 검색창 등) 안의 TIP도 접속할 수 있는 보안 설정
    let sa = pipe_security::pipe_security();
    log(if sa.is_some() { "pipe security: user + AppContainer, low integrity" } else { "pipe security: default (AppContainer apps cannot connect)" });
    let per_process = PER_PROCESS.get_or_init(pipe::PerProcess::new);
    let mut first = true;
    loop {
        let Some(h) = pipe::create(&name, sa.as_ref(), first) else {
            if first {
                // 같은 사용자의 백엔드는 뮤텍스로 막으니, 이 이름을 먼저 가진 것은 다른 프로그램이다
                log("pipe name already taken by another process: not starting");
                return;
            }
            log("CreateNamedPipeW failed");
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        };
        first = false;
        let connected =
            unsafe { ConnectNamedPipe(h, None) }.is_ok() || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe {
                let _ = CloseHandle(h);
            }
            continue;
        }
        // 동시 연결 상한: 넘으면 받자마자 끊는다 (TIP는 실패하면 키를 앱에 넘기고 나중에 다시 붙는다)
        let Some(slot) = CONNECTIONS.try_acquire() else {
            log_throttled(&REJECT_LOG, &format!("too many connections (max {}): rejected", pipe::MAX_CONNECTIONS));
            unsafe {
                let _ = DisconnectNamedPipe(h);
                let _ = CloseHandle(h);
            }
            continue;
        };
        // 프로세스별 상한: 샌드박스·낮은 무결성(또는 확인 실패)은 더 적게
        let pid = pipe::client_pid(h).unwrap_or(0);
        let identity = if pid != 0 { pipe::process_identity(pid) } else { None };
        let may_manage = identity.is_some_and(|(ac, rid)| pipe::may_manage(ac, rid));
        let cap = if may_manage { pipe::MAX_PER_PROCESS } else { pipe::MAX_PER_UNTRUSTED_PROCESS };
        let Some(proc_slot) = per_process.try_acquire(pid, cap) else {
            log_throttled(&REJECT_LOG, &format!("too many connections from process {pid} (max {cap}): rejected"));
            unsafe {
                let _ = DisconnectNamedPipe(h);
                let _ = CloseHandle(h);
            }
            continue;
        };
        let ph = PipeHandle(h.0 as isize);
        let hist = history.clone();
        std::thread::spawn(move || {
            let _slots = (slot, proc_slot);
            handle_client(ph, models, hist, identity, may_manage)
        });
    }
}
