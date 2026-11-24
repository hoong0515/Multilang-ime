//! 등록되는 TSF 텍스트 서비스 DLL (`multilang_tip.dll`, stub).
//!
//! COM 등록(InprocServer32)과 TSF 프로필 아이콘은 이 DLL을 가리키고, 실제 입력기 코드는
//! `multilang_tip_impl.dll`(impl)에 있다. stub은 처음 불릴 때 impl을 읽고 네 개의 DLL 진입점을 넘긴다.
//!
//! - impl 위치는 stub 옆의 `multilang_tip.impl` 파일 (한 줄, stub 폴더 기준 상대 경로, 예:
//!   `impl\3f2a…\multilang_tip_impl.dll`). 설치 프로그램이 새 impl을 새 폴더에 넣은 뒤 이 파일을 바꾼다.
//!   앱이 쓰고 있는 이전 impl은 이름이 겹치지 않으므로 그대로 두면 되고, 업데이트에 재부팅이 필요 없다.
//!   이미 열려 있던 앱은 읽어 둔 impl을 계속 쓰고, 새로 여는 앱부터 새 impl을 쓴다.
//! - 파일이 없으면 stub 옆의 `multilang_tip_impl.dll` (개발 빌드: target\release에 둘이 나란히 있다).
//! - 이 DLL은 거의 바뀌지 않게 작게 둔다 (바뀌면 쓰는 중인 파일을 바꾸는 예전 방식이 된다).

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, E_FAIL, E_POINTER, HMODULE, S_FALSE};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::core::{BOOL, GUID, HRESULT, PCSTR, PCWSTR};

static MODULE: AtomicIsize = AtomicIsize::new(0);
/// 읽은 impl (프로세스 동안 하나). 읽지 못했으면 None.
static IMPL: OnceLock<Option<isize>> = OnceLock::new();

const POINTER_FILE: &str = "multilang_tip.impl";
const IMPL_NAME: &str = "multilang_tip_impl.dll";

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module.0 as isize, Ordering::Relaxed);
    }
    true.into()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 이 DLL(stub)의 전체 경로.
fn stub_path() -> Option<String> {
    let module = HMODULE(MODULE.load(Ordering::Relaxed) as *mut c_void);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    (n > 0 && n < buf.len()).then(|| String::from_utf16_lossy(&buf[..n]))
}

/// 가리키는 파일의 한 줄이 받아들일 상대 경로인가: 비어 있지 않고, 드라이브·루트·`..`·와일드카드가 없고, impl 이름으로 끝난다.
fn valid_relative(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split(['\\', '/']).collect();
    !rel.is_empty()
        && !rel.contains(':')
        && !rel.starts_with(['\\', '/'])
        && !rel.contains(['*', '?', '"', '<', '>', '|'])
        && parts.iter().all(|p| !p.is_empty() && *p != "." && *p != "..")
        && parts.last().is_some_and(|l| l.eq_ignore_ascii_case(IMPL_NAME))
}

/// 읽을 impl의 경로: 가리키는 파일의 경로, 없거나 잘못됐으면 stub 옆.
fn impl_path(stub: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(stub).parent().unwrap_or(std::path::Path::new(""));
    if let Ok(text) = std::fs::read_to_string(dir.join(POINTER_FILE)) {
        let rel = text.trim().trim_start_matches('\u{feff}');
        if valid_relative(rel) {
            return dir.join(rel);
        }
    }
    dir.join(IMPL_NAME)
}

type SetHost = unsafe extern "system" fn(PCWSTR);

/// impl을 읽고 stub 경로를 알려 준다 (impl은 등록·백엔드·설정 화면 위치를 stub 기준으로 찾는다).
fn load_impl() -> Option<HMODULE> {
    let h = *IMPL.get_or_init(|| {
        let stub = stub_path()?;
        let path = wide(&impl_path(&stub).to_string_lossy());
        let h = unsafe { LoadLibraryExW(PCWSTR(path.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }.ok()?;
        let stub_w = wide(&stub);
        if let Some(f) = unsafe { GetProcAddress(h, PCSTR(c"MultilangSetHost".as_ptr() as *const u8)) } {
            let set: SetHost = unsafe { std::mem::transmute(f) };
            unsafe { set(PCWSTR(stub_w.as_ptr())) };
        }
        Some(h.0 as isize)
    });
    h.map(|h| HMODULE(h as *mut c_void))
}

fn proc(name: &std::ffi::CStr) -> Option<unsafe extern "system" fn() -> isize> {
    let h = load_impl()?;
    unsafe { GetProcAddress(h, PCSTR(name.as_ptr() as *const u8)) }
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = std::ptr::null_mut() };
    type F = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    match proc(c"DllGetClassObject") {
        Some(f) => unsafe { std::mem::transmute::<_, F>(f)(rclsid, riid, ppv) },
        None => CLASS_E_CLASSNOTAVAILABLE,
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    // impl도 언로드하지 않는다
    S_FALSE
}

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    type F = unsafe extern "system" fn() -> HRESULT;
    match proc(c"DllRegisterServer") {
        Some(f) => unsafe { std::mem::transmute::<_, F>(f)() },
        None => E_FAIL,
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    type F = unsafe extern "system" fn() -> HRESULT;
    match proc(c"DllUnregisterServer") {
        Some(f) => unsafe { std::mem::transmute::<_, F>(f)() },
        None => E_FAIL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_paths() {
        assert!(valid_relative(r"impl\0123abcd\multilang_tip_impl.dll"));
        assert!(valid_relative("impl/0123/multilang_tip_impl.dll"));
        assert!(valid_relative("multilang_tip_impl.dll"));
        assert!(!valid_relative(""));
        assert!(!valid_relative(r"..\evil\multilang_tip_impl.dll"));
        assert!(!valid_relative(r"impl\..\..\multilang_tip_impl.dll"));
        assert!(!valid_relative(r"C:\x\multilang_tip_impl.dll"));
        assert!(!valid_relative(r"\x\multilang_tip_impl.dll"));
        assert!(!valid_relative(r"impl\x\other.dll"));
        assert!(!valid_relative(r"impl\\multilang_tip_impl.dll"));
    }

    #[test]
    fn impl_path_falls_back_next_to_stub() {
        let dir = std::env::temp_dir().join(format!("tipstub-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stub = dir.join("multilang_tip.dll").to_string_lossy().to_string();
        assert_eq!(impl_path(&stub), dir.join(IMPL_NAME));
        std::fs::write(dir.join(POINTER_FILE), "\u{feff}impl\\abc\\multilang_tip_impl.dll\r\n").unwrap();
        assert_eq!(impl_path(&stub), dir.join(r"impl\abc\multilang_tip_impl.dll"));
        std::fs::write(dir.join(POINTER_FILE), r"..\multilang_tip_impl.dll").unwrap();
        assert_eq!(impl_path(&stub), dir.join(IMPL_NAME));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
