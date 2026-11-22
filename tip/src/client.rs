//! 백엔드 연결. TIP 인스턴스마다 파이프 연결 하나를 유지한다.
//!
//! - 응답이 TIMEOUT_MS 안에 오지 않으면 연결을 끊고 None을 돌려준다 (호출한 쪽은 키를 먹지 않는다).
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
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Client {
    pub fn new() -> Self {
        Self { pipe: None, event: None, last_spawn: None }
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

    fn ensure_connected(&mut self) -> bool {
        if self.pipe.is_some() {
            return true;
        }
        if let Some(h) = Self::open() {
            self.pipe = Some(h);
            return true;
        }
        self.spawn_backend();
        // 띄운 직후 모델을 읽는 동안 잠깐 기다린다 (첫 키에서만)
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(h) = Self::open() {
                self.pipe = Some(h);
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

    pub fn call(&mut self, req: &Request) -> Option<Response> {
        if !self.ensure_connected() {
            return None;
        }
        let pipe = self.pipe?;
        if self.event.is_none() {
            self.event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok();
        }
        let event = self.event?;
        let body = serde_json::to_vec(req).ok()?;
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
            Ok(()) => true,
            Err(_) if unsafe { GetLastError() } == ERROR_IO_PENDING => {
                if unsafe { WaitForSingleObject(event, TIMEOUT_MS) } == WAIT_OBJECT_0 {
                    unsafe { GetOverlappedResult(pipe, &ov, &mut n, false) }.is_ok()
                } else {
                    unsafe {
                        let _ = CancelIoEx(pipe, Some(&ov));
                        let _ = GetOverlappedResult(pipe, &ov, &mut n, true);
                    }
                    false
                }
            }
            Err(_) => false,
        };
        if !done {
            self.disconnect();
            return None;
        }
        serde_json::from_slice(&out[..n as usize]).ok()
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
