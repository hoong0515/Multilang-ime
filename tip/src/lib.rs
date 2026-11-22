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

mod candwin;
mod client;
mod display;

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use client::Client;
use ipc::{Key, PreeditSegment, Request, Response};
use windows::core::{implement, w, IUnknownImpl, Interface, Ref, BOOL, GUID, HRESULT, IUnknown, PCWSTR, Result};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HMODULE, LPARAM, RECT, S_FALSE, S_OK, WPARAM,
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

pub(crate) fn module_handle() -> HMODULE {
    HMODULE(DLL_MODULE.load(Ordering::Relaxed) as *mut c_void)
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

/// 왼쪽 Shift 탭: 눌렀다가 다른 키 없이 이 시간 안에 떼면 변환.
const SHIFT_TAP: Duration = Duration::from_millis(250);
/// 백엔드 실패 뒤 다시 부르기까지 쉬는 시간.
const OFFLINE_BACKOFF: Duration = Duration::from_millis(500);
const VK_SHIFT_RAW: u16 = 0x10;
const VK_LSHIFT_RAW: u16 = 0xA0;
const SCAN_RSHIFT: u32 = 0x36;

/// 왼쪽 Shift인가. 오른쪽 Shift(스캔 코드 0x36)만 아니면 왼쪽으로 본다
/// (자동화 도구 등이 스캔 코드 없이 보낸 Shift도 받기 위해).
/// 임시 진단 로그 (%TEMP%\multilang-tip.log). MULTILANG_TIP_DEBUG 파일이 DLL 옆에 있을 때만.
fn debug_log(what: &str, wparam: WPARAM, lparam: LPARAM) {
    let dll = String::from_utf16_lossy(&module_path());
    let Some(dir) = std::path::Path::new(&dll).parent() else { return };
    if !dir.join("MULTILANG_TIP_DEBUG").exists() {
        return;
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("tip-debug.log")) {
        let _ = writeln!(f, "{what} vk=0x{:02X} lparam=0x{:08X}", wparam.0, lparam.0 as u32);
    }
}

/// 진단 로그 한 줄 (MULTILANG_TIP_DEBUG 파일이 있을 때만).
fn debug_note(what: &str) {
    debug_log(what, WPARAM(0), LPARAM(0));
}

fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

fn is_left_shift(wparam: WPARAM, lparam: LPARAM) -> bool {
    let vk = wparam.0 as u16;
    (vk == VK_SHIFT_RAW || vk == VK_LSHIFT_RAW) && ((lparam.0 as u32 >> 16) & 0xFF) != SCAN_RSHIFT
}

// ---------------------------------------------------------------- 텍스트 서비스
#[derive(Default)]
struct State {
    thread_mgr: Option<ITfThreadMgr>,
    client_id: u32,
    composition: Option<ITfComposition>,
    /// 표시 속성 GUID의 TSF 원자값 (일반, 모호, 후보 선택 중). 처음 쓸 때 등록한다.
    atoms: Option<(u32, u32, u32)>,
    /// 조합 영역의 화면 위치 (후보창을 붙일 곳). 편집 세션에서 갱신한다.
    anchor: Option<RECT>,
    /// 왼쪽 Shift를 누른 시각 (그 뒤 다른 키가 오면 None).
    shift_down: Option<Instant>,
}

#[implement(ITfTextInputProcessorEx, ITfTextInputProcessor, ITfKeyEventSink, ITfCompositionSink, ITfDisplayAttributeProvider)]
struct TextService {
    state: Rc<RefCell<State>>,
    backend: RefCell<Client>,
    /// 지금 조합을 만든 키들. 백엔드와 새로 연결되면(재시작·시간 초과 뒤) 다시 보내 조합을 되살린다.
    journal: RefCell<Vec<Key>>,
    /// 백엔드 상태가 journal과 맞춰진 연결 세대
    synced_gen: Cell<u64>,
    /// 지금 화면에 보이는 조합 문자열과 후보창 여부 (백엔드 응답이 없을 때 이어 붙이기용)
    shown: RefCell<String>,
    window_open: Cell<bool>,
    /// 백엔드가 실패한 직후에는 이 시각까지 부르지 않는다 (키마다 시간 초과를 기다려 앱이 버벅이지 않게)
    offline_until: Cell<Option<Instant>>,
}

impl TextService {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(State::default())),
            backend: RefCell::new(Client::new()),
            journal: RefCell::new(Vec::new()),
            synced_gen: Cell::new(0),
            shown: RefCell::new(String::new()),
            window_open: Cell::new(false),
            offline_until: Cell::new(None),
        }
    }
}

impl TextService_Impl {
    /// 백엔드에 요청한다. 새 연결이면 먼저 지금 조합의 키를 다시 보내 상태를 되살린다.
    /// 연결이 끊겨 있었으면(백엔드 재시작 등) 한 번 다시 연결해 재시도한다. 실패하면 None.
    fn call(&self, req: &Request) -> Option<Response> {
        // 확정에 관계된 키(Space, Enter, Esc, Backspace)와 그 밖의 요청은 쉬는 중에도 시도한다
        let urgent = match req {
            Request::Test { key } | Request::Key { key } => matches!(key.vk, 0x08 | 0x0D | 0x1B | 0x20),
            _ => true,
        };
        if !urgent && self.offline_until.get().is_some_and(|t| Instant::now() < t) {
            return None;
        }
        let r = self.call_inner(req);
        self.offline_until.set(if r.is_none() { Some(Instant::now() + OFFLINE_BACKOFF) } else { None });
        r
    }

    fn call_inner(&self, req: &Request) -> Option<Response> {
        let mut c = self.backend.try_borrow_mut().ok()?;
        for attempt in 0..2 {
            if !c.connect() {
                return None;
            }
            if c.generation != self.synced_gen.get() {
                let keys = self.journal.borrow().clone();
                let replay = (|| -> std::result::Result<(), client::Fail> {
                    c.transact(&Request::Reset)?;
                    for key in keys {
                        c.transact(&Request::Key { key })?;
                    }
                    Ok(())
                })();
                match replay {
                    Ok(()) => self.synced_gen.set(c.generation),
                    Err(f) => {
                        debug_note(&format!("replay failed: {f:?}"));
                        if f == client::Fail::Broken && attempt == 0 {
                            continue;
                        }
                        return None;
                    }
                }
            }
            match c.transact(req) {
                Ok(r) => return Some(r),
                Err(f) => {
                    debug_note(&format!("call failed: {f:?} {req:?}"));
                    if f == client::Fail::Broken && attempt == 0 {
                        continue; // 끊긴 연결: 새로 연결(+되살리기)해서 한 번 더
                    }
                    return None;
                }
            }
        }
        None
    }

    /// 글자 키 응답으로 journal을 갱신한다.
    fn track(&self, key: Key, r: &Response, window_was_open: bool) {
        let mut j = self.journal.borrow_mut();
        if r.commit.is_some() {
            // 확정됐으면 새 조합은 (있다면) 이 키로 시작한 것
            j.clear();
            if !r.preedit.is_empty() {
                j.push(key);
            }
        } else if r.preedit.is_empty() {
            j.clear();
        } else if !window_was_open {
            // 후보창 안의 이동·선택은 되살리지 않는다 (되살릴 때는 자동 판정 결과로)
            j.push(key);
        }
    }

    /// 백엔드가 응답하지 않을 때: 글자는 친 그대로 조합에 이어 붙이고(나중에 되살림), 확정 키는 보이는 그대로 확정.
    /// 키를 먹었으면 true.
    fn offline_key(&self, ctx: &ITfContext, key: Key) -> bool {
        let composing = self.composing();
        let mut r = Response { eaten: true, ..Default::default() };
        if let Some(ch) = ipc::key_char(&key) {
            self.journal.borrow_mut().push(key);
            let text = format!("{}{ch}", self.shown.borrow());
            r.preedit = vec![ipc::PreeditSegment { text, lang: "raw".into(), ambiguous: false, focused: false }];
        } else if !composing {
            return false;
        } else {
            let shown = self.shown.borrow().clone();
            match key.vk {
                0x08 => {
                    self.journal.borrow_mut().pop();
                    let mut t = shown;
                    t.pop();
                    if !t.is_empty() {
                        r.preedit = vec![ipc::PreeditSegment { text: t, lang: "raw".into(), ambiguous: false, focused: false }];
                    }
                }
                0x1B => {}
                0x20 => r.commit = Some(format!("{shown} ")),
                _ => {
                    // 그 밖의 키: 보이는 그대로 확정하고 키는 앱에 넘긴다
                    r.commit = Some(shown);
                    r.eaten = false;
                }
            }
            if r.preedit.is_empty() {
                self.journal.borrow_mut().clear();
            }
        }
        let eaten = r.eaten;
        let _ = self.apply(ctx, &r);
        eaten
    }

    fn atoms(&self) -> Option<(u32, u32, u32)> {
        if let Some(a) = self.state.try_borrow().ok()?.atoms {
            return Some(a);
        }
        let cats: ITfCategoryMgr = unsafe { CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let a = unsafe {
            (
                cats.RegisterGUID(&display::ATTR_INPUT).ok()?,
                cats.RegisterGUID(&display::ATTR_AMBIGUOUS).ok()?,
                cats.RegisterGUID(&display::ATTR_FOCUSED).ok()?,
            )
        };
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
            commit_segments: resp.commit_segments.clone(),
            preedit: resp.preedit_text(),
            segments: resp.preedit.clone(),
            atoms,
        }
        .into();
        let hr = unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE)? };
        *self.shown.borrow_mut() = resp.preedit_text();
        self.window_open.set(resp.candidates.is_some() && !resp.preedit.is_empty());
        // 후보창: 응답에 후보가 있으면 조합 아래에 띄우고, 없으면 숨긴다
        match (&resp.candidates, self.state.try_borrow().ok().and_then(|s| s.anchor)) {
            (Some(list), Some(anchor)) if !resp.preedit.is_empty() => candwin::show(list, anchor),
            _ => candwin::hide(),
        }
        hr.ok()
    }

    fn composing(&self) -> bool {
        self.state.try_borrow().map(|s| s.composition.is_some()).unwrap_or(false)
    }

    fn reset(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.composition = None;
        }
        self.journal.borrow_mut().clear();
        self.shown.borrow_mut().clear();
        self.window_open.set(false);
        candwin::hide();
        let _ = self.call(&Request::Reset);
    }

    /// 왼쪽 Shift 탭인가 (KeyUp에서 확인). 조합 중일 때만 의미가 있다.
    fn shift_tap_pending(&self) -> bool {
        self.state
            .try_borrow()
            .ok()
            .and_then(|s| s.shift_down)
            .is_some_and(|t| t.elapsed() <= SHIFT_TAP)
            && self.composing()
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
        self.journal.borrow_mut().clear();
        self.shown.borrow_mut().clear();
        let _ = self.call(&Request::Reset);
        candwin::hide();
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
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        if !fforeground.as_bool() {
            candwin::hide();
        }
        Ok(())
    }

    /// 키를 먹을지 묻기만 한다 (상태를 바꾸지 않음). TSF는 보통 이 단계 없이 OnKeyDown을 바로 부른다.
    fn OnTestKeyDown(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        debug_log("TestKeyDown", wparam, lparam);
        if is_left_shift(wparam, lparam) {
            return Ok(false.into());
        }
        let Some(t) = self.call(&Request::Test { key: current_key(wparam) }) else {
            return Ok(false.into());
        };
        // flush_first면 OnKeyDown에서 확정한 뒤 키를 앱에 넘긴다
        Ok((t.eaten || t.flush_first).into())
    }

    /// 실제 키 처리. 메모장 실측에서 TSF가 OnTestKeyDown 없이 이것만 부르는 것을 확인했다.
    fn OnKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        debug_log("KeyDown", wparam, lparam);
        // 왼쪽 Shift: 누른 시각만 기록하고 키는 앱에 넘긴다 (탭 판정은 KeyUp에서)
        let repeat = (lparam.0 as u32 >> 30) & 1 == 1;
        if let Ok(mut s) = self.state.try_borrow_mut() {
            if is_left_shift(wparam, lparam) {
                if !repeat {
                    s.shift_down = Some(Instant::now());
                }
                return Ok(false.into());
            }
            s.shift_down = None; // 다른 키가 끼면 탭이 아니다 (Shift+글자 = 대문자)
        }
        let Ok(ctx) = pic.ok() else { return Ok(false.into()) };
        let key = current_key(wparam);
        if is_modifier_vk(key.vk) {
            return Ok(false.into());
        }
        let Some(t) = self.call(&Request::Test { key }) else {
            return Ok(self.offline_key(ctx, key).into());
        };
        if t.flush_first {
            // 조합 중 다른 키(화살표 등): 먼저 확정하고 키는 앱이 처리하게 둔다
            match self.call(&Request::Flush) {
                Some(r) => {
                    self.journal.borrow_mut().clear();
                    let _ = self.apply(ctx, &r);
                }
                None => return Ok(self.offline_key(ctx, key).into()),
            }
            return Ok(false.into());
        }
        if !t.eaten {
            return Ok(false.into());
        }
        let window_was_open = self.window_open.get();
        let Some(r) = self.call(&Request::Key { key }) else {
            return Ok(self.offline_key(ctx, key).into());
        };
        if !r.eaten {
            return Ok(false.into());
        }
        self.track(key, &r, window_was_open);
        self.apply(ctx, &r)?;
        Ok(true.into())
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        debug_log("TestKeyUp", wparam, lparam);
        if !is_left_shift(wparam, lparam) {
            return Ok(false.into());
        }
        let tap = self.shift_tap_pending();
        if !tap {
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.shift_down = None;
            }
        }
        Ok(tap.into())
    }

    fn OnKeyUp(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        debug_log("KeyUp", wparam, lparam);
        if !is_left_shift(wparam, lparam) {
            return Ok(false.into());
        }
        let tap = self.shift_tap_pending();
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.shift_down = None;
        }
        if !tap {
            return Ok(false.into());
        }
        let (Ok(ctx), Some(r)) = (pic.ok(), self.call(&Request::ShiftTap)) else { return Ok(false.into()) };
        self.apply(ctx, &r)?;
        Ok(true.into())
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
    commit_segments: Vec<PreeditSegment>,
    preedit: String,
    segments: Vec<PreeditSegment>,
    atoms: Option<(u32, u32, u32)>,
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

    /// 조합 영역의 화면 위치를 기록한다 (후보창 위치).
    fn remember_anchor(&self, ec: u32, range: &ITfRange) {
        let rect = (|| -> Result<RECT> {
            let view = unsafe { self.context.GetActiveView()? };
            let mut rc = RECT::default();
            let mut clipped = BOOL::default();
            unsafe { view.GetTextExt(ec, range, &mut rc, &mut clipped)? };
            Ok(rc)
        })();
        if let (Ok(rc), Ok(mut s)) = (rect, self.state.try_borrow_mut()) {
            s.anchor = Some(rc);
        }
    }

    /// 조합 범위 안에서 구간마다 밑줄 종류(일반/모호/후보 선택 중)를 표시한다.
    fn mark_segments(&self, ec: u32, range: &ITfRange) {
        let Some((input, ambiguous, focused)) = self.atoms else { return };
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
                let atom = if seg.focused {
                    focused
                } else if seg.ambiguous {
                    ambiguous
                } else {
                    input
                };
                let v = variant_i4(atom as i32);
                unsafe { prop.SetValue(ec, &r, &v) }
            };
            let _ = mark();
            start = end;
        }
    }

    /// 구간마다 언어 속성(GUID_PROP_LANGID)을 붙인다. 앱은 이걸 보고 글꼴을 고른다
    /// (한국어 프로필의 TIP가 넣은 일본어를 한국어 글꼴로 그려 気·ー 등이 깨지는 문제).
    fn mark_langs(&self, ec: u32, range: &ITfRange, segs: &[PreeditSegment]) {
        let Ok(prop) = (unsafe { self.context.GetProperty(&GUID_PROP_LANGID) }) else { return };
        let mut start = 0i32;
        for seg in segs {
            let len = seg.text.encode_utf16().count() as i32;
            let end = start + len;
            let langid = match seg.lang.as_str() {
                "ja" => Some(0x0411),
                "ko" => Some(0x0412),
                "en" => Some(0x0409),
                _ => None,
            };
            if let (Some(id), true) = (langid, len > 0) {
                let mark = || -> Result<()> {
                    let r = unsafe { range.Clone()? };
                    unsafe { r.Collapse(ec, TF_ANCHOR_START)? };
                    let mut moved = 0i32;
                    unsafe { r.ShiftEnd(ec, end, &mut moved, std::ptr::null())? };
                    unsafe { r.ShiftStart(ec, start, &mut moved, std::ptr::null())? };
                    unsafe { prop.SetValue(ec, &r, &variant_i4(id)) }
                };
                if let Err(e) = mark() {
                    debug_log(&format!("langid set failed: {e:?}"), WPARAM(0), LPARAM(0));
                }
            }
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
                    self.mark_langs(ec, &range, &self.commit_segments);
                    self.caret_to_end(ec, &range)?;
                    self.end_composition(ec, &comp)?;
                }
                None if !text.is_empty() => {
                    let ins: ITfInsertAtSelection = self.context.cast()?;
                    let range = unsafe { ins.InsertTextAtSelection(ec, Default::default(), &utf16(text))? };
                    self.mark_langs(ec, &range, &self.commit_segments);
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
            self.mark_langs(ec, &range, &self.segments);
            self.caret_to_end(ec, &range)?;
            self.remember_anchor(ec, &range);
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
