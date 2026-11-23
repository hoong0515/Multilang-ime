//! 백엔드 파이프의 생성·읽기·접속 제한 (보안 검토 R1·R3·R5).
//!
//! - 요청 하나는 `MAX_REQUEST` 바이트까지. 넘으면 연결을 끊는다 (메모리 누적 방지).
//! - 동시 연결은 `MAX_CONNECTIONS`개까지. 넘는 연결은 받자마자 끊는다 (스레드 소진 방지).
//! - 원격(다른 컴퓨터) 클라이언트는 받지 않는다 (`PIPE_REJECT_REMOTE_CLIENTS`).
//! - 연결한 프로세스가 샌드박스(AppContainer)이거나 보통(Medium)보다 낮은 무결성이면
//!   관리 명령(학습 지우기)을 거부한다. 키 처리는 그대로 받는다 (검색창 같은 샌드박스 앱 안의 TIP).

use std::ffi::c_void;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_MORE_DATA, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel, TokenIsAppContainer,
    SECURITY_ATTRIBUTES, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{ReadFile, FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

/// 요청 하나의 최대 크기. 실제 요청은 수백 바이트 (키 하나, 프로그램 이름 하나).
pub const MAX_REQUEST: usize = 16 * 1024;
/// 동시에 붙어 있을 수 있는 연결 수 (TIP는 앱의 입력 스레드마다 하나씩 붙는다).
pub const MAX_CONNECTIONS: usize = 128;
/// 프로세스 하나가 가질 수 있는 연결 수: 보통 앱 (탐색기는 창마다 스레드가 따로라 넉넉히).
pub const MAX_PER_PROCESS: usize = 64;
/// 샌드박스(AppContainer)·낮은 무결성 프로세스 하나의 연결 수 (검색창은 몇 개면 된다).
pub const MAX_PER_UNTRUSTED_PROCESS: usize = 8;
/// `Hello.app`(실행 파일 이름)의 최대 길이 (글자 수, MAX_PATH).
pub const MAX_APP_NAME: usize = 260;
const BUF_SIZE: u32 = 16 * 1024;

/// 파이프 인스턴스 하나를 만든다. 듣는 인스턴스 하나를 더 둘 수 있게 상한은 연결 수 + 1.
pub fn create(name: &[u16], sa: Option<&SECURITY_ATTRIBUTES>) -> Option<HANDLE> {
    let h = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAGS_AND_ATTRIBUTES(0),
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            (MAX_CONNECTIONS + 1) as u32,
            BUF_SIZE,
            BUF_SIZE,
            0,
            sa.map(|a| a as *const _),
        )
    };
    (h != INVALID_HANDLE_VALUE).then_some(h)
}

#[derive(Debug, PartialEq)]
pub enum Read {
    Message(Vec<u8>),
    /// 상한을 넘는 요청. 남은 내용은 읽지 않고 연결을 끊는다.
    TooLarge,
    Closed,
}

/// 메시지 하나를 읽는다. `max` 바이트를 넘으면 그 자리에서 멈춘다 (그 이상 쌓지 않는다).
pub fn read_message(h: HANDLE, max: usize) -> Read {
    let mut out = Vec::new();
    let mut buf = vec![0u8; BUF_SIZE as usize];
    loop {
        let mut n = 0u32;
        let res = unsafe { ReadFile(h, Some(&mut buf), Some(&mut n), None) };
        if out.len() + n as usize > max {
            return Read::TooLarge;
        }
        out.extend_from_slice(&buf[..n as usize]);
        match res {
            Ok(()) => return Read::Message(out),
            Err(_) if unsafe { GetLastError() } == ERROR_MORE_DATA => continue,
            Err(_) => return Read::Closed,
        }
    }
}

/// 동시 연결 수 세기. `try_acquire`가 Some이면 그 값이 살아 있는 동안 한 자리를 차지한다.
pub struct Slots {
    active: AtomicUsize,
    max: usize,
}

pub struct Slot<'a>(&'a Slots);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Slots {
    pub const fn new(max: usize) -> Self {
        Self { active: AtomicUsize::new(0), max }
    }

    pub fn try_acquire(&self) -> Option<Slot<'_>> {
        let mut n = self.active.load(Ordering::SeqCst);
        while n < self.max {
            match self.active.compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return Some(Slot(self)),
                Err(now) => n = now,
            }
        }
        None
    }

    #[cfg(test)]
    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
}

/// 프로세스별 연결 수 (보안 재검토 2026-11-24 R4: 한 프로세스가 연결을 열어 두기만 해서 전체 상한을 다 차지하지 못하게).
/// 정상 TIP 연결도 오래 쉬므로 유휴 시간으로 끊지는 않는다.
pub struct PerProcess {
    counts: Mutex<HashMap<u32, usize>>,
}

pub struct ProcessSlot<'a> {
    owner: &'a PerProcess,
    pid: u32,
}

impl Drop for ProcessSlot<'_> {
    fn drop(&mut self) {
        if let Ok(mut m) = self.owner.counts.lock() {
            if let Some(n) = m.get_mut(&self.pid) {
                *n -= 1;
                if *n == 0 {
                    m.remove(&self.pid);
                }
            }
        }
    }
}

impl PerProcess {
    pub fn new() -> Self {
        Self { counts: Mutex::new(HashMap::new()) }
    }

    pub fn try_acquire(&self, pid: u32, max: usize) -> Option<ProcessSlot<'_>> {
        let mut m = self.counts.lock().ok()?;
        let n = m.entry(pid).or_insert(0);
        if *n >= max {
            return None;
        }
        *n += 1;
        Some(ProcessSlot { owner: self, pid })
    }
}

/// 파이프 건너편 프로세스 ID.
pub fn client_pid(h: HANDLE) -> Option<u32> {
    let mut pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(h, &mut pid) }.ok()?;
    Some(pid)
}

/// `Hello.app`로 받아들일 이름인가: 비어 있지 않고, 너무 길지 않고, 제어 문자·경로 구분자가 없다.
pub fn valid_app_name(app: &str) -> bool {
    !app.is_empty() && app.chars().count() <= MAX_APP_NAME && !app.chars().any(|c| c.is_control() || c == '\\' || c == '/')
}

/// 보통 무결성 (SECURITY_MANDATORY_MEDIUM_RID).
const MEDIUM_RID: u32 = 0x2000;

/// 관리 명령을 허락할 클라이언트인가: 샌드박스가 아니고 무결성이 Medium 이상.
pub fn may_manage(app_container: bool, integrity_rid: u32) -> bool {
    !app_container && integrity_rid >= MEDIUM_RID
}

/// 파이프 건너편 프로세스의 (샌드박스 여부, 무결성 RID). 알 수 없으면 None.
#[cfg(test)]
pub fn client_identity(h: HANDLE) -> Option<(bool, u32)> {
    process_identity(client_pid(h)?)
}

pub fn process_identity(pid: u32) -> Option<(bool, u32)> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(process);
        opened.ok()?;
        let out = token_identity(token);
        let _ = CloseHandle(token);
        out
    }
}

unsafe fn token_identity(token: HANDLE) -> Option<(bool, u32)> {
    unsafe {
        let mut is_ac = 0u32;
        let mut len = 0u32;
        GetTokenInformation(token, TokenIsAppContainer, Some(&mut is_ac as *mut u32 as *mut c_void), 4, &mut len).ok()?;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut len);
        if len == 0 {
            return None;
        }
        // TOKEN_MANDATORY_LABEL 정렬을 위해 u64 버퍼
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        GetTokenInformation(token, TokenIntegrityLevel, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len).ok()?;
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        let rid = *GetSidSubAuthority(sid, (count - 1) as u32);
        Some((is_ac != 0, rid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use windows::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn unique_name(tag: &str) -> String {
        format!(r"\\.\pipe\multilang-ime-test.{tag}.{}", std::process::id())
    }

    /// 서버 인스턴스를 만들고, 다른 스레드에서 클라이언트로 `messages`를 차례로 보낸 뒤, 서버 쪽 읽기 결과를 돌려준다.
    fn roundtrip(tag: &str, messages: Vec<Vec<u8>>, reads: usize) -> Vec<Read> {
        let name = unique_name(tag);
        let server = create(&wide(&name), None).expect("create pipe");
        let client_name = name.clone();
        let client = std::thread::spawn(move || {
            let mut f = loop {
                match std::fs::OpenOptions::new().read(true).write(true).open(&client_name) {
                    Ok(f) => break f,
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            };
            for m in messages {
                if f.write_all(&m).is_err() {
                    break;
                }
            }
            // 서버가 다 읽을 때까지 연결을 유지
            std::thread::sleep(std::time::Duration::from_millis(300));
        });
        let _ = unsafe { ConnectNamedPipe(server, None) };
        let out = (0..reads).map(|_| read_message(server, MAX_REQUEST)).collect();
        unsafe {
            let _ = DisconnectNamedPipe(server);
            let _ = CloseHandle(server);
        }
        client.join().unwrap();
        out
    }

    #[test]
    fn reads_small_request() {
        let msg = br#"{"op":"status"}"#.to_vec();
        assert_eq!(roundtrip("small", vec![msg.clone()], 1), vec![Read::Message(msg)]);
    }

    #[test]
    fn accepts_request_at_limit() {
        let msg = vec![b' '; MAX_REQUEST];
        assert_eq!(roundtrip("limit", vec![msg.clone()], 1), vec![Read::Message(msg)]);
    }

    #[test]
    fn rejects_request_over_limit() {
        // 상한 + 1 바이트, 그리고 버퍼 여러 개에 걸치는 큰 요청
        assert_eq!(roundtrip("over1", vec![vec![b' '; MAX_REQUEST + 1]], 1), vec![Read::TooLarge]);
        assert_eq!(roundtrip("over4m", vec![vec![b' '; 4 << 20]], 1), vec![Read::TooLarge]);
    }

    #[test]
    fn reads_consecutive_requests_separately() {
        let a = br#"{"op":"flush"}"#.to_vec();
        let b = br#"{"op":"reset"}"#.to_vec();
        assert_eq!(roundtrip("seq", vec![a.clone(), b.clone()], 2), vec![Read::Message(a), Read::Message(b)]);
    }

    #[test]
    fn slots_limit_concurrent_connections() {
        let slots = Slots::new(2);
        let a = slots.try_acquire().expect("1");
        let b = slots.try_acquire().expect("2");
        assert!(slots.try_acquire().is_none());
        drop(a);
        assert_eq!(slots.active(), 1);
        let c = slots.try_acquire().expect("freed slot");
        assert!(slots.try_acquire().is_none());
        drop((b, c));
        assert_eq!(slots.active(), 0);
    }

    #[test]
    fn per_process_limit() {
        let p = PerProcess::new();
        let a: Vec<_> = (0..3).map(|_| p.try_acquire(10, 3).expect("under cap")).collect();
        assert!(p.try_acquire(10, 3).is_none());
        // 다른 프로세스는 따로 센다
        let other = p.try_acquire(11, 3).expect("other process");
        drop(a);
        assert!(p.try_acquire(10, 3).is_some());
        drop(other);
        assert!(p.counts.lock().unwrap().is_empty());
    }

    #[test]
    fn app_name_limits() {
        assert!(valid_app_name("notepad.exe"));
        assert!(valid_app_name("카카오톡.exe"));
        assert!(valid_app_name(&"a".repeat(MAX_APP_NAME)));
        assert!(!valid_app_name(&"a".repeat(MAX_APP_NAME + 1)));
        assert!(!valid_app_name(""));
        assert!(!valid_app_name("a\u{0}b"));
        assert!(!valid_app_name("c:\\windows\\notepad.exe"));
    }

    #[test]
    fn manage_policy() {
        assert!(may_manage(false, 0x2000)); // Medium
        assert!(may_manage(false, 0x3000)); // High (관리자)
        assert!(!may_manage(false, 0x1000)); // Low
        assert!(!may_manage(true, 0x1000)); // AppContainer
        assert!(!may_manage(true, 0x2000));
    }

    #[test]
    fn this_process_is_trusted() {
        let (ac, rid) = process_identity(std::process::id()).expect("identity");
        assert!(!ac);
        assert!(may_manage(ac, rid), "rid {rid:#x}");
    }

    /// 낮은 무결성(Low)으로 띄운 자식 프로세스가 파이프에 붙으면 관리 명령을 허락하지 않는다.
    #[test]
    fn low_integrity_client_may_not_manage() {
        use windows::core::PWSTR;
        use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
        use windows::Win32::Security::{
            DuplicateTokenEx, SecurityImpersonation, SetTokenInformation, TokenPrimary, PSID, SID_AND_ATTRIBUTES,
            TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
        };
        use windows::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe};
        use windows::Win32::System::Threading::{
            CreateProcessAsUserW, GetCurrentProcess, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW,
            PROCESS_INFORMATION, STARTUPINFOW,
        };

        const SE_GROUP_INTEGRITY: u32 = 0x20;
        let name = unique_name("lowil");
        let server = create(&wide(&name), None).expect("create pipe");
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT | TOKEN_ASSIGN_PRIMARY, &mut token)
                .expect("open token");
            let mut low = HANDLE::default();
            DuplicateTokenEx(token, windows::Win32::Security::TOKEN_ACCESS_MASK(0), None, SecurityImpersonation, TokenPrimary, &mut low)
                .expect("duplicate");
            let mut sid = PSID::default();
            ConvertStringSidToSidW(windows::core::w!("S-1-16-4096"), &mut sid).expect("low sid");
            let label = TOKEN_MANDATORY_LABEL { Label: SID_AND_ATTRIBUTES { Sid: sid, Attributes: SE_GROUP_INTEGRITY } };
            SetTokenInformation(low, TokenIntegrityLevel, &label as *const _ as *const c_void, std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32)
                .expect("set low integrity");
            // 파이프를 읽기로 열고 잠시 기다리는 자식 (cmd 리디렉션)
            let mut cmd = wide(&format!(r#"cmd.exe /d /c "ping -n 4 127.0.0.1 > nul < {name}""#));
            let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
            let mut pi = PROCESS_INFORMATION::default();
            CreateProcessAsUserW(Some(low), None, Some(PWSTR(cmd.as_mut_ptr())), None, None, false, CREATE_NO_WINDOW, None, None, &si, &mut pi)
                .expect("spawn low-integrity child");
            let _ = ConnectNamedPipe(server, None);
            let id = client_identity(server);
            let _ = TerminateProcess(pi.hProcess, 0);
            let _ = WaitForSingleObject(pi.hProcess, 5000);
            let _ = DisconnectNamedPipe(server);
            for h in [server, pi.hProcess, pi.hThread, low, token] {
                let _ = CloseHandle(h);
            }
            let (ac, rid) = id.expect("client identity");
            assert_eq!((ac, rid), (false, 0x1000));
            assert!(!may_manage(ac, rid));
        }
    }
}
