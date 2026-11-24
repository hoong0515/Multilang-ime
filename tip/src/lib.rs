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
mod host;
mod langbar;
mod uielement;

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
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IClassFactory, IClassFactory_Impl, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::System::Variant::{VariantClear, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4, VT_UNKNOWN};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, HKL, VK_CAPITAL, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::TextServices::*;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetParent, GetWindowLongW, GWL_STYLE};

// ---------------------------------------------------------------- 식별자
pub const CLSID_TEXT_SERVICE: GUID = GUID::from_u128(0x05c49e35_3b3f_4bc3_8d48_ef1336f9cd4d);
const PROFILE_KO: GUID = GUID::from_u128(0x6e9dbd5f_4a03_40fa_8186_35dedf98e150);
const PROFILE_JA: GUID = GUID::from_u128(0xb21cf121_71fb_4c56_8b91_3bae60cb6e74);
const LANG_KO: u16 = 0x0412;
const LANG_JA: u16 = 0x0411;
const PROFILE_EN: GUID = GUID::from_u128(0x681b9e1a_408f_4af6_8eb3_af2c9283a82f);
const LANG_EN: u16 = 0x0409;
/// 등록하는 언어 프로필. 어느 언어 밑에 보일지는 설치할 때 고른다 (installer/install.ps1 -Language):
/// 고른 언어만 켜고 입력 방법 목록에 넣는다. 작업 표시줄 언어 표시와 앱의 글꼴 선택이 이 언어를 따른다.
const PROFILES: [(u16, GUID); 3] = [(LANG_KO, PROFILE_KO), (LANG_JA, PROFILE_JA), (LANG_EN, PROFILE_EN)];
const DESCRIPTION: &str = "Multilang IME";

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

/// stub(`multilang_tip.dll`, tipstub)이 이 DLL을 읽은 뒤 알려 준 자기 경로.
static HOST_PATH: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();

/// stub이 부른다: 등록·백엔드·설정 화면 위치의 기준이 될 stub 경로 (끝의 NUL 포함 가능).
#[unsafe(no_mangle)]
extern "system" fn MultilangSetHost(path: PCWSTR) {
    if path.is_null() {
        return;
    }
    if let Ok(s) = unsafe { path.to_string() } {
        let _ = HOST_PATH.set(s.encode_utf16().collect());
    }
}

/// 등록되는 DLL의 경로: stub을 거쳐 읽혔으면 stub, 아니면 이 DLL (개발 때 impl을 직접 등록한 경우).
/// impl은 `impl\<해시>\` 폴더에 있으므로 설치 폴더 기준 파일(backend.exe 등)은 이 경로로 찾는다.
pub(crate) fn host_path() -> Vec<u16> {
    HOST_PATH.get().cloned().unwrap_or_else(module_path)
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
/// 오른쪽 Alt 단독 탭으로 볼 최대 누름 시간 (길게 누르고 있다 떼면 Alt 조합을 하려던 것으로 본다)
const RALT_TAP: Duration = Duration::from_millis(600);
/// 백엔드 실패 뒤 다시 부르기까지 쉬는 시간.
const OFFLINE_BACKOFF: Duration = Duration::from_millis(500);
const VK_SHIFT_RAW: u16 = 0x10;
const VK_LSHIFT_RAW: u16 = 0xA0;
const SCAN_RSHIFT: u32 = 0x36;

/// 왼쪽 Shift인가. 오른쪽 Shift(스캔 코드 0x36)만 아니면 왼쪽으로 본다
/// (자동화 도구 등이 스캔 코드 없이 보낸 Shift도 받기 위해).
/// 임시 진단 로그 (%TEMP%\multilang-tip.log). MULTILANG_TIP_DEBUG 파일이 DLL 옆에 있을 때만.
fn debug_log(what: &str, wparam: WPARAM, lparam: LPARAM) {
    let Some(dir) = debug_dir() else { return };
    use std::io::Write as _;
    // logs 폴더가 있으면 거기에 (샌드박스 앱도 쓸 수 있게 권한을 준 폴더), 프로세스 이름도 함께
    let logs = dir.join("logs");
    let path = if logs.is_dir() { logs.join("tip-debug.log") } else { dir.join("tip-debug.log") };
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{exe}] {what} vk=0x{:02X} lparam=0x{:08X}", wparam.0, lparam.0 as u32);
    }
}

/// 진단 로그 한 줄 (MULTILANG_TIP_DEBUG 파일이 있을 때만).
/// 이 프로세스의 실행 파일 이름 (예: notepad.exe).
pub(crate) fn app_name() -> String {
    std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

pub(crate) fn debug_note(what: &str) {
    debug_log(what, WPARAM(0), LPARAM(0));
}

/// 한/영 키
const VK_HANGUL: u16 = 0x15;

fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// 오른쪽 Alt (확장 키 표시가 붙은 Alt). 한/영 키가 없는 키보드에서 언어 고정 키로 쓴다 (단독 탭만).
fn is_right_alt(wparam: WPARAM, lparam: LPARAM) -> bool {
    let vk = wparam.0 as u16;
    (vk == 0x12 || vk == 0xA5) && (lparam.0 as u32 >> 24) & 1 == 1
}

/// AltGr 배열(독일어 등)에서는 오른쪽 Alt가 Ctrl+Alt로 들어와 글자를 친다: 그때는 언어 고정에 쓰지 않는다.
fn altgr_layout_active() -> bool {
    key_down(0xA2) // VK_LCONTROL: Windows가 AltGr에 덧붙이는 왼쪽 Ctrl
}

/// 일본어 배열의 半角/全角 키 → 한/영 키와 같은 언어 고정 순환.
/// VK_OEM_AUTO/VK_OEM_ENLW(0xF3/0xF4)는 일본어 배열에만 있다. VK_KANJI(0x19)는 한국어 배열의 한자 키와 같은 값이라
/// 지금 키보드 배열이 일본어일 때만 받는다.
fn is_ja_lock_key(wparam: WPARAM) -> bool {
    let vk = wparam.0 as u16;
    if vk == 0xF3 || vk == 0xF4 {
        return true;
    }
    if vk == 0x19 {
        let hkl = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout(0) };
        return (hkl.0 as usize & 0x3FF) == 0x11; // LANG_JAPANESE
    }
    false
}

/// 키 이벤트를 백엔드에 보낼 키로: 일본어 배열의 半角/全角은 한/영 키로 바꾼다.
fn request_key(wparam: WPARAM) -> Key {
    let mut key = current_key(wparam);
    if is_ja_lock_key(wparam) {
        key.vk = VK_HANGUL;
        key.alt = false;
        key.ctrl = false;
    }
    key
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
    /// 오른쪽 Alt를 누른 시각 (단독 탭 판정, 다른 키가 끼면 None)
    ralt_down: Option<Instant>,
    /// 조합 창 모드의 조합 문자열과 그 문맥 (옛 입력칸: 조합을 입력칸에 넣지 않는다)
    floating: Option<(String, usize)>,
}

#[implement(ITfTextInputProcessorEx, ITfTextInputProcessor, ITfKeyEventSink, ITfCompositionSink, ITfDisplayAttributeProvider, ITfThreadMgrEventSink)]
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
    /// 후보 목록 UI 요소 (TSF에 알린 것): 내용, 객체, 요소 id, 우리 창으로 그릴지
    ui_data: Rc<RefCell<uielement::Data>>,
    ui_elem: RefCell<Option<ITfCandidateListUIElement>>,
    ui_id: Cell<Option<u32>>,
    ui_own: Cell<bool>,
    /// 작업 표시줄 입력 표시기의 입력 모드 단추 (우리 아이콘)
    langbar: RefCell<Option<ITfLangBarItem>>,
    indicator: Rc<RefCell<langbar::Shared>>,
    /// 입력칸(문서) 포커스 알림 구독 (ITfThreadMgrEventSink) 쿠키
    tm_sink_cookie: Cell<Option<u32>>,
    /// 마지막으로 입력칸 진입 알림을 보인 시각 (연달아 보이지 않게)
    last_focus_notice: Cell<Option<Instant>>,
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
            ui_data: Rc::new(RefCell::new(uielement::Data::default())),
            ui_elem: RefCell::new(None),
            ui_id: Cell::new(None),
            ui_own: Cell::new(true),
            langbar: RefCell::new(None),
            indicator: Rc::new(RefCell::new(langbar::Shared::default())),
            tm_sink_cookie: Cell::new(None),
            last_focus_notice: Cell::new(None),
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
        if let Some(ind) = r.as_ref().and_then(|r| r.indicator.as_ref()) {
            langbar::update(&self.indicator, ind);
        }
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
                    // 새 연결: 프로그램 이름을 알리고(앱별 자동 꺼짐) 지금 조합의 키를 되살린다
                    c.transact(&Request::Hello { app: app_name() })?;
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
        let floating = legacy_floating(context);
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
            floating,
        }
        .into();
        let hr = match unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READWRITE) } {
            Ok(hr) => hr,
            Err(e) => {
                debug_note(&format!("edit session failed {e:?}"));
                return Err(e);
            }
        };
        debug_note(&format!("edit session hr {hr:?} commit={:?} preedit={:?}", resp.commit, resp.preedit_text()));
        *self.shown.borrow_mut() = resp.preedit_text();
        self.window_open.set(resp.candidates.is_some() && !resp.preedit.is_empty());
        let anchor = self.state.try_borrow().ok().and_then(|s| s.anchor);
        if resp.candidates.is_some() {
            debug_note(&format!("candidates: floating={floating} anchor={:?}", anchor.map(|a| (a.left, a.top, a.right, a.bottom))));
        }
        if floating {
            // 조합 창 모드: 조합 문자열과 후보를 커서 아래 창에 보인다 (부분 변환 중이면 고치는 부분을 [ ]로)
            let text = resp.preedit_text();
            let mut anchor = anchor;
            if juce::is_juce_class(&focus_class()) {
                if resp.commit.as_ref().is_some_and(|c| !c.is_empty()) {
                    juce::note_commit();
                }
                let focus = unsafe { GetFocus() };
                let mut window = RECT::default();
                let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(focus, &mut window) };
                let chosen = juce::anchor(anchor, window);
                if chosen != anchor {
                    debug_note(&format!("juce anchor {:?} -> {:?}", anchor.map(|a| (a.left, a.top)), chosen.map(|a| (a.left, a.top))));
                }
                anchor = chosen;
            } else {
                // 커서 위치를 알려 주지 않는 앱(게임 등): Windows가 화면 구석을 돌려준다. 창 밖이면 고른 자리로.
                juce::ensure_hook();
                let focus = unsafe { GetFocus() };
                let mut window = RECT::default();
                if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(focus, &mut window) }.is_ok() {
                    let chosen = juce::fallback(anchor, window);
                    if chosen != anchor {
                        debug_note(&format!("fallback anchor {:?} -> {:?}", anchor.map(|a| (a.left, a.top)), chosen.map(|a| (a.left, a.top))));
                    }
                    anchor = chosen;
                }
            }
            let shown_text = if resp.preedit.len() > 1 && resp.preedit.iter().any(|p| p.focused) {
                resp.preedit.iter().map(|p| if p.focused { format!("[{}]", p.text) } else { p.text.clone() }).collect::<String>()
            } else {
                text.clone()
            };
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.floating = if text.is_empty() { None } else { Some((text.clone(), context.as_raw() as usize)) };
            }
            let own = match (&resp.candidates, text.is_empty()) {
                (Some(list), false) => self.ui_update(context, list),
                _ => {
                    self.ui_end();
                    true
                }
            };
            let list = if own { resp.candidates.as_ref() } else { None };
            floating_diag(anchor);
            match anchor {
                Some(a) if !text.is_empty() => candwin::show_floating(&shown_text, list, a),
                _ => candwin::hide(),
            }
            return hr.ok();
        }
        // 후보창: 응답에 후보가 있으면 UI 요소로 알리고, 앱이 그리지 않으면 조합 아래에 우리 창을 띄운다
        match (&resp.candidates, anchor) {
            (Some(list), Some(anchor)) if !resp.preedit.is_empty() => {
                if self.ui_update(context, list) {
                    candwin::show(list, anchor);
                } else {
                    candwin::hide();
                }
            }
            _ => {
                self.ui_end();
                candwin::hide();
            }
        }
        hr.ok()
    }

    /// 후보 목록을 TSF UI 요소로 알리거나 갱신한다. 우리 창으로 그려야 하면 true (앱이 그리겠다고 하면 false).
    fn ui_update(&self, context: &ITfContext, list: &ipc::CandidateList) -> bool {
        let Some(tm) = self.state.try_borrow().ok().and_then(|s| s.thread_mgr.clone()) else { return true };
        let Ok(mgr) = tm.cast::<ITfUIElementMgr>() else { return true };
        if let Ok(mut d) = self.ui_data.try_borrow_mut() {
            let changed_items = d.items != list.items;
            d.items = list.items.clone();
            d.selected = list.selected as u32;
            d.docmgr = unsafe { context.GetDocumentMgr() }.ok();
            d.flags |= TF_CLUIE_SELECTION | TF_CLUIE_DOCUMENTMGR;
            if changed_items {
                d.flags |= TF_CLUIE_COUNT | TF_CLUIE_STRING | TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE;
            }
        }
        if let Some(id) = self.ui_id.get() {
            let _ = unsafe { mgr.UpdateUIElement(id) };
            return self.ui_own.get();
        }
        let elem: ITfCandidateListUIElement = uielement::CandidateUi { data: self.ui_data.clone() }.into();
        let mut show = BOOL::from(true);
        let mut id = 0u32;
        match unsafe { mgr.BeginUIElement(&elem, &mut show, &mut id) } {
            Ok(()) => {
                debug_note(&format!("uielement begin id={id} show={}", show.as_bool()));
                self.ui_id.set(Some(id));
                self.ui_own.set(show.as_bool());
                if let Ok(mut d) = self.ui_data.try_borrow_mut() {
                    d.shown = show.as_bool();
                }
                *self.ui_elem.borrow_mut() = Some(elem);
                show.as_bool()
            }
            Err(e) => {
                debug_note(&format!("uielement begin failed {e:?}"));
                true
            }
        }
    }

    /// 후보 목록 UI 요소를 닫는다.
    fn ui_end(&self) {
        if let Some(id) = self.ui_id.take() {
            if let Some(tm) = self.state.try_borrow().ok().and_then(|s| s.thread_mgr.clone()) {
                if let Ok(mgr) = tm.cast::<ITfUIElementMgr>() {
                    let _ = unsafe { mgr.EndUIElement(id) };
                }
            }
            debug_note("uielement end");
        }
        *self.ui_elem.borrow_mut() = None;
        self.ui_own.set(true);
    }

    /// 한/영 키 뒤: 바뀐 입력 언어 고정 상태를 커서 아래에 잠깐 보인다.
    fn toast_lock(&self, ctx: &ITfContext, r: &Response) {
        let Some(ind) = &r.indicator else { return };
        let (text, badge) = lock_label(ind);
        let Ok(client_id) = self.state.try_borrow().map(|s| s.client_id) else { return };
        let out = Rc::new(Cell::new(None));
        let session: ITfEditSession = CaretSession { context: ctx.clone(), out: out.clone() }.into();
        let _ = unsafe { ctx.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READ) };
        // 커서 위치를 못 읽으면 마지막 조합 위치, 그것도 없으면 마우스 위치
        let anchor = out.take().or_else(|| self.state.try_borrow().ok().and_then(|s| s.anchor)).unwrap_or_else(|| {
            let mut p = windows::Win32::Foundation::POINT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
            }
            RECT { left: p.x, top: p.y, right: p.x + 1, bottom: p.y + 16 }
        });
        candwin::show_toast(&text, badge, anchor);
    }

    /// 입력칸에 들어왔을 때: 언어를 고정해 쓰는 중이면 그 상태를 커서 옆에 잠깐 보인다 (설정 `focus_notice`).
    /// 자동 전환일 때, 꺼진 칸·프로그램, 조합 중에는 보이지 않는다. 커서 위치를 모르면 보이지 않는다.
    fn focus_notice(&self, ctx: &ITfContext) {
        if self.composing() || self.quick_off(ctx) {
            let _ = self.call(&Request::Status);
            return;
        }
        let Some(r) = self.call(&Request::Status) else { return };
        if self.last_focus_notice.get().is_some_and(|t| t.elapsed() < FOCUS_NOTICE_GAP) {
            return;
        }
        let Some(ind) = r.indicator.as_ref().filter(|i| i.locked && r.focus_notice) else { return };
        let (text, badge) = lock_label(ind);
        let Ok(client_id) = self.state.try_borrow().map(|s| s.client_id) else { return };
        self.last_focus_notice.set(Some(Instant::now()));
        // 포커스 알림 안에서는 동기 편집 세션이 안 될 수 있다: 비동기로 요청하고 세션 안에서 보인다
        let session: ITfEditSession = FocusToastSession { context: ctx.clone(), text, badge }.into();
        let _ = unsafe { ctx.RequestEditSession(client_id, &session, TF_ES_ASYNCDONTCARE | TF_ES_READ) };
    }

    /// 스레드가 지금 포커스를 둔 입력칸 문맥.
    fn focused_context(&self) -> Option<ITfContext> {
        let tm = self.state.try_borrow().ok()?.thread_mgr.clone()?;
        let dim = unsafe { tm.GetFocus() }.ok()?;
        unsafe { dim.GetTop() }.ok()
    }

    /// 지금 입력칸에서 IME를 꺼야 하는가: 앱이 키보드를 꺼 둔 문맥, 빈 문맥, 또는 비밀번호·숫자 등 용도의 칸.
    fn input_off(&self, context: &ITfContext) -> bool {
        if self.quick_off(context) {
            return true;
        }
        let Ok(client_id) = self.state.try_borrow().map(|s| s.client_id) else { return false };
        let out = Rc::new(Cell::new(None));
        let session: ITfEditSession = ScopeSession { context: context.clone(), out: out.clone() }.into();
        match unsafe { context.RequestEditSession(client_id, &session, TF_ES_SYNC | TF_ES_READ) } {
            Ok(hr) if hr.is_err() => debug_note(&format!("scope session hr {hr:?}")),
            Err(e) => {
                debug_note(&format!("scope session failed {e:?}"));
                return false;
            }
            _ => {}
        }
        let scopes = out.take().unwrap_or_default();
        let off = scopes_off(&scopes);
        if off || !scopes.is_empty() {
            debug_note(&format!("input scopes {scopes:?} -> off={off}"));
        }
        off
    }

    /// 편집 세션 없이 알 수 있는 꺼짐: 앱이 키보드를 꺼 둔 문맥, 빈 문맥, 옛 입력칸의 비밀번호·숫자 스타일.
    fn quick_off(&self, context: &ITfContext) -> bool {
        if compartment_on(context, &GUID_COMPARTMENT_KEYBOARD_DISABLED) || compartment_on(context, &GUID_COMPARTMENT_EMPTYCONTEXT) {
            debug_note("input off: disabled/empty context");
            return true;
        }
        // 옛 Win32 입력칸(CUAS 경유)은 용도 속성을 읽을 수 없어 창 스타일로 본다: 숫자 전용, 비밀번호
        let hwnd = unsafe { GetFocus() };
        if !hwnd.0.is_null() {
            let mut buf = [0u16; 64];
            let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
            let class = String::from_utf16_lossy(&buf[..n]).to_ascii_lowercase();
            if class == "edit" || class.starts_with("windowsforms10.edit") {
                const ES_PASSWORD: u32 = 0x0020;
                const ES_NUMBER: u32 = 0x2000;
                let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
                if style & (ES_PASSWORD | ES_NUMBER) != 0 {
                    debug_note("input off: edit style");
                    return true;
                }
            }
        }
        false
    }

    fn composing(&self) -> bool {
        self.state.try_borrow().map(|s| s.composition.is_some() || s.floating.is_some()).unwrap_or(false)
    }

    fn reset(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.composition = None;
            s.floating = None;
        }
        self.journal.borrow_mut().clear();
        self.shown.borrow_mut().clear();
        self.window_open.set(false);
        self.ui_end();
        candwin::hide();
        let _ = self.call(&Request::Reset);
    }

    /// 키 누름에서 오른쪽 Alt 탭 상태를 갱신한다. 오른쪽 Alt면 Some(먹을지 = false), 아니면 None (다른 키가 끼면 탭 취소).
    fn note_right_alt_down(&self, wparam: WPARAM, lparam: LPARAM, repeat: bool) -> Option<bool> {
        let Ok(mut s) = self.state.try_borrow_mut() else { return None };
        if is_right_alt(wparam, lparam) {
            if !repeat {
                s.ralt_down = if altgr_layout_active() { None } else { Some(Instant::now()) };
            }
            return Some(false); // 누름은 앱에 넘긴다 (Alt 조합이 그대로 동작하게)
        }
        s.ralt_down = None;
        None
    }

    /// 오른쪽 Alt를 혼자 짧게 눌렀다 떼는 중인가.
    fn right_alt_tap_pending(&self) -> bool {
        self.state.try_borrow().ok().and_then(|s| s.ralt_down).is_some_and(|t| t.elapsed() <= RALT_TAP)
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
        juce::remove_hook();
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
            if let (Ok(lbm), Some(item)) = (tm.cast::<ITfLangBarItemMgr>(), self.langbar.borrow_mut().take()) {
                let _ = unsafe { lbm.RemoveItem(&item) };
            }
            if let (Ok(src), Some(cookie)) = (tm.cast::<ITfSource>(), self.tm_sink_cookie.take()) {
                let _ = unsafe { src.UnadviseSink(cookie) };
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
        // IME 켜짐 + 입력 모드(고유 문자) 표시. 옛 입력칸(CUAS)은 이 표시가 꺼져 있으면 IME가 꺼진 것으로 보고
        // 조합을 키마다 끝내 버린다 (MS 한국어 IME도 한글 모드에서 켠다).
        set_thread_compartment(&tm, tid, &GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, 1);
        set_thread_compartment(&tm, tid, &GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION, TF_CONVERSIONMODE_NATIVE as i32);
        // 입력 표시기: 언어 기본 표시("한글") 대신 우리 아이콘
        if let Ok(lbm) = tm.cast::<ITfLangBarItemMgr>() {
            let item: ITfLangBarItem = langbar::InputModeButton::new(self.indicator.clone()).into();
            match unsafe { lbm.AddItem(&item) } {
                Ok(()) => *self.langbar.borrow_mut() = Some(item),
                Err(e) => debug_note(&format!("langbar add failed {e:?}")),
            }
        }
        let _ = self.call(&Request::Status); // 입력 표시기 첫 상태
        // 입력칸(문서) 포커스가 바뀌는 것을 받는다 (입력칸 진입 알림)
        if let Ok(src) = tm.cast::<ITfSource>() {
            let sink: ITfThreadMgrEventSink = self.to_interface();
            match unsafe { src.AdviseSink(&ITfThreadMgrEventSink::IID, &sink) } {
                Ok(cookie) => self.tm_sink_cookie.set(Some(cookie)),
                Err(e) => debug_note(&format!("thread mgr sink advise failed {e:?}")),
            }
        }
        let km: ITfKeystrokeMgr = tm.cast()?;
        let sink: ITfKeyEventSink = self.to_interface();
        unsafe { km.AdviseKeyEventSink(tid, &sink, true) }
    }
}

impl ITfThreadMgrEventSink_Impl for TextService_Impl {
    fn OnInitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    fn OnUninitDocumentMgr(&self, _pdim: Ref<ITfDocumentMgr>) -> Result<()> {
        Ok(())
    }

    /// 같은 앱 안에서 다른 입력칸으로 옮겼다: 고정 중이면 알린다.
    fn OnSetFocus(&self, pdimfocus: Ref<ITfDocumentMgr>, _pdimprevfocus: Ref<ITfDocumentMgr>) -> Result<()> {
        if let Ok(dim) = pdimfocus.ok() {
            if let Ok(ctx) = unsafe { dim.GetTop() } {
                self.focus_notice(&ctx);
            }
        }
        Ok(())
    }

    fn OnPushContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }

    fn OnPopContext(&self, _pic: Ref<ITfContext>) -> Result<()> {
        Ok(())
    }
}

impl ITfKeyEventSink_Impl for TextService_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        if !fforeground.as_bool() {
            candwin::hide();
        } else {
            // 다른 앱에서 한/영 키로 고정을 바꿨을 수 있다: 입력 표시기를 맞추고, 고정 중이면 알린다
            match self.focused_context() {
                Some(ctx) => self.focus_notice(&ctx),
                None => {
                    let _ = self.call(&Request::Status);
                }
            }
            if juce::is_juce_class(&focus_class()) {
                juce::ensure_hook();
            }
        }
        Ok(())
    }

    /// 키를 먹을지 묻기만 한다 (상태를 바꾸지 않음). TSF는 보통 이 단계 없이 OnKeyDown을 바로 부른다.
    fn OnTestKeyDown(&self, pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        debug_log("TestKeyDown", wparam, lparam);
        // Shift 탭 판정용 시각 기록 (옛 입력칸(CUAS)에서는 Shift에 대해 OnKeyDown 없이 이것만 불린다)
        let repeat = (lparam.0 as u32 >> 30) & 1 == 1;
        if let Some(r) = self.note_right_alt_down(wparam, lparam, repeat) {
            return Ok(r.into());
        }
        if let Ok(mut s) = self.state.try_borrow_mut() {
            if is_left_shift(wparam, lparam) {
                if !repeat {
                    s.shift_down = Some(Instant::now());
                }
            } else {
                s.shift_down = None;
            }
        }
        if is_left_shift(wparam, lparam) {
            return Ok(false.into());
        }
        if let Ok(ctx) = pic.ok() {
            if !self.composing() && self.input_off(ctx) {
                return Ok(false.into());
            }
        }
        let Some(t) = self.call(&Request::Test { key: request_key(wparam) }) else {
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
        if let Some(r) = self.note_right_alt_down(wparam, lparam, repeat) {
            return Ok(r.into());
        }
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
        // JUCE 앱: 조합 창 위치를 정하려고 이 스레드의 마우스 클릭을 기록한다
        if juce::is_juce_class(&focus_class()) {
            juce::ensure_hook();
        }
        // 조합 창 모드의 조합이 다른 입력칸 것이면 (포커스 이동) 버린다
        let stale = self.state.try_borrow().ok().and_then(|s| s.floating.as_ref().map(|f| f.1)).is_some_and(|p| p != ctx.as_raw() as usize);
        if stale {
            self.reset();
        }
        // 비밀번호·숫자 칸 등: 조합을 시작하지 않고 키를 앱에 그대로 넘긴다
        if !self.composing() && self.input_off(ctx) {
            return Ok(false.into());
        }
        let key = request_key(wparam);
        if is_modifier_vk(key.vk) {
            return Ok(false.into());
        }
        let Some(t) = self.call(&Request::Test { key }) else {
            return Ok(self.offline_key(ctx, key).into());
        };
        // JUCE 앱, 새 조합의 첫 키: 앱이 알려 둔 위치를 지금 입력칸 기준으로 다시 받는다 (Tab으로 노트 이동)
        if t.eaten && !self.composing() && juce::is_juce_class(&focus_class()) {
            juce::refresh(unsafe { GetFocus() });
        }
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
        if key.vk == VK_HANGUL {
            self.toast_lock(ctx, &r);
        }
        Ok(true.into())
    }

    fn OnTestKeyUp(&self, _pic: Ref<ITfContext>, wparam: WPARAM, lparam: LPARAM) -> Result<BOOL> {
        debug_log("TestKeyUp", wparam, lparam);
        if is_right_alt(wparam, lparam) {
            return Ok(self.right_alt_tap_pending().into());
        }
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
        if is_right_alt(wparam, lparam) {
            // 오른쪽 Alt 단독 탭: 한/영 키처럼 언어 고정 순환. 뗌을 먹어서 앱 메뉴 막대가 켜지지 않게 한다.
            let tap = self.right_alt_tap_pending();
            if let Ok(mut s) = self.state.try_borrow_mut() {
                s.ralt_down = None;
            }
            if !tap {
                return Ok(false.into());
            }
            let Ok(ctx) = pic.ok() else { return Ok(false.into()) };
            if !self.composing() && self.input_off(ctx) {
                return Ok(false.into());
            }
            let key = Key { vk: VK_HANGUL, shift: false, caps: false, ctrl: false, alt: false };
            let Some(r) = self.call(&Request::Key { key }) else { return Ok(false.into()) };
            if !r.eaten {
                return Ok(false.into());
            }
            self.apply(ctx, &r)?;
            self.toast_lock(ctx, &r);
            return Ok(true.into());
        }
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
        debug_note(&format!("composition terminated by app (focus class {})", focus_class()));
        note_composition_terminated_by_app();
        self.reset();
        Ok(())
    }
}

// ---------------------------------------------------------------- 편집 세션
// ---------------------------------------------------------------- 자동 꺼짐
/// IME를 쓰지 않는 입력칸 용도 (비밀번호·PIN, 숫자·전화·금액, 이메일·로그인 이름, URL, 반각 영숫자).
/// 비밀번호·PIN 용도: 여러 용도 중 어디에 있든 끈다 (보안 검토 R2).
const PROTECTED_SCOPES: &[i32] = &[IS_PASSWORD.0, IS_NUMERIC_PASSWORD.0, IS_NUMERIC_PIN.0, IS_ALPHANUMERIC_PIN.0];

/// 입력 용도 목록으로 IME를 끌지 정한다. 비밀번호·PIN은 하나라도 있으면 끈다.
/// 나머지(주소·숫자 등)는 첫 번째(주된) 용도로만 판단한다 (예: 검색칸은 IS_SEARCH가 먼저, 이메일칸은
/// IS_EMAIL_SMTPEMAILADDRESS가 먼저. 뒤에 붙은 보조 용도로 끄면 검색창에서 IME가 꺼진다).
fn scopes_off(scopes: &[i32]) -> bool {
    scopes.iter().any(|s| PROTECTED_SCOPES.contains(s)) || scopes.first().is_some_and(|s| OFF_SCOPES.contains(s))
}

const OFF_SCOPES: &[i32] = &[
    IS_URL.0,
    IS_EMAIL_USERNAME.0,
    IS_EMAIL_SMTPEMAILADDRESS.0,
    IS_LOGINNAME.0,
    IS_CURRENCY_AMOUNT.0,
    IS_DIGITS.0,
    IS_NUMBER.0,
    IS_PASSWORD.0,
    IS_TELEPHONE_FULLTELEPHONENUMBER.0,
    IS_TELEPHONE_COUNTRYCODE.0,
    IS_TELEPHONE_AREACODE.0,
    IS_TELEPHONE_LOCALNUMBER.0,
    IS_ALPHANUMERIC_HALFWIDTH.0,
    IS_NUMERIC_PASSWORD.0,
    IS_NUMERIC_PIN.0,
    IS_ALPHANUMERIC_PIN.0,
];

/// 조합 창은 CUAS를 거치는 Win32 편집 컨트롤에서만 사용한다.
/// TS_SS_TRANSITORY는 사용 주기가 짧은 문서라는 뜻이라, 이것만으로 옛 입력칸을 판별하면
/// 그 플래그를 쓰는 다른 TSF 문맥까지 조합 창 모드로 잘못 분류할 수 있다.
/// 처음부터 조합 창 모드로 쓰는 프로그램 (자체 입력칸이 조합을 키마다 끝내는 것으로 확인된 것).
const FLOATING_APPS: [&str; 1] = ["kakaotalk.exe"];

/// 조합을 시작하자마자 앱이 끝내 버리는 입력칸을 배운다 (창 클래스 단위, 이 프로세스 동안).
/// 조합을 시작하고 0.5초 안에 앱이 끝내는 일이 같은 클래스에서 두 번 이어지면 그 클래스는 조합 창 모드로 쓴다.
struct QuickEnd {
    started: Option<(Instant, String)>,
    count: u32,
    learned: Vec<String>,
}
static QUICK_END: std::sync::Mutex<QuickEnd> = std::sync::Mutex::new(QuickEnd { started: None, count: 0, learned: Vec::new() });

fn focus_class() -> String {
    let focus = unsafe { GetFocus() };
    if focus.0.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(focus, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn note_composition_started() {
    if let Ok(mut q) = QUICK_END.lock() {
        q.started = Some((Instant::now(), focus_class()));
    }
}

fn note_composition_terminated_by_app() {
    let Ok(mut q) = QUICK_END.lock() else { return };
    let class = focus_class();
    let quick = q.started.take().is_some_and(|(t, c)| t.elapsed() < Duration::from_millis(500) && c == class);
    if !quick {
        q.count = 0;
        return;
    }
    q.count += 1;
    if q.count >= 2 && !class.is_empty() && !q.learned.contains(&class) {
        debug_note(&format!("floating mode learned for class {class}"));
        q.learned.push(class);
    }
}

/// JUCE 앱(Synthesizer V 등)의 조합 창 위치.
///
/// JUCE 입력칸은 창 핸들이 없는 자체 컴포넌트라, TSF(CUAS)가 주는 위치는 앱이 IMM 후보 위치로 알려 준 값이다.
/// JUCE는 그 값을 조합 메시지(WM_IME_COMPOSITION)를 받을 때만 고치므로, 조합 창 모드에서는 글자를 확정할 때만
/// 갱신된다. 새 입력칸(다른 노트의 가사)을 열고 첫 확정 전까지는 이전 입력칸 위치가 남는다. 시스템 커서는 창의 (0,0).
/// 그래서 마지막 확정 뒤에 마우스 클릭(노트 더블 클릭)이 있었으면 그 자리 아래에 띄운다.
mod juce {
    use std::cell::Cell;
    use std::time::Instant;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::UI::Input::Ime::{ImmGetCandidateWindow, ImmGetContext, ImmReleaseContext, CANDIDATEFORM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, SendMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MOUSEHOOKSTRUCT, WH_MOUSE, WM_IME_COMPOSITION,
        WM_IME_ENDCOMPOSITION, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    };

    thread_local! {
        static HOOK: Cell<isize> = const { Cell::new(0) };
        static CLICK: Cell<Option<(POINT, Instant)>> = const { Cell::new(None) };
        static COMMIT: Cell<Option<Instant>> = const { Cell::new(None) };
    }

    pub fn is_juce_class(class: &str) -> bool {
        class.starts_with("JUCE_")
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam.0 as u32 == WM_LBUTTONDOWN || wparam.0 as u32 == WM_LBUTTONDBLCLK) && lparam.0 != 0 {
            let m = unsafe { &*(lparam.0 as *const MOUSEHOOKSTRUCT) };
            CLICK.with(|c| c.set(Some((m.pt, Instant::now()))));
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// 이 스레드의 마우스 클릭을 기록하기 시작한다 (스레드 전용 훅, 이 프로세스 안).
    pub fn ensure_hook() {
        if HOOK.with(|h| h.get()) != 0 {
            return;
        }
        if let Ok(h) = unsafe { SetWindowsHookExW(WH_MOUSE, Some(mouse_proc), None, GetCurrentThreadId()) } {
            HOOK.with(|c| c.set(h.0 as isize));
        }
    }

    /// 훅을 푼다 (TIP 비활성화 때. DLL이 내려가기 전에 반드시).
    pub fn remove_hook() {
        let h = HOOK.with(|c| c.replace(0));
        if h != 0 {
            let _ = unsafe { UnhookWindowsHookEx(HHOOK(h as *mut _)) };
        }
        CLICK.with(|c| c.set(None));
    }

    /// 앱이 알려 준 위치가 지금 입력칸 것으로 바뀐 시각 (확정했거나 `refresh`로 다시 받았을 때).
    pub fn note_commit() {
        COMMIT.with(|c| c.set(Some(Instant::now())));
    }

    /// 앱이 IMM에 알려 둔 후보 위치 (창 기준 좌표).
    fn ime_pos(hwnd: HWND) -> Option<POINT> {
        unsafe {
            let himc = ImmGetContext(hwnd);
            if himc.0.is_null() {
                return None;
            }
            let mut cand = CANDIDATEFORM::default();
            let ok = ImmGetCandidateWindow(himc, 0, &mut cand).as_bool();
            let _ = ImmReleaseContext(hwnd, himc);
            ok.then_some(cand.ptCurrentPos)
        }
    }

    /// 조합을 시작하기 전에 JUCE가 후보 위치를 지금 입력칸 기준으로 다시 정하게 한다.
    /// JUCE는 WM_IME_COMPOSITION을 받을 때마다 (내용 플래그가 없어도) 후보 위치를 커서 아래로 옮기고,
    /// 이어 보내는 WM_IME_ENDCOMPOSITION은 진행 중인 조합이 없으면 내부 상태만 초기화한다. 글자는 바뀌지 않는다.
    /// 위치가 바뀌었으면 그 뒤로는 앱이 준 위치를 믿는다 (Tab으로 다른 노트로 옮긴 경우).
    pub fn refresh(hwnd: HWND) {
        if hwnd.0.is_null() {
            return;
        }
        let before = ime_pos(hwnd);
        unsafe {
            SendMessageW(hwnd, WM_IME_COMPOSITION, Some(WPARAM(0)), Some(LPARAM(0)));
            SendMessageW(hwnd, WM_IME_ENDCOMPOSITION, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        let after = ime_pos(hwnd);
        if after.is_some() && after != before {
            note_commit();
        }
        crate::debug_note(&format!("juce refresh {before:?} -> {after:?}"));
    }

    /// 조합 창을 띄울 위치. `tsf`: TSF가 준 위치, `window`: 포커스 창 화면 영역.
    pub fn anchor(tsf: Option<RECT>, window: RECT) -> Option<RECT> {
        let click = CLICK.with(|c| c.get());
        let commit = COMMIT.with(|c| c.get());
        choose(tsf, window, click, commit)
    }

    /// 일반 조합 창 모드: TSF 위치가 창 안이면 그대로. 아니면(게임처럼 위치를 안 알려 주는 앱) 창 안의 마지막 클릭 아래,
    /// 그것도 없으면 창 아래쪽 가운데.
    /// 커서 사각형으로 보기에 너무 높은 값 (px), 그때 쓸 줄 높이.
    const TALL: i32 = 80;
    const LINE: i32 = 20;

    pub fn fallback(tsf: Option<RECT>, window: RECT) -> Option<RECT> {
        let click = CLICK.with(|c| c.get()).map(|(p, _)| p);
        Some(fallback_for(tsf, window, click))
    }

    pub fn fallback_for(tsf: Option<RECT>, window: RECT, click: Option<POINT>) -> RECT {
        let inside_pt = |x: i32, y: i32| x >= window.left && x < window.right && y >= window.top && y < window.bottom;
        if let Some(r) = tsf.filter(|r| inside_pt(r.left, r.top)) {
            // 한워드: 창 위쪽부터 커서 줄까지 이어진 큰 사각형을 준다 (IMM 조합 영역 위쪽 ~ 조합 위치).
            // 글자 한 줄보다 훨씬 높으면 바닥(앱이 알려 준 커서 점)을 커서 줄로 본다.
            if r.bottom - r.top > TALL {
                return RECT { left: r.left, top: r.bottom, right: r.left + 1, bottom: r.bottom + LINE };
            }
            return r;
        }
        if let Some(p) = click.filter(|p| inside_pt(p.x, p.y)) {
            return RECT { left: p.x, top: p.y + 12, right: p.x + 1, bottom: p.y + 28 };
        }
        let x = window.left + (window.right - window.left) / 3;
        let y = window.bottom - (window.bottom - window.top) / 6;
        RECT { left: x, top: y, right: x + 1, bottom: y + 16 }
    }

    /// 마지막 확정 뒤의 클릭이 있으면 그 아래, 아니면 TSF 위치 (창 밖이면 클릭 자리).
    pub fn choose(tsf: Option<RECT>, window: RECT, click: Option<(POINT, Instant)>, commit: Option<Instant>) -> Option<RECT> {
        let below = |p: POINT| RECT { left: p.x, top: p.y + 12, right: p.x + 1, bottom: p.y + 28 };
        let inside = |r: &RECT| r.left >= window.left && r.left < window.right && r.top >= window.top && r.top < window.bottom;
        match (tsf, click) {
            (_, Some((p, t))) if commit.is_none_or(|c| t > c) => Some(below(p)),
            (Some(r), _) if inside(&r) => Some(r),
            (_, Some((p, _))) => Some(below(p)),
            (r, None) => r,
        }
    }
}

/// 진단 로그가 켜져 있는가 (DLL 옆 MULTILANG_TIP_DEBUG).
fn debug_enabled() -> bool {
    debug_dir().is_some()
}

/// 진단 로그 폴더 기준: DLL 옆, 32비트 DLL(x86\)이면 한 단계 위 (설치 폴더). MULTILANG_TIP_DEBUG가 있을 때만.
fn debug_dir() -> Option<std::path::PathBuf> {
    let dll = std::path::PathBuf::from(String::from_utf16_lossy(&host_path()));
    let mut dir = dll.parent()?.to_path_buf();
    if dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("x86")) {
        dir = dir.parent()?.to_path_buf();
    }
    dir.join("MULTILANG_TIP_DEBUG").exists().then_some(dir)
}

/// 조합 창 위치 진단: TSF가 준 위치와 함께 시스템 커서, IMM 조합·후보 창 위치, 포커스 창, DPI를 남긴다.
fn floating_diag(anchor: Option<RECT>) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::HiDpi::{GetAwarenessFromDpiAwarenessContext, GetDpiForWindow, GetThreadDpiAwarenessContext};
    use windows::Win32::UI::Input::Ime::{ImmGetCandidateWindow, ImmGetCompositionWindow, ImmGetContext, ImmReleaseContext, CANDIDATEFORM, COMPOSITIONFORM};
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GetWindowRect, GUITHREADINFO};
    if !debug_enabled() {
        return;
    }
    let r = |rc: &RECT| format!("{},{}-{},{}", rc.left, rc.top, rc.right, rc.bottom);
    let mut out = format!("floating diag: tsf={}", anchor.as_ref().map(r).unwrap_or_else(|| "none".into()));
    unsafe {
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(0, &mut gti).is_ok() && !gti.hwndCaret.0.is_null() {
            let mut p = POINT { x: gti.rcCaret.left, y: gti.rcCaret.top };
            let _ = ClientToScreen(gti.hwndCaret, &mut p);
            out += &format!(" caret={} (client {}) hwnd={:?}", format!("{},{}", p.x, p.y), r(&gti.rcCaret), gti.hwndCaret.0);
        } else {
            out += " caret=none";
        }
        let focus = GetFocus();
        if !focus.0.is_null() {
            let mut wr = RECT::default();
            let _ = GetWindowRect(focus, &mut wr);
            let mut buf = [0u16; 128];
            let n = GetClassNameW(focus, &mut buf) as usize;
            out += &format!(" focus={:?} class={} rect={} dpi={}", focus.0, String::from_utf16_lossy(&buf[..n]), r(&wr), GetDpiForWindow(focus));
            let himc = ImmGetContext(focus);
            if !himc.0.is_null() {
                let mut cf = COMPOSITIONFORM::default();
                if ImmGetCompositionWindow(himc, &mut cf).as_bool() {
                    let mut p = cf.ptCurrentPos;
                    let _ = ClientToScreen(focus, &mut p);
                    out += &format!(" immcomp style={:#x} pos={},{} (screen {},{}) area={}", cf.dwStyle, cf.ptCurrentPos.x, cf.ptCurrentPos.y, p.x, p.y, r(&cf.rcArea));
                }
                for i in 0..2u32 {
                    let mut cand = CANDIDATEFORM::default();
                    if ImmGetCandidateWindow(himc, i, &mut cand).as_bool() {
                        let mut p = cand.ptCurrentPos;
                        let _ = ClientToScreen(focus, &mut p);
                        out += &format!(" immcand{i} style={:#x} pos={},{} (screen {},{}) area={}", cand.dwStyle, cand.ptCurrentPos.x, cand.ptCurrentPos.y, p.x, p.y, r(&cand.rcArea));
                    }
                }
                let _ = ImmReleaseContext(focus, himc);
            } else {
                out += " himc=none";
            }
        }
        out += &format!(" awareness={:?}", GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext()));
        let mut cur = POINT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut cur);
        out += &format!(" mouse={},{}", cur.x, cur.y);
    }
    debug_note(&out);
}

fn legacy_floating(context: &ITfContext) -> bool {
    if FLOATING_APPS.contains(&app_name().to_lowercase().as_str()) {
        return true;
    }
    let class = focus_class();
    if !class.is_empty() && QUICK_END.lock().is_ok_and(|q| q.learned.contains(&class)) {
        return true;
    }
    let transitory = unsafe { context.GetStatus() }
        .map(|st| st.dwStaticFlags & TS_SS_TRANSITORY != 0)
        .unwrap_or(false);
    let is_edit_window = |mut hwnd: windows::Win32::Foundation::HWND| {
        for _ in 0..8 {
            if hwnd.0.is_null() {
                break;
            }
            let mut buf = [0u16; 128];
            let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
            let class = String::from_utf16_lossy(&buf[..n]).to_ascii_lowercase();
            if class == "edit" || class.starts_with("windowsforms10.edit") {
                return true;
            }
            hwnd = unsafe { GetParent(hwnd) }.unwrap_or_default();
        }
        false
    };
    let is_standard_dialog = |mut hwnd: windows::Win32::Foundation::HWND| {
        for _ in 0..8 {
            if hwnd.0.is_null() {
                break;
            }
            let mut buf = [0u16; 64];
            let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
            if String::from_utf16_lossy(&buf[..n]) == "#32770" {
                return true;
            }
            hwnd = unsafe { GetParent(hwnd) }.unwrap_or_default();
        }
        false
    };
    let focus = unsafe { GetFocus() };
    if is_edit_window(focus) {
        return true;
    }
    let view_hwnd = unsafe { context.GetActiveView() }.and_then(|view| unsafe { view.GetWnd() }).unwrap_or_default();
    if is_edit_window(view_hwnd) {
        return true;
    }
    // CharMap routes its legacy Edit field through a standard dialog owner; its focused
    // child HWND is not always visible to GetFocus from the TIP callback thread.
    transitory && (is_standard_dialog(focus) || is_standard_dialog(view_hwnd))
}

/// 스레드 관리자의 compartment에 정수 값을 쓴다.
fn set_thread_compartment(tm: &ITfThreadMgr, tid: u32, guid: &GUID, value: i32) {
    let r = (|| -> Result<()> {
        let mgr: ITfCompartmentMgr = tm.cast()?;
        let c = unsafe { mgr.GetCompartment(guid)? };
        unsafe { c.SetValue(tid, &variant_i4(value)) }
    })();
    if let Err(e) = r {
        debug_note(&format!("compartment set failed: {e:?}"));
    }
}

/// 문맥의 compartment 값이 켜져 있는가 (키보드 비활성, 빈 문맥).
fn compartment_on(context: &ITfContext, guid: &GUID) -> bool {
    (|| -> Result<bool> {
        let mgr: ITfCompartmentMgr = context.cast()?;
        let c = unsafe { mgr.GetCompartment(guid)? };
        let mut v = unsafe { c.GetValue()? };
        let on = unsafe { v.Anonymous.Anonymous.vt == VT_I4 && v.Anonymous.Anonymous.Anonymous.lVal != 0 };
        unsafe {
            let _ = VariantClear(&mut v);
        }
        Ok(on)
    })()
    .unwrap_or(false)
}

/// 커서(선택 영역)의 화면 위치를 읽는 읽기 전용 편집 세션 (한/영 알림 위치).
#[implement(ITfEditSession)]
struct CaretSession {
    context: ITfContext,
    out: Rc<Cell<Option<RECT>>>,
}

impl ITfEditSession_Impl for CaretSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let mut sel = [TF_SELECTION::default()];
        let mut n = 0u32;
        unsafe { self.context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut n)? };
        if n == 0 {
            return Ok(());
        }
        let Some(range) = (unsafe { ManuallyDrop::take(&mut sel[0].range) }) else { return Ok(()) };
        let view = unsafe { self.context.GetActiveView()? };
        let mut rc = RECT::default();
        let mut clipped = BOOL::default();
        unsafe { view.GetTextExt(ec, &range, &mut rc, &mut clipped)? };
        self.out.set(Some(rc));
        Ok(())
    }
}

/// 입력칸 진입 알림: 커서 자리의 용도를 보고(비밀번호 등이면 그만둔다) 커서 옆에 고정 상태를 보인다.
#[implement(ITfEditSession)]
struct FocusToastSession {
    context: ITfContext,
    text: String,
    badge: &'static str,
}

impl ITfEditSession_Impl for FocusToastSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let scopes = Rc::new(Cell::new(None));
        let read: ITfEditSession = ScopeSession { context: self.context.clone(), out: scopes.clone() }.into();
        let _ = unsafe { read.DoEditSession(ec) };
        if scopes_off(&scopes.take().unwrap_or_default()) {
            return Ok(());
        }
        let caret = Rc::new(Cell::new(None));
        let read: ITfEditSession = CaretSession { context: self.context.clone(), out: caret.clone() }.into();
        let _ = unsafe { read.DoEditSession(ec) };
        // 배치가 아직 없으면(빈 사각형) 시스템 커서로, 그것도 없으면 보이지 않는다
        let anchor = caret.take().filter(|rc: &RECT| rc.bottom > rc.top && (rc.left != 0 || rc.top != 0)).or_else(system_caret);
        if let Some(anchor) = anchor {
            candwin::show_toast(&self.text, self.badge, anchor);
        }
        Ok(())
    }
}

/// 시스템 커서(캐럿) 위치 (화면 좌표). 없으면 None.
fn system_caret() -> Option<RECT> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
    unsafe {
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(0, &mut gti).is_err() || gti.hwndCaret.0.is_null() {
            return None;
        }
        let mut p = POINT { x: gti.rcCaret.left, y: gti.rcCaret.top };
        if !ClientToScreen(gti.hwndCaret, &mut p).as_bool() {
            return None;
        }
        let h = (gti.rcCaret.bottom - gti.rcCaret.top).max(16);
        Some(RECT { left: p.x, top: p.y, right: p.x + 1, bottom: p.y + h })
    }
}

/// 고정 상태 알림 문구와 언어 배지.
fn lock_label(ind: &ipc::Indicator) -> (String, &'static str) {
    let text = match (ind.locked, ind.lang.as_str()) {
        (false, _) => "자동 전환",
        (true, "en") => "English 고정 (IME 끔)",
        (true, "ja") => "日本語 고정",
        (true, _) => "한국어 고정",
    };
    let badge = match ind.lang.as_str() {
        "en" => "A",
        "ja" => "あ",
        _ => "가",
    };
    (text.to_string(), badge)
}

/// 입력칸 진입 알림 사이의 최소 간격 (앱 전환과 입력칸 포커스가 함께 올 때 한 번만).
const FOCUS_NOTICE_GAP: Duration = Duration::from_millis(1000);

/// 커서 자리의 입력칸 용도(InputScope)를 읽는 읽기 전용 편집 세션.
#[implement(ITfEditSession)]
struct ScopeSession {
    context: ITfContext,
    out: Rc<Cell<Option<Vec<i32>>>>,
}

impl ITfEditSession_Impl for ScopeSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let mut sel = [TF_SELECTION::default()];
        let mut n = 0u32;
        unsafe { self.context.GetSelection(ec, TF_DEFAULT_SELECTION, &mut sel, &mut n) }
            .inspect_err(|e| debug_note(&format!("scope: GetSelection {e:?}")))?;
        if n == 0 {
            return Ok(());
        }
        let Some(range) = (unsafe { ManuallyDrop::take(&mut sel[0].range) }) else { return Ok(()) };
        let prop = unsafe { self.context.GetAppProperty(&GUID_PROP_INPUTSCOPE) }
            .inspect_err(|e| debug_note(&format!("scope: GetAppProperty {e:?}")))?;
        let mut v = unsafe { prop.GetValue(ec, &range) }.inspect_err(|e| debug_note(&format!("scope: GetValue {e:?}")))?;
        let unk = unsafe {
            if v.Anonymous.Anonymous.vt == VT_UNKNOWN { (*v.Anonymous.Anonymous.Anonymous.punkVal).clone() } else { None }
        };
        unsafe {
            let _ = VariantClear(&mut v);
        }
        let Some(unk) = unk else {
            self.out.set(Some(Vec::new()));
            return Ok(());
        };
        let scope: ITfInputScope = unk.cast().inspect_err(|e| debug_note(&format!("scope: cast {e:?}")))?;
        let mut p: *mut InputScope = std::ptr::null_mut();
        let mut count = 0u32;
        unsafe { scope.GetInputScopes(&mut p, &mut count) }.inspect_err(|e| debug_note(&format!("scope: GetInputScopes {e:?}")))?;
        let mut list = Vec::new();
        if !p.is_null() {
            for i in 0..count as usize {
                list.push(unsafe { (*p.add(i)).0 });
            }
            unsafe { CoTaskMemFree(Some(p as *const c_void)) };
        }
        self.out.set(Some(list));
        Ok(())
    }
}

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
    /// 조합 창 모드 (옛 입력칸): 조합을 입력칸에 넣지 않고 확정 글만 넣는다
    floating: bool,
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
        debug_note("composition started");
        note_composition_started();
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
        match &rect {
            Ok(rc) => debug_note(&format!("anchor ok {},{} - {},{}", rc.left, rc.top, rc.right, rc.bottom)),
            Err(e) => debug_note(&format!("anchor failed {e:?}")),
        }
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
        // 2-a) 조합 창 모드: 입력칸에는 조합을 넣지 않고, 커서 위치만 기록한다 (조합 창을 띄울 곳)
        if self.floating {
            if let Some(comp) = self.composition() {
                let range = unsafe { comp.GetRange()? };
                unsafe { range.SetText(ec, 0, &[])? };
                self.end_composition(ec, &comp)?;
            }
            if !self.preedit.is_empty() {
                let ins: ITfInsertAtSelection = self.context.cast()?;
                if let Ok(range) = unsafe { ins.InsertTextAtSelection(ec, TF_IAS_QUERYONLY, &[]) } {
                    self.remember_anchor(ec, &range);
                }
            }
            return Ok(());
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
    let res = set_reg_sz(key, PCWSTR::null(), &host_path())
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

const CATEGORIES: [GUID; 5] = [
    GUID_TFCAT_TIP_KEYBOARD,
    GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT,
    GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT,
    GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER,
    // 후보 목록을 UI 요소(ITfCandidateListUIElement)로 알린다 (UI-less 앱·전체 화면 게임용)
    GUID_TFCAT_TIPCAP_UIELEMENTENABLED,
];

fn register_tsf() -> Result<()> {
    let profiles: ITfInputProcessorProfileMgr =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)? };
    let desc = utf16(DESCRIPTION);
    let icon = host_path();
    for (lang, profile) in PROFILES {
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
            for (lang, profile) in PROFILES {
                let _ = profiles.UnregisterProfile(&CLSID_TEXT_SERVICE, lang, &profile, 0);
            }
        }
        if let Ok(cats) = CoCreateInstance::<_, ITfCategoryMgr>(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER) {
            for cat in CATEGORIES {
                let _ = cats.UnregisterCategory(&CLSID_TEXT_SERVICE, &cat, &CLSID_TEXT_SERVICE);
            }
        }
        // the text service itself (removes HKLM\...\CTF\TIP\{CLSID}; the calls above leave empty keys behind)
        if let Ok(p) = CoCreateInstance::<_, ITfInputProcessorProfiles>(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) {
            let _ = p.Unregister(&CLSID_TEXT_SERVICE);
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

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn protected_scope_anywhere_turns_off() {
        assert!(scopes_off(&[IS_PASSWORD.0]));
        assert!(scopes_off(&[IS_DEFAULT.0, IS_PASSWORD.0]));
        assert!(scopes_off(&[IS_SEARCH.0, IS_NUMERIC_PIN.0]));
        assert!(scopes_off(&[IS_TEXT.0, IS_ALPHANUMERIC_PIN.0, IS_NUMERIC_PASSWORD.0]));
    }

    #[test]
    fn other_off_scopes_only_when_first() {
        assert!(scopes_off(&[IS_URL.0]));
        assert!(scopes_off(&[IS_EMAIL_SMTPEMAILADDRESS.0, IS_TEXT.0]));
        assert!(!scopes_off(&[IS_SEARCH.0, IS_URL.0]));
        assert!(!scopes_off(&[IS_DEFAULT.0, IS_NUMBER.0]));
    }

    #[test]
    fn ordinary_fields_stay_on() {
        assert!(!scopes_off(&[]));
        assert!(!scopes_off(&[IS_DEFAULT.0]));
        assert!(!scopes_off(&[IS_SEARCH.0]));
        assert!(!scopes_off(&[IS_TEXT.0, IS_CHAT.0]));
    }
}

#[cfg(test)]
mod juce_tests {
    use super::juce::choose;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{POINT, RECT};

    const WINDOW: RECT = RECT { left: 27, top: 111, right: 2054, bottom: 1264 };
    const STALE: RECT = RECT { left: 618, top: 943, right: 634, bottom: 959 };

    #[test]
    fn click_after_last_commit_wins() {
        let commit = Instant::now();
        let click = (POINT { x: 397, y: 869 }, commit + Duration::from_millis(10));
        assert_eq!(choose(Some(STALE), WINDOW, Some(click), Some(commit)), Some(RECT { left: 397, top: 881, right: 398, bottom: 897 }));
        // 아직 확정한 적이 없어도 클릭 자리
        assert_eq!(choose(Some(STALE), WINDOW, Some(click), None).map(|r| r.left), Some(397));
    }

    #[test]
    fn after_commit_tsf_position_is_used() {
        let click = (POINT { x: 397, y: 869 }, Instant::now());
        let commit = click.1 + Duration::from_millis(10);
        assert_eq!(choose(Some(STALE), WINDOW, Some(click), Some(commit)), Some(STALE));
        assert_eq!(choose(Some(STALE), WINDOW, None, Some(commit)), Some(STALE));
    }

    #[test]
    fn outside_window_falls_back_to_click() {
        let click = (POINT { x: 397, y: 869 }, Instant::now());
        let commit = click.1 + Duration::from_millis(10);
        let corner = RECT { left: 2559, top: 1391, right: 2560, bottom: 1391 };
        assert_eq!(choose(Some(corner), WINDOW, Some(click), Some(commit)).map(|r| r.left), Some(397));
        assert_eq!(choose(Some(corner), WINDOW, None, None), Some(corner));
    }
}

#[cfg(test)]
mod lock_key_tests {
    use super::*;

    #[test]
    fn right_alt_needs_extended_flag() {
        let ext = LPARAM((1 << 24) | (0x38 << 16) | 1);
        let left = LPARAM((0x38 << 16) | 1);
        assert!(is_right_alt(WPARAM(0x12), ext));
        assert!(is_right_alt(WPARAM(0xA5), ext));
        assert!(!is_right_alt(WPARAM(0x12), left)); // 왼쪽 Alt
        assert!(!is_right_alt(WPARAM(0x15), ext)); // 한/영 키는 따로
    }

    #[test]
    fn japanese_hankaku_zenkaku_maps_to_lock_key() {
        assert!(is_ja_lock_key(WPARAM(0xF3)));
        assert!(is_ja_lock_key(WPARAM(0xF4)));
        assert_eq!(request_key(WPARAM(0xF3)).vk, VK_HANGUL);
        assert!(!is_ja_lock_key(WPARAM(0x41)));
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::juce::fallback_for;
    use windows::Win32::Foundation::{POINT, RECT};

    const WIN: RECT = RECT { left: 589, top: 316, right: 1971, bottom: 1123 };

    #[test]
    fn corner_anchor_is_replaced() {
        let corner = RECT { left: 2559, top: 1391, right: 2560, bottom: 1391 };
        // 클릭이 창 안에 있으면 그 아래
        assert_eq!(fallback_for(Some(corner), WIN, Some(POINT { x: 900, y: 1000 })).left, 900);
        // 없으면 창 아래쪽
        let r = fallback_for(Some(corner), WIN, None);
        assert!(r.left > WIN.left && r.left < WIN.right && r.top > 900 && r.top < WIN.bottom);
        // 창 안의 위치는 그대로
        // 한워드: 창 위쪽부터 커서 줄까지의 큰 사각형 → 바닥을 커서 줄로
        let hword = RECT { left: 502, top: 310, right: 1542, bottom: 561 };
        let hword_win = RECT { left: 62, top: 310, right: 1542, bottom: 1325 };
        let r = fallback_for(Some(hword), hword_win, None);
        assert_eq!((r.left, r.top), (502, 561));
        let ok = RECT { left: 700, top: 500, right: 716, bottom: 516 };
        assert_eq!(fallback_for(Some(ok), WIN, Some(POINT { x: 900, y: 1000 })), ok);
    }
}
