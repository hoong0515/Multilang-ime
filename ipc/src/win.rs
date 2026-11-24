//! 파이프 이름과 서버 확인 (TIP·백엔드 공용).
//!
//! - 파이프 이름은 사용자 SID로 만든다. 환경 변수 `USERNAME`은 바꿀 수 있고, 같은 이름의 다른 계정
//!   (로컬·도메인)도 구분하지 못한다.
//! - 파이프 이름 공간은 로그온 세션 사이에 공유된다. 다른 사용자가 같은 이름의 파이프를 먼저 만들어 두면
//!   그 사람이 키 입력을 받게 된다. 그래서 TIP는 붙은 파이프의 소유자가 자기 계정(또는 Administrators)인지
//!   확인한다. 파이프 소유자는 만든 프로세스 토큰의 기본 소유자라, 다른 계정은 남의 SID로 만들 수 없다.
//!   같은 계정의 프로그램이 먼저 만드는 경우는 막지 못한다 (같은 계정 키로거와 같은 수준).

use std::ffi::c_void;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT};
use windows::Win32::Security::{
    EqualSid, GetTokenInformation, IsWellKnownSid, TokenUser, WinBuiltinAdministratorsSid, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// 이 프로세스 토큰의 TOKEN_USER (SID가 버퍼 안을 가리키므로 버퍼째 들고 다닌다).
struct TokenUserBuf(Vec<u64>);

impl TokenUserBuf {
    fn current() -> Option<Self> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            if len == 0 {
                let _ = CloseHandle(token);
                return None;
            }
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let r = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut c_void), len, &mut len);
            let _ = CloseHandle(token);
            r.ok()?;
            Some(Self(buf))
        }
    }

    fn sid(&self) -> PSID {
        unsafe { (*(self.0.as_ptr() as *const TOKEN_USER)).User.Sid }
    }
}

/// 이 프로세스 사용자의 SID 문자열 (S-1-5-21-...).
pub fn user_sid() -> Option<String> {
    let user = TokenUserBuf::current()?;
    unsafe {
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.sid(), &mut s).ok()?;
        let out = s.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(s.0 as *mut c_void)));
        out
    }
}

/// 이 사용자의 백엔드 파이프 이름. SID를 모르면 None (그때는 연결하지 않는다).
pub fn user_pipe_name() -> Option<String> {
    user_sid().map(|sid| crate::pipe_name(&sid))
}

/// 붙은 파이프(클라이언트 쪽 핸들)의 소유자가 이 사용자이거나 Administrators인가.
/// 백엔드가 관리자 권한으로 떠 있으면 기본 소유자가 Administrators다 (다른 계정이 만들 수는 없다).
pub fn pipe_owner_trusted(h: HANDLE) -> bool {
    let Some(user) = TokenUserBuf::current() else { return false };
    unsafe {
        let mut owner = PSID::default();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let r = GetSecurityInfo(h, SE_KERNEL_OBJECT, OWNER_SECURITY_INFORMATION, Some(&mut owner), None, None, None, Some(&mut sd));
        if r.is_err() || owner.is_invalid() {
            if !sd.is_invalid() {
                let _ = LocalFree(Some(HLOCAL(sd.0)));
            }
            return false;
        }
        let ok = EqualSid(owner, user.sid()).is_ok() || IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool();
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        ok
    }
}
