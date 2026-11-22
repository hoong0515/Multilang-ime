//! M1 뼈대 TIP (TSF 텍스트 서비스).
//!
//! - 알파벳 키를 그대로 조합(composition)에 넣는다. 언어 판정·변환은 아직 없음 (M2에서 백엔드 연결).
//! - Backspace: 조합에서 한 글자 지우기 / Space: 확정 + 공백 / Enter: 확정 / Esc: 취소.
//! - 조합 중 다른 키(구두점, 화살표 등)가 오면 먼저 확정하고 그 키는 앱에 넘긴다.
//! - DllRegisterServer: COM 클래스 + 한국어·일본어 언어 프로필 + 카테고리 등록 (관리자 권한 필요).
//!
//! 이 DLL은 모든 앱 프로세스에 로드되므로 패닉이 나면 그 앱이 죽는다.
//! unwrap/expect를 쓰지 않고, RefCell은 try_borrow로만 접근한다.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{implement, w, IUnknownImpl, Interface, Ref, BOOL, GUID, HRESULT, IUnknown, PCWSTR, Result};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HMODULE, LPARAM, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IClassFactory, IClassFactory_Impl, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, HKL, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT,
    VK_SPACE,
};
use windows::Win32::UI::TextServices::*;

// ---------------------------------------------------------------- 식별자
pub const CLSID_TEXT_SERVICE: GUID = GUID::from_u128(0x05c49e35_3b3f_4bc3_8d48_ef1336f9cd4d);
const PROFILE_KO: GUID = GUID::from_u128(0x6e9dbd5f_4a03_40fa_8186_35dedf98e150);
const PROFILE_JA: GUID = GUID::from_u128(0xb21cf121_71fb_4c56_8b91_3bae60cb6e74);
const LANG_KO: u16 = 0x0412;
const LANG_JA: u16 = 0x0411;
const DESCRIPTION: &str = "Multilang IME (dev)";

static DLL_MODULE: AtomicIsize = AtomicIsize::new(0);

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        DLL_MODULE.store(module.0 as isize, Ordering::Relaxed);
    }
    true.into()
}

fn module_path() -> Vec<u16> {
    let module = HMODULE(DLL_MODULE.load(Ordering::Relaxed) as *mut c_void);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    buf.truncate(n);
    buf
}

// ---------------------------------------------------------------- 키 분류
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeyAction {
    Ignore,        // 키를 먹지 않음
    Append(char),  // 조합에 글자 추가
    Backspace,     // 조합에서 한 글자 지우기
    CommitSpace,   // 확정 + 공백
    Commit,        // 확정 (공백 없음)
    Cancel,        // 조합 취소
    CommitAndPass, // 확정한 뒤 키는 앱에 넘김
}

fn key_down(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn key_toggled(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) & 1) != 0 }
}

fn classify(vk: u16, composing: bool) -> KeyAction {
    let ctrl_or_alt = key_down(VK_CONTROL.0) || key_down(VK_MENU.0) || key_down(VK_LWIN.0) || key_down(VK_RWIN.0);
    if (0x41..=0x5A).contains(&vk) && !ctrl_or_alt {
        let upper = key_down(VK_SHIFT.0) ^ key_toggled(VK_CAPITAL.0);
        let c = (vk as u8) as char;
        return KeyAction::Append(if upper { c } else { c.to_ascii_lowercase() });
    }
    if !composing {
        return KeyAction::Ignore;
    }
    match vk {
        v if v == VK_BACK.0 => KeyAction::Backspace,
        v if v == VK_SPACE.0 => KeyAction::CommitSpace,
        v if v == VK_RETURN.0 => KeyAction::Commit,
        v if v == VK_ESCAPE.0 => KeyAction::Cancel,
        // 수식 키 자체(Shift, Ctrl, Alt, CapsLock, Win)는 조합을 건드리지 않는다
        0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5 => KeyAction::Ignore,
        _ => KeyAction::CommitAndPass,
    }
}

// ---------------------------------------------------------------- 텍스트 서비스
#[derive(Default)]
struct State {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    composition: Option<ITfComposition>,
    buffer: String,
}

#[implement(ITfTextInputProcessorEx, ITfTextInputProcessor, ITfKeyEventSink, ITfCompositionSink)]
struct TextService {
    state: Rc<RefCell<State>>,
}

impl TextService {
    fn new() -> Self {
        Self { state: Rc::new(RefCell::new(State::default())) }
    }
}

impl TextService_Impl {
    fn composing(&self) -> bool {
        self.state.try_borrow().map(|s| s.composition.is_some()).unwrap_or(false)
    }

    /// 동기 편집 세션으로 문서를 바꾼다 (키 이벤트 콜백 안에서만 호출).
    fn edit(&self, context: &ITfContext, op: EditOp) -> Result<()> {
        let client_id = self.state.try_borrow().map(|s| s.client_id).map_err(|_| E_FAIL)?;
        let sink: ITfCompositionSink = self.to_interface();
        let session: ITfEditSession =
            EditSession { context: context.clone(), state: self.state.clone(), sink, op }.into();
        let hr = unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE)? };
        hr.ok()
    }
}

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        let (tm, tid) = match self.state.try_borrow_mut() {
            Ok(mut s) => {
                s.composition = None;
                s.buffer.clear();
                (s.thread_mgr.take(), s.client_id)
            }
            Err(_) => return Ok(()),
        };
        if let Some(tm) = tm {
            if let Ok(km) = tm.cast::<ITfKeystrokeMgr>() {
                let _ = unsafe { km.UnadviseKeyEventSink(tid) };
            }
        }
        Ok(())
    }
}

impl ITfTextInputProcessorEx_Impl for TextService_Impl {
    fn ActivateEx(&self, ptim: Ref<ITfThreadMgr>, tid: u32, _dwflags: u32) -> Result<()> {
        let tm = ptim.ok()?.clone();
        {
            let mut s = self.state.try_borrow_mut().map_err(|_| E_FAIL)?;
            s.thread_mgr = Some(tm.clone());
            s.client_id = tid;
        }
        let km: ITfKeystrokeMgr = tm.cast()?;
        let sink: ITfKeyEventSink = self.to_interface();
        unsafe { km.AdviseKeyEventSink(tid, &sink, true) }
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        let action = classify(wparam.0 as u16, self.composing());
        if action == KeyAction::CommitAndPass {
            // 조합 중 다른 키: 여기서 확정하고 키는 앱이 처리하게 둔다
            if let Ok(ctx) = pic.ok() {
                let _ = self.edit(ctx, EditOp::Commit { space: false });
            }
            return Ok(false.into());
        }
        Ok((action != KeyAction::Ignore).into())
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        let action = classify(wparam.0 as u16, self.composing());
        let ctx = match pic.ok() {
            Ok(c) => c,
            Err(_) => return Ok(false.into()),
        };
        let op = match action {
            KeyAction::Ignore | KeyAction::CommitAndPass => return Ok(false.into()),
            KeyAction::Append(c) => {
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.buffer.push(c);
                }
                EditOp::Update
            }
            KeyAction::Backspace => {
                let empty = match self.state.try_borrow_mut() {
                    Ok(mut s) => {
                        s.buffer.pop();
                        s.buffer.is_empty()
                    }
                    Err(_) => false,
                };
                if empty { EditOp::Cancel } else { EditOp::Update }
            }
            KeyAction::CommitSpace => EditOp::Commit { space: true },
            KeyAction::Commit => EditOp::Commit { space: false },
            KeyAction::Cancel => EditOp::Cancel,
        };
        self.edit(ctx, op)?;
        Ok(true.into())
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(false.into())
    }

    fn OnKeyUp(&self, _pic: Ref<ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        Ok(false.into())
    }

    fn OnPreservedKey(&self, _pic: Ref<ITfContext>, _rguid: *const GUID) -> Result<BOOL> {
        Ok(false.into())
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    /// 앱이 조합을 강제로 끝냈을 때 (클릭으로 커서 이동 등): 표시된 대로 확정된 것으로 본다.
    fn OnCompositionTerminated(&self, _ecwrite: u32, _pcomposition: Ref<ITfComposition>) -> Result<()> {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.composition = None;
            s.buffer.clear();
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- 편집 세션
#[derive(Clone, Copy)]
enum EditOp {
    Update,
    Commit { space: bool },
    Cancel,
}

#[implement(ITfEditSession)]
struct EditSession {
    context: ITfContext,
    state: Rc<RefCell<State>>,
    sink: ITfCompositionSink,
    op: EditOp,
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl EditSession {
    fn ensure_composition(&self, ec: u32) -> Result<ITfComposition> {
        if let Some(c) = self.state.try_borrow().map_err(|_| E_FAIL)?.composition.clone() {
            return Ok(c);
        }
        let ins: ITfInsertAtSelection = self.context.cast()?;
        let range = unsafe { ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? };
        let cc: ITfContextComposition = self.context.cast()?;
        let comp = unsafe { cc.StartComposition(ec, &range, &self.sink)? };
        self.state.try_borrow_mut().map_err(|_| E_FAIL)?.composition = Some(comp.clone());
        Ok(comp)
    }

    fn caret_to_end(&self, ec: u32, range: &ITfRange) -> Result<()> {
        let r = unsafe { range.Clone()? };
        unsafe { r.Collapse(ec, TF_ANCHOR_END)? };
        let mut sel = TF_SELECTION {
            range: ManuallyDrop::new(Some(r)),
            style: TF_SELECTIONSTYLE { ase: TF_AE_NONE, fInterimChar: false.into() },
        };
        let res = unsafe { self.context.SetSelection(ec, std::slice::from_ref(&sel)) };
        unsafe { ManuallyDrop::drop(&mut sel.range) };
        res
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        match self.op {
            EditOp::Update => {
                let text = self.state.try_borrow().map_err(|_| E_FAIL)?.buffer.clone();
                let comp = self.ensure_composition(ec)?;
                let range = unsafe { comp.GetRange()? };
                unsafe { range.SetText(ec, 0, &utf16(&text))? };
                self.caret_to_end(ec, &range)
            }
            EditOp::Commit { space } => {
                let (comp, mut text) = {
                    let mut s = self.state.try_borrow_mut().map_err(|_| E_FAIL)?;
                    (s.composition.take(), std::mem::take(&mut s.buffer))
                };
                let Some(comp) = comp else { return Ok(()) };
                if space {
                    text.push(' ');
                }
                let range = unsafe { comp.GetRange()? };
                unsafe { range.SetText(ec, 0, &utf16(&text))? };
                self.caret_to_end(ec, &range)?;
                unsafe { comp.EndComposition(ec) }
            }
            EditOp::Cancel => {
                let comp = {
                    let mut s = self.state.try_borrow_mut().map_err(|_| E_FAIL)?;
                    s.buffer.clear();
                    s.composition.take()
                };
                let Some(comp) = comp else { return Ok(()) };
                let range = unsafe { comp.GetRange()? };
                unsafe { range.SetText(ec, 0, &[])? };
                unsafe { comp.EndComposition(ec) }
            }
        }
    }
}

// ---------------------------------------------------------------- 클래스 팩토리
#[implement(IClassFactory)]
struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> Result<()> {
        if object.is_null() || iid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let unknown: IUnknown = TextService::new().into();
        unsafe { unknown.query(iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe { *ppv = std::ptr::null_mut() };
    if unsafe { *rclsid } != CLSID_TEXT_SERVICE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = ClassFactory.into();
    unsafe { factory.query(riid, ppv) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    // M1: 단순하게 언로드하지 않는다 (객체 수 추적은 이후에)
    S_FALSE
}

// ---------------------------------------------------------------- 등록
fn guid_string(g: &GUID) -> String {
    format!("{{{:?}}}", g)
}

fn set_reg_sz(key: HKEY, name: PCWSTR, value: &[u16]) -> Result<()> {
    let mut data: Vec<u16> = value.to_vec();
    data.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
    unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(bytes)).ok() }
}

fn register_com_class() -> Result<()> {
    let path = format!("SOFTWARE\\Classes\\CLSID\\{}\\InprocServer32\0", guid_string(&CLSID_TEXT_SERVICE));
    let sub = utf16(&path);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(sub.as_ptr()), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE,
                        None, &mut key, None)
            .ok()?;
    }
    let res = set_reg_sz(key, PCWSTR::null(), &module_path())
        .and_then(|_| set_reg_sz(key, w!("ThreadingModel"), &utf16("Apartment")));
    unsafe {
        let _ = RegCloseKey(key);
    }
    res
}

fn unregister_com_class() {
    let path = format!("SOFTWARE\\Classes\\CLSID\\{}\0", guid_string(&CLSID_TEXT_SERVICE));
    let sub = utf16(&path);
    unsafe {
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(sub.as_ptr()));
    }
}

const CATEGORIES: [GUID; 3] =
    [GUID_TFCAT_TIP_KEYBOARD, GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT, GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT];

fn register_tsf() -> Result<()> {
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)? };
    let desc = utf16(DESCRIPTION);
    let icon = module_path();
    for (lang, profile) in [(LANG_KO, PROFILE_KO), (LANG_JA, PROFILE_JA)] {
        unsafe {
            profiles.RegisterProfile(&CLSID_TEXT_SERVICE, lang, &profile, &desc, &icon, 0, HKL::default(), 0, true, 0)?
        };
    }
    let cats: ITfCategoryMgr = unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)? };
    for cat in CATEGORIES {
        unsafe { cats.RegisterCategory(&CLSID_TEXT_SERVICE, &cat, &CLSID_TEXT_SERVICE)? };
    }
    Ok(())
}

fn unregister_tsf() {
    unsafe {
        if let Ok(profiles) = CoCreateInstance::<_, ITfInputProcessorProfileMgr>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        ) {
            for (lang, profile) in [(LANG_KO, PROFILE_KO), (LANG_JA, PROFILE_JA)] {
                let _ = profiles.UnregisterProfile(&CLSID_TEXT_SERVICE, lang, &profile, 0);
            }
        }
        if let Ok(cats) = CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
            for cat in CATEGORIES {
                let _ = cats.UnregisterCategory(&CLSID_TEXT_SERVICE, &cat, &CLSID_TEXT_SERVICE);
            }
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllRegisterServer() -> HRESULT {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    match register_com_class().and_then(|_| register_tsf()) {
        Ok(()) => S_OK,
        Err(e) => {
            unregister_tsf();
            unregister_com_class();
            e.code()
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllUnregisterServer() -> HRESULT {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    unregister_tsf();
    unregister_com_class();
    S_OK
}
