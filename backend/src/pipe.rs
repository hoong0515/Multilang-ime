//! 백엔드 파이프의 생성·읽기·접속 제한 (보안 검토 R1·R5).
//!
//! - 요청 하나는 `MAX_REQUEST` 바이트까지. 넘으면 연결을 끊는다 (메모리 누적 방지).
//! - 동시 연결은 `MAX_CONNECTIONS`개까지. 넘는 연결은 받자마자 끊는다 (스레드 소진 방지).
//! - 원격(다른 컴퓨터) 클라이언트는 받지 않는다 (`PIPE_REJECT_REMOTE_CLIENTS`).

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_MORE_DATA, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{ReadFile, FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};

/// 요청 하나의 최대 크기. 실제 요청은 수백 바이트 (키 하나, 프로그램 이름 하나).
pub const MAX_REQUEST: usize = 16 * 1024;
/// 동시에 붙어 있을 수 있는 연결 수 (TIP는 앱의 입력 스레드마다 하나씩 붙는다).
pub const MAX_CONNECTIONS: usize = 128;
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

/// `Hello.app`로 받아들일 이름인가: 비어 있지 않고, 너무 길지 않고, 제어 문자·경로 구분자가 없다.
pub fn valid_app_name(app: &str) -> bool {
    !app.is_empty() && app.chars().count() <= MAX_APP_NAME && !app.chars().any(|c| c.is_control() || c == '\\' || c == '/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use windows::Win32::Foundation::CloseHandle;
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
    fn app_name_limits() {
        assert!(valid_app_name("notepad.exe"));
        assert!(valid_app_name("카카오톡.exe"));
        assert!(valid_app_name(&"a".repeat(MAX_APP_NAME)));
        assert!(!valid_app_name(&"a".repeat(MAX_APP_NAME + 1)));
        assert!(!valid_app_name(""));
        assert!(!valid_app_name("a\u{0}b"));
        assert!(!valid_app_name("c:\\windows\\notepad.exe"));
    }

}
