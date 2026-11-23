//! 작업 표시줄 입력 표시기에 보이는 입력 모드 단추 (GUID_LBI_INPUTMODE).
//!
//! TSF TIP가 이 단추를 주지 않으면 Windows는 언어 기본 표시("한글" 등)를 그린다. 우리는 한·영·일을
//! 자동으로 오가므로 언어 하나로 보이지 않게, 앱 아이콘(가·あ)과 이름을 보인다. 누르면 설정 화면을 연다.

use std::cell::RefCell;

use windows::core::{implement, w, BOOL, BSTR, GUID, Interface, PCWSTR, Ref, Result};
use windows::core::HRESULT;
use windows::Win32::Foundation::{E_INVALIDARG, POINT, RECT};
use windows::Win32::Graphics::Gdi::HBITMAP;

const CONNECT_E_ADVISELIMIT: HRESULT = HRESULT(0x8004_0201_u32 as i32);
const CONNECT_E_CANNOTCONNECT: HRESULT = HRESULT(0x8004_0202_u32 as i32);
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXSMICON, SM_CYSMICON, SW_SHOWNORMAL,
};

const TOOLTIP: &str = "Multilang IME (한·영·일 자동 전환)";
const MENU_SETTINGS: u32 = 1;

#[implement(ITfLangBarItemButton, ITfLangBarItem, ITfSource)]
pub struct InputModeButton {
    sink: RefCell<Option<ITfLangBarItemSink>>,
}

impl InputModeButton {
    pub fn new() -> Self {
        Self { sink: RefCell::new(None) }
    }
}

/// 설정 화면 실행 파일: DLL 옆, 없으면 한 단계 위 (32비트 DLL은 x86 폴더에 있다).
fn open_settings() {
    let dll = String::from_utf16_lossy(&crate::module_path());
    let Some(dir) = std::path::Path::new(dll.trim_end_matches('\0')).parent() else { return };
    let exe = [dir.join("MultilangIME-Settings.exe"), dir.join("..").join("MultilangIME-Settings.exe")]
        .into_iter()
        .find(|p| p.exists());
    if let Some(exe) = exe {
        let w: Vec<u16> = exe.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }
}

impl ITfLangBarItem_Impl for InputModeButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        if pinfo.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let mut desc = [0u16; 32];
        for (i, c) in "Multilang IME".encode_utf16().take(31).enumerate() {
            desc[i] = c;
        }
        unsafe {
            *pinfo = TF_LANGBARITEMINFO {
                clsidService: crate::CLSID_TEXT_SERVICE,
                guidItem: GUID_LBI_INPUTMODE,
                dwStyle: TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY,
                ulSort: 0,
                szDescription: desc,
            };
        }
        Ok(())
    }
    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }
    fn Show(&self, _fshow: BOOL) -> Result<()> {
        Ok(())
    }
    fn GetTooltipString(&self) -> Result<BSTR> {
        Ok(BSTR::from(TOOLTIP))
    }
}

impl ITfLangBarItemButton_Impl for InputModeButton_Impl {
    fn OnClick(&self, click: TfLBIClick, _pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        if click == TF_LBI_CLK_LEFT {
            open_settings();
        }
        Ok(())
    }
    fn InitMenu(&self, pmenu: Ref<ITfMenu>) -> Result<()> {
        if let Ok(menu) = pmenu.ok() {
            let text: Vec<u16> = "Multilang IME 설정".encode_utf16().collect();
            unsafe { menu.AddMenuItem(MENU_SETTINGS, 0, HBITMAP::default(), HBITMAP::default(), &text, std::ptr::null_mut())? };
        }
        Ok(())
    }
    fn OnMenuSelect(&self, wid: u32) -> Result<()> {
        if wid == MENU_SETTINGS {
            open_settings();
        }
        Ok(())
    }
    fn GetIcon(&self) -> Result<HICON> {
        // 아이콘 리소스 1번 (res/multilang.ico). 받는 쪽이 지운다.
        let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
        let h = unsafe { LoadImageW(Some(crate::module_handle().into()), PCWSTR(1 as *const u16), IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR)? };
        Ok(HICON(h.0))
    }
    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from("Multilang"))
    }
}

impl ITfSource_Impl for InputModeButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<windows::core::IUnknown>) -> Result<u32> {
        if riid.is_null() || unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(CONNECT_E_CANNOTCONNECT.into());
        }
        let mut s = self.sink.borrow_mut();
        if s.is_some() {
            return Err(CONNECT_E_ADVISELIMIT.into());
        }
        *s = Some(punk.ok()?.cast()?);
        Ok(1)
    }
    fn UnadviseSink(&self, _dwcookie: u32) -> Result<()> {
        *self.sink.borrow_mut() = None;
        Ok(())
    }
}
