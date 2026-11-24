//! 백엔드 파이프의 보안 설정.
//!
//! 작업 표시줄 검색창 같은 샌드박스(AppContainer) 앱 안의 TIP도 접속할 수 있게 한다.
//! - DACL: SYSTEM, 이 사용자 계정, ALL APPLICATION PACKAGES(AC), ALL RESTRICTED APPLICATION PACKAGES
//!   (샌드박스 앱은 사용자 확인과 샌드박스 확인을 둘 다 통과해야 하므로 다른 사용자의 앱은 들어올 수 없다)
//! - SACL: 무결성 레이블 Low (샌드박스 앱은 Low 무결성이라, 기본(Medium) 레이블이면 쓰기가 막힌다)

use windows::core::PCWSTR;
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

/// 파이프 생성에 쓸 보안 특성. 만들지 못하면 None (기본 보안으로 만든다).
/// 돌려준 SECURITY_ATTRIBUTES가 가리키는 보안 설명자는 프로세스가 끝날 때까지 쓰므로 해제하지 않는다.
pub fn pipe_security() -> Option<SECURITY_ATTRIBUTES> {
    let sid = ipc::win::user_sid()?;
    let sddl = format!("D:(A;;GA;;;SY)(A;;GA;;;{sid})(A;;GRGW;;;AC)(A;;GRGW;;;S-1-15-2-2)S:(ML;;NW;;;LW)");
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(wide.as_ptr()), SDDL_REVISION_1, &mut sd, None) }.ok()?;
    Some(SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    })
}
