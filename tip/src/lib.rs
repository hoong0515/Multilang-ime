//! TIP (TSF 텍스트 서비스). M2: 키를 백엔드에 보내고, 돌아온 조합·확정을 문서에 쓴다.
//! 조합은 구간마다 밑줄 종류(일반 실선 / 모호 점선)를 달리 표시한다 (display.rs).
//!
//! - OnTestKeyDown: 백엔드에 "이 키를 먹을지"만 묻는다 (상태 변화 없음).
//!   조합 중에 다른 키(구두점 등)가 오면 백엔드가 flush_first를 돌려주고, 여기서 먼저 확정한 뒤 키는 앱에 넘긴다.
//! - OnKeyDown: 백엔드가 키를 처리하고 돌려준 preedit(조합)과 commit(확정)을 동기 편집 세션으로 반영한다.
//! - 백엔드가 없거나 응답이 늦으면 키를 먹지 않는다 (앱은 IME 없이 입력되는 상태).
//!
//! 이 DLL은 모든 앱 프로세스에 로드되므로 패닉이 나면 그 앱이 죽는다.
//! unwrap/expect를 쓰지 않고, RefCell은 try_borrow로만 접근한다.

#![allow(non_snake_case)]

mod client;
mod display;

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};

use client::Client;
use ipc::{Key, PreeditSegment, Request, Response};
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
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, HKL, VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
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

pub(crate) fn module_path() -> Vec<u16> {
    let module = HMODULE(DLL_MODULE.load(Ordering::Relaxed) as *mut c_void);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    buf.truncate(n);
    buf
}

// ---------------------------------------------------------------- 키
fn key_down(vk: u16) -> bool {
    unsafe { (GetKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn current_key(wparam: WPARAM) -> Key {
    Key {
        vk: wparam.0 as u16,
        shift: key_down(VK_SHIFT.0),
        caps: unsafe { (GetKeyState(VK_CAPITAL.0 as i32) & 1) != 0 },
        ctrl: key_down(VK_CONTROL.0) || key_down(VK_LWIN.0) || key_down(VK_RWIN.0),
        alt: key_down(VK_MENU.0),
    }
}

// ---------------------------------------------------------------- 텍스트 서비스
#[derive(Default)]
struct State {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    composition: Option<ITfComposition>,
    /// 표시 속성 GUID의 TSF 원자값 (일반, 모호). 처음 쓸 때 등록한다.
    atoms: Option<(u32, u32)>,
}

#[implement(ITfTextInputProcessorEx, ITfTextInputProcessor, ITfKeyEventSink, ITfCompositionSink, ITfDisplayAttributeProvider)]
struct TextService {
    state: Rc<RefCell<State>>,
    backend: RefCell<Client>,
}

impl TextService {
    fn new() -> Self {
        Self { state: Rc::new(RefCell::new(State::default())), backend: RefCell::new(Client::new()) }
    }
}

impl TextService_Impl {
    fn call(&self, req: &Request) -> Option<Response> {
        self.backend.try_borrow_mut().ok()?.call(req)
    }

    fn atoms(&self) -> Option<(u32, u32)> {
        if let Some(a) = self.state.try_borrow().ok()?.atoms {
            return Some(a);
        }
        let cats: ITfCategoryMgr = unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let a = unsafe { (cats.RegisterGUID(&display::ATTR_INPUT).ok()?, cats.RegisterGUID(&display::ATTR_AMBIGUOUS).ok()?) };
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.atoms = Some(a);
        }
        Some(a)
    }

    /// 동기 편집 세션으로 응답(확정·조합)을 문서에 반영한다 (키 이벤트 콜백 안에서만 호출).
    fn apply(&self, context: &ITfContext, resp: &Response) -> Result<()> {
        let atoms = self.atoms();
        let client_id = self.state.try_borrow().map(|s| s.client_id).map_err(|_| E_FAIL)?;
        let sink: ITfCompositionSink = self.to_interface();
        let session: ITfEditSession = EditSession {
            context: context.clone(),
            state: self.state.clone(),
            sink,
            commit: resp.commit.clone(),
            preedit: resp.preedit_text(),
            segments: resp.preedit.clone(),
            atoms,
        }
        .into();
        let hr = unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE)? };
        hr.ok()
    }

    fn reset(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.composition = None;
        }
        let _ = self.call(&Request::Reset);
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
                (s.thread_mgr.take(), s.client_id)
            }
            Err(_) => return Ok(()),
        };
        let _ = self.call(&Request::Reset);
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
        let Some(t) = self.call(&Request::Test { key: current_key(wparam) }) else {
            return Ok(false.into());
        };
        if t.flush_first {
            // 조합 중 다른 키: 먼저 확정하고 키는 앱이 처리하게 둔다
            if let (Ok(ctx), Some(r)) = (pic.ok(), self.call(&Request::Flush)) {
                let _ = self.apply(ctx, &r);
            }
            return Ok(false.into());
        }
        Ok(t.eaten.into())
    }

    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        let Ok(ctx) = pic.ok() else { return Ok(false.into()) };
        let Some(r) = self.call(&Request::Key { key: current_key(wparam) }) else {
            return Ok(false.into());
        };
        if !r.eaten {
            return Ok(false.into());
        }
        self.apply(ctx, &r)?;
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

impl ITfDisplayAttributeProvider_Impl for TextService_Impl {
    fn EnumDisplayAttributeInfo(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        Ok(display::EnumAttributes::new(0))
    }

    fn GetDisplayAttributeInfo(&self, guid: *const GUID) -> Result<ITfDisplayAttributeInfo> {
        if guid.is_null() {
            return Err(E_POINTER.into());
        }
        display::AttributeInfo::get(unsafe { &*guid })
    }
}

impl ITfCompositionSink_Impl for TextService_Impl {
    /// 앱이 조합을 강제로 끝냈을 때 (클릭으로 커서 이동 등): 표시된 대로 확정된 것으로 보고 백엔드 상태를 비운다.
    fn OnCompositionTerminated(&self, _ecwrite: u32, _pcomposition: Ref<ITfComposition>) -> Result<()> {
        self.reset();
        Ok(())
    }
}

// ---------------------------------------------------------------- 편집 세션
#[implement(ITfEditSession)]
struct EditSession {
    context: ITfContext,
    state: Rc<RefCell<State>>,
    sink: ITfCompositionSink,
    commit: Option<String>,
    preedit: String,
    segments: Vec<PreeditSegment>,
    atoms: Option<(u32, u32)>,
}

fn variant_i4(v: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: v },
            }),
        },
    }
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl EditSession {
    fn composition(&self) -> Option<ITfComposition> {
        self.state.try_borrow().ok()?.composition.clone()
    }

    fn start_composition(&self, ec: u32) -> Result<ITfComposition> {
        let ins: ITfInsertAtSelection = self.context.cast()?;
        let range = unsafe { ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[])? };
        let cc: ITfContextComposition = self.context.cast()?;
        let comp = unsafe { cc.StartComposition(ec, &range, &self.sink)? };
        self.state.try_borrow_mut().map_err(|_| E_FAIL)?.composition = Some(comp.clone());
        Ok(comp)
    }

    fn end_composition(&self, ec: u32, comp: &ITfComposition) -> Result<()> {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.composition = None;
        }
        // 확정된 글에 밑줄이 남지 않게 표시 속성을 지운다
        if let (Ok(prop), Ok(range)) =
            (unsafe { self.context.GetProperty(&GUID_PROP_ATTRIBUTE) }, unsafe { comp.GetRange() })
        {
            let _ = unsafe { prop.Clear(ec, &range) };
        }
        unsafe { comp.EndComposition(ec) }
    }

    /// 조합 범위 안에서 구간마다 밑줄 종류(일반/모호)를 표시한다.
    fn mark_segments(&self, ec: u32, range: &ITfRange) {
        let Some((input, ambiguous)) = self.atoms else { return };
        let Ok(prop) = (unsafe { self.context.GetProperty(&GUID_PROP_ATTRIBUTE) }) else { return };
        let mut start = 0i32;
        for seg in &self.segments {
            let len = seg.text.encode_utf16().count() as i32;
            let end = start + len;
            let mark = || -> Result<()> {
                let r = unsafe { range.Clone()? };
                unsafe { r.Collapse(ec, TF_ANCHOR_START)? };
                let mut moved = 0i32;
                unsafe { r.ShiftEnd(ec, end, &mut moved, std::ptr::null())? };
                unsafe { r.ShiftStart(ec, start, &mut moved, std::ptr::null())? };
                let atom = if seg.ambiguous { ambiguous } else { input };
                let v = variant_i4(atom as i32);
                unsafe { prop.SetValue(ec, &r, &v) }
            };
            let _ = mark();
            start = end;
        }
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
        // 1) 확정: 조합이 있으면 그 자리를 확정 텍스트로 바꾸고 조합을 끝낸다. 없으면 커서 위치에 넣는다.
        if let Some(text) = &self.commit {
            match self.composition() {
                Some(comp) => {
                    let range = unsafe { comp.GetRange()? };
                    unsafe { range.SetText(ec, 0, &utf16(text))? };
                    self.caret_to_end(ec, &range)?;
                    self.end_composition(ec, &comp)?;
                }
                None if !text.is_empty() => {
                    let ins: ITfInsertAtSelection = self.context.cast()?;
                    let range = unsafe { ins.InsertTextAtSelection(ec, Default::default(), &utf16(text))? };
                    self.caret_to_end(ec, &range)?;
                }
                None => {}
            }
        }
        // 2) 조합: 남은 조합이 있으면 표시, 없으면 (확정 없이 끝난 경우) 조합을 지우고 끝낸다.
        if !self.preedit.is_empty() {
            let comp = match self.composition() {
                Some(c) => c,
                None => self.start_composition(ec)?,
            };
            let range = unsafe { comp.GetRange()? };
            unsafe { range.SetText(ec, 0, &utf16(&self.preedit))? };
            self.mark_segments(ec, &range);
            self.caret_to_end(ec, &range)?;
        } else if self.commit.is_none() {
            if let Some(comp) = self.composition() {
                let range = unsafe { comp.GetRange()? };
                unsafe { range.SetText(ec, 0, &[])? };
                self.end_composition(ec, &comp)?;
            }
        }
        Ok(())
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
    // 단순하게 언로드하지 않는다 (객체 수 추적은 이후에)
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

const CATEGORIES: [GUID; 4] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
];

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
