//! Mozc 서버(mozc_server) 클라이언트. 일본어 구간의 가나→한자 변환과 Mozc 학습에 쓴다.
//!
//! poc/mozc/mozc_client.py의 Rust 이식:
//! - 서버는 샌드박스 런처가 만든 스레드 토큰이 없으면 실행을 거부한다(base/run_level.cc).
//!   그래서 일시 정지 상태로 띄우고 우리 토큰을 가장 토큰으로 복제해 메인 스레드에 붙인 뒤 재개한다.
//! - 서버가 사용자 프로필(LocalLow\Mozc)에 쓴 session.ipc에서 키를 읽어 \\.\pipe\mozc.<key>.session 에 붙는다.
//! - 요청 하나마다 연결 하나 (src/ipc/win32_ipc.cc). 서버가 직전 연결의 ACK를 기다리는 동안은 PIPE_BUSY.

/// Mozc protobuf 정의에서 prost가 만든 코드 (패키지 구조 그대로).
#[allow(clippy::all)]
pub mod mozc {
    include!(concat!(env!("OUT_DIR"), "/mozc.rs"));
    pub mod commands {
        include!(concat!(env!("OUT_DIR"), "/mozc.commands.rs"));
    }
    pub mod config {
        include!(concat!(env!("OUT_DIR"), "/mozc.config.rs"));
    }
    pub mod ipc {
        include!(concat!(env!("OUT_DIR"), "/mozc.ipc.rs"));
    }
}

use std::path::{Path, PathBuf};
use std::time::Duration;

use prost::Message;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_MORE_DATA, ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows::Win32::Security::{
    DuplicateTokenEx, SecurityImpersonation, TokenImpersonation, TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_NONE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Pipes::{SetNamedPipeHandleState, WaitNamedPipeW, PIPE_READMODE_MESSAGE};
use windows::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, OpenProcessToken, ResumeThread, SetThreadToken, CREATE_NO_WINDOW,
    CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW,
};

use mozc::commands::{input, key_event, session_command, CompositionMode, Input, KeyEvent, Output, SessionCommand};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug)]
pub enum Error {
    Win(windows::core::Error),
    Proto(String),
    NotRunning,
    Busy,
}

impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Error::Win(e)
    }
}

impl From<prost::DecodeError> for Error {
    fn from(e: prost::DecodeError) -> Self {
        Error::Proto(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Mozc 사용자 프로필 폴더 (session.ipc, 학습 데이터).
pub fn profile_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    Path::new(&home).join("AppData").join("LocalLow").join("Mozc")
}

/// mozc_server를 띄운다 (일시 정지 → 가장 토큰 → 재개).
pub fn launch_server(exe: &Path) -> Result<u32> {
    let exe_w = wide(&exe.to_string_lossy());
    let dir_w = wide(&exe.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default());
    let mut cmd = wide(&format!("\"{}\"", exe.to_string_lossy()));
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(exe_w.as_ptr()),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            None,
            PCWSTR(dir_w.as_ptr()),
            &si,
            &mut pi,
        )?;
        let mut token = HANDLE::default();
        let mut imp = HANDLE::default();
        let res = (|| -> Result<()> {
            OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut token)?;
            DuplicateTokenEx(token, TOKEN_IMPERSONATE | TOKEN_QUERY, None, SecurityImpersonation, TokenImpersonation, &mut imp)?;
            SetThreadToken(Some(&pi.hThread), Some(imp))?;
            Ok(())
        })();
        ResumeThread(pi.hThread);
        for h in [token, imp, pi.hThread, pi.hProcess] {
            if !h.0.is_null() && !h.is_invalid() {
                let _ = CloseHandle(h);
            }
        }
        res?;
    }
    Ok(pi.dwProcessId)
}

/// 서버가 잡고 있는 파일을 Mozc 클라이언트와 같은 공유 플래그로 읽는다.
fn read_shared(path: &Path) -> Result<Vec<u8>> {
    let p = wide(&path.to_string_lossy());
    unsafe {
        let h = CreateFileW(
            PCWSTR(p.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )?;
        let mut buf = vec![0u8; 4096];
        let mut n = 0u32;
        let r = ReadFile(h, Some(&mut buf), Some(&mut n), None);
        let _ = CloseHandle(h);
        r?;
        buf.truncate(n as usize);
        Ok(buf)
    }
}

/// 서버 연결 정보 (파이프 이름).
#[derive(Clone)]
pub struct Client {
    pipe: Vec<u16>,
}

impl Client {
    /// 실행 중인 서버에 붙는다. 서버가 없으면 Error::NotRunning.
    pub fn connect() -> Result<Self> {
        let data = read_shared(&profile_dir().join("session.ipc")).map_err(|_| Error::NotRunning)?;
        let info = mozc::ipc::IpcPathInfo::decode(&data[..])?;
        Ok(Self { pipe: wide(&format!(r"\\.\pipe\mozc.{}.session", info.key.unwrap_or_default())) })
    }

    /// 서버가 없으면 띄우고 붙는다 (최대 약 5초 대기).
    pub fn connect_or_launch(server_exe: &Path) -> Result<Self> {
        if let Ok(c) = Self::connect() {
            if c.ping() {
                return Ok(c);
            }
        }
        launch_server(server_exe)?;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(c) = Self::connect() {
                if c.ping() {
                    return Ok(c);
                }
            }
        }
        Err(Error::NotRunning)
    }

    fn ping(&self) -> bool {
        let i = Input { r#type: input::CommandType::NoOperation as i32, ..Default::default() };
        self.call(&i).is_ok()
    }

    pub fn call(&self, req: &Input) -> Result<Output> {
        let body = req.encode_to_vec();
        unsafe {
            let mut h = None;
            for _ in 0..50 {
                match CreateFileW(
                    PCWSTR(self.pipe.as_ptr()),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    None,
                ) {
                    Ok(x) => {
                        h = Some(x);
                        break;
                    }
                    Err(_) if GetLastError() == ERROR_PIPE_BUSY => {
                        let _ = WaitNamedPipeW(PCWSTR(self.pipe.as_ptr()), 1000);
                    }
                    Err(_) => return Err(Error::NotRunning),
                }
            }
            let h = h.ok_or(Error::Busy)?;
            let res = (|| -> Result<Output> {
                let mode = PIPE_READMODE_MESSAGE;
                SetNamedPipeHandleState(h, Some(&mode), None, None)?;
                let mut n = 0u32;
                WriteFile(h, Some(&body), Some(&mut n), None)?;
                let mut out = Vec::new();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let mut n = 0u32;
                    let r = ReadFile(h, Some(&mut buf), Some(&mut n), None);
                    out.extend_from_slice(&buf[..n as usize]);
                    match r {
                        Ok(()) => break,
                        Err(_) if GetLastError() == ERROR_MORE_DATA => continue,
                        Err(e) => return Err(e.into()),
                    }
                }
                Ok(Output::decode(&out[..])?)
            })();
            let _ = CloseHandle(h); // 연결을 끊는 것이 서버에 대한 ACK
            res
        }
    }
}

/// 변환 결과: 문절별 텍스트와, 첫 문절(포커스)의 후보.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conversion {
    pub segments: Vec<String>,
    pub candidates: Vec<(i32, String)>,
}

impl Conversion {
    pub fn text(&self) -> String {
        self.segments.concat()
    }
}

/// Mozc 세션 하나 (우리 백엔드 연결 하나에 하나).
pub struct Session {
    client: Client,
    id: u64,
}

impl Session {
    pub fn new(client: Client) -> Result<Self> {
        let i = Input { r#type: input::CommandType::CreateSession as i32, ..Default::default() };
        let out = client.call(&i)?;
        let mut s = Self { client, id: out.id.unwrap_or(0) };
        let c = SessionCommand {
            r#type: session_command::CommandType::TurnOnIme as i32,
            composition_mode: Some(CompositionMode::Hiragana as i32),
            ..Default::default()
        };
        s.command(c)?;
        Ok(s)
    }

    fn command(&mut self, c: SessionCommand) -> Result<Output> {
        let i = Input {
            r#type: input::CommandType::SendCommand as i32,
            id: Some(self.id),
            command: Some(c),
            ..Default::default()
        };
        self.client.call(&i)
    }

    fn send_key(&mut self, k: KeyEvent) -> Result<Output> {
        let i = Input { r#type: input::CommandType::SendKey as i32, id: Some(self.id), key: Some(k), ..Default::default() };
        self.client.call(&i)
    }

    fn simple(&mut self, t: session_command::CommandType) -> Result<Output> {
        self.command(SessionCommand { r#type: t as i32, ..Default::default() })
    }

    pub fn revert(&mut self) -> Result<()> {
        self.simple(session_command::CommandType::Revert).map(|_| ())
    }

    /// 조합을 주어진 가나로 통째로 바꾼다.
    pub fn set_composition(&mut self, kana: &str) -> Result<Output> {
        let ev = session_command::CompositionEvent { composition_string: Some(kana.to_string()), probability: Some(1.0) };
        self.command(SessionCommand {
            r#type: session_command::CommandType::UpdateComposition as i32,
            composition_events: vec![ev],
            ..Default::default()
        })
    }

    /// 가나를 변환해 문절과 후보를 돌려준다 (조합은 변환 상태로 남는다).
    pub fn convert(&mut self, kana: &str) -> Result<Conversion> {
        self.revert()?;
        self.set_composition(kana)?;
        let k = KeyEvent { special_key: Some(key_event::SpecialKey::Space as i32), ..Default::default() };
        let out = self.send_key(k)?;
        Ok(conversion(&out))
    }

    /// 첫 문절의 후보를 id로 고른다.
    pub fn select(&mut self, id: i32) -> Result<Conversion> {
        let out = self.command(SessionCommand {
            r#type: session_command::CommandType::SelectCandidate as i32,
            id: Some(id),
            ..Default::default()
        })?;
        Ok(conversion(&out))
    }

    /// 현재 변환을 확정한다 (Mozc 학습이 일어난다). 확정된 텍스트를 돌려준다.
    pub fn submit(&mut self) -> Result<String> {
        let out = self.simple(session_command::CommandType::Submit)?;
        Ok(out.result.map(|r| r.value).unwrap_or_default())
    }
}

fn conversion(out: &Output) -> Conversion {
    let segments = out.preedit.as_ref().map(|p| p.segment.iter().map(|s| s.value.clone()).collect()).unwrap_or_default();
    let mut candidates: Vec<(i32, String)> = out
        .candidate_window
        .as_ref()
        .map(|w| w.candidate.iter().map(|c| (c.id.unwrap_or(0), c.value.clone())).collect())
        .unwrap_or_default();
    if candidates.is_empty() {
        candidates = out
            .all_candidate_words
            .as_ref()
            .map(|l| l.candidates.iter().map(|c| (c.id.unwrap_or(0), c.value.clone().unwrap_or_default())).collect())
            .unwrap_or_default();
    }
    Conversion { segments, candidates }
}
