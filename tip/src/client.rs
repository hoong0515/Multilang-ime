//! 백엔드 연결. TIP 인스턴스마다 파이프 연결 하나를 유지한다.
//!
//! - 응답이 TIMEOUT_MS 안에 오지 않거나 연결이 끊겼으면 연결을 닫고 실패를 돌려준다.
//! - 새로 연결할 때마다 gen이 늘어난다. 백엔드는 연결마다 조합 상태를 새로 만들므로,
//!   부르는 쪽(TextService)은 gen이 바뀌면 지금 조합의 키를 다시 보내 상태를 되살린다.
//! - 백엔드가 없으면 DLL 옆의 backend.exe를 한 번 띄우고 잠깐 기다린다.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use ipc::{Request, Response};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{SetNamedPipeHandleState, TransactNamedPipe, PIPE_READMODE_MESSAGE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateProcessW, WaitForSingleObject, CREATE_NO_WINDOW, DETACHED_PROCESS, PROCESS_INFORMATION,
    STARTUPINFOW,
};

const TIMEOUT_MS: u32 = 200;
const RESPAWN_INTERVAL: Duration = Duration::from_secs(5);

pub struct Client {
    pipe: Option<HANDLE>,
    event: Option<HANDLE>,
    last_spawn: Option<Instant>,
    /// 연결 세대 (새로 연결할 때마다 1 증가)
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fail {
    /// 응답이 늦었다 (백엔드가 바쁨)
    Timeout,
    /// 바로 실패했다 (연결 끊김: 백엔드 재시작 등)
    Broken,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Client {
    pub fn new() -> Self {
        Self { pipe: None, event: None, last_spawn: None, generation: 0 }
    }

    fn pipe_name() -> Vec<u16> {
        wide(&ipc::pipe_name(&std::env::var("USERNAME").unwrap_or_default()))
    }

    fn open() -> Option<HANDLE> {
        let name = Self::pipe_name();
        let h = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .ok()?;
        let mode = PIPE_READMODE_MESSAGE;
        if unsafe { SetNamedPipeHandleState(h, Some(&mode), None, None) }.is_err() {
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }
        Some(h)
    }

    /// DLL과 같은 폴더의 backend.exe를 띄운다.
    fn spawn_backend(&mut self) {
        if self.last_spawn.is_some_and(|t| t.elapsed() < RESPAWN_INTERVAL) {
            return;
        }
        self.last_spawn = Some(Instant::now());
        let dll = crate::module_path();
        let dll = String::from_utf16_lossy(&dll);
        let Some(dir) = std::path::Path::new(&dll).parent() else { return };
        let exe = dir.join("backend.exe");
        let exe_w = wide(&exe.to_string_lossy());
        let dir_w = wide(&dir.to_string_lossy());
        let mut cmd = wide(&format!("\"{}\"", exe.to_string_lossy()));
        let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        let mut pi = PROCESS_INFORMATION::default();
        let ok = unsafe {
            CreateProcessW(
                PCWSTR(exe_w.as_ptr()),
                Some(PWSTR(cmd.as_mut_ptr())),
                None,
                None,
                false,
                DETACHED_PROCESS | CREATE_NO_WINDOW,
                None,
                PCWSTR(dir_w.as_ptr()),
                &si,
                &mut pi,
            )
        };
        if ok.is_ok() {
            unsafe {
                let _ = CloseHandle(pi.hThread);
                let _ = CloseHandle(pi.hProcess);
            }
        }
    }

    pub fn connect(&mut self) -> bool {
        if self.pipe.is_some() {
            return true;
        }
        if let Some(h) = Self::open() {
            self.pipe = Some(h);
            self.generation += 1;
            return true;
        }
        self.spawn_backend();
        // 띄운 직후 모델을 읽는 동안 잠깐 기다린다 (첫 키에서만)
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(h) = Self::open() {
                self.pipe = Some(h);
                self.generation += 1;
                return true;
            }
        }
        false
    }

    fn disconnect(&mut self) {
        if let Some(h) = self.pipe.take() {
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }

    /// 요청 하나를 보내고 응답을 받는다 (연결되어 있어야 한다).
    pub fn transact(&mut self, req: &Request) -> Result<Response, Fail> {
        let pipe = self.pipe.ok_or(Fail::Broken)?;
        if self.event.is_none() {
            self.event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok();
        }
        let event = self.event.ok_or(Fail::Broken)?;
        let body = serde_json::to_vec(req).map_err(|_| Fail::Broken)?;
        let mut out = vec![0u8; 64 * 1024];
        let mut ov = OVERLAPPED { hEvent: event, ..Default::default() };
        let mut n = 0u32;
        let started = unsafe {
            TransactNamedPipe(
                pipe,
                Some(body.as_ptr() as *const c_void),
                body.len() as u32,
                Some(out.as_mut_ptr() as *mut c_void),
                out.len() as u32,
                &mut n,
                Some(&mut ov),
            )
        };
        let done = match started {
            Ok(()) => Ok(()),
            Err(_) if unsafe { GetLastError() } == ERROR_IO_PENDING => {
                if unsafe { WaitForSingleObject(event, TIMEOUT_MS) } == WAIT_OBJECT_0 {
                    if unsafe { GetOverlappedResult(pipe, &ov, &mut n, false) }.is_ok() { Ok(()) } else { Err(Fail::Broken) }
                } else {
                    unsafe {
                        let _ = CancelIoEx(pipe, Some(&ov));
                        let _ = GetOverlappedResult(pipe, &ov, &mut n, true);
                    }
                    Err(Fail::Timeout)
                }
            }
            Err(_) => Err(Fail::Broken),
        };
        if let Err(f) = done {
            // 늦은 응답이 다음 요청에 섞이지 않게 연결을 닫는다 (다음 요청은 새 연결 = 새 세대)
            self.disconnect();
            return Err(f);
        }
        serde_json::from_slice(&out[..n as usize]).map_err(|_| Fail::Broken)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.disconnect();
        if let Some(e) = self.event.take() {
            unsafe {
                let _ = CloseHandle(e);
            }
        }
    }
}
