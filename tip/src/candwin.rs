//! 후보창: 조합 바로 아래에 뜨는, 포커스를 가져가지 않는 팝업.
//!
//! 앱의 UI 스레드에서 TIP가 직접 그린다 (스레드마다 창 하나). 게임 등 UI-less 앱 지원은 M5.
//!
//! 모양은 Windows 11 플라이아웃을 따른다: 둥근 모서리(DWM, Windows 11에서만), 여백 있는 줄, 선택 줄은
//! 옅은 배경과 왼쪽 강조 막대, 번호·언어 표시는 흐리게. 색은 Windows 앱 테마(밝게/어둡게)를 따르고,
//! 고대비 테마에서는 시스템 색만 쓴다. 가나가 든 줄은 Yu Gothic UI, 나머지는 맑은 고딕.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use ipc::CandidateList;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: PCWSTR = w!("MultilangImeCandidateWindow");
static REGISTERED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Win {
    hwnd: Option<HWND>,
    list: CandidateList,
    /// 조합 창 모드: 맨 윗줄에 보이는 조합 문자열 (옛 입력칸용)
    header: Option<String>,
    /// 잠깐 보이는 알림 (한/영 키로 바꾼 입력 언어 고정 상태): 밑줄 없이, 시간이 지나면 숨긴다
    toast: bool,
    /// 알림 앞의 언어 글자 (가 / A / あ)
    badge: String,
    /// DWM이 모서리를 둥글게 그리는가 (Windows 11). 그러면 테두리도 DWM이 그린다.
    rounded: bool,
}

/// 알림을 숨기는 타이머
const TOAST_TIMER: usize = 0x4D4C;
const TOAST_MS: u32 = 1200;

thread_local! {
    static WIN: RefCell<Win> = RefCell::new(Win::default());
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// --- 색 ---

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

#[derive(Clone, Copy)]
struct Theme {
    bg: COLORREF,
    border: COLORREF,
    fg: COLORREF,
    dim: COLORREF,
    sel_bg: COLORREF,
    sel_fg: COLORREF,
    sel_dim: COLORREF,
    /// 선택 줄 왼쪽 막대, 알림 배지 (고대비에서는 없음)
    accent: Option<COLORREF>,
    on_accent: COLORREF,
    /// 언어 표시 칩 배경 (고대비에서는 없음)
    chip: Option<COLORREF>,
    chip_sel: Option<COLORREF>,
}

const LIGHT: Theme = Theme {
    bg: rgb(249, 249, 249),
    border: rgb(222, 222, 222),
    fg: rgb(26, 26, 26),
    dim: rgb(96, 96, 96),
    sel_bg: rgb(234, 234, 234),
    sel_fg: rgb(26, 26, 26),
    sel_dim: rgb(80, 80, 80),
    accent: Some(rgb(0, 95, 184)),
    on_accent: rgb(255, 255, 255),
    chip: Some(rgb(237, 237, 237)),
    chip_sel: Some(rgb(222, 222, 222)),
};

const DARK: Theme = Theme {
    bg: rgb(44, 44, 44),
    border: rgb(72, 72, 72),
    fg: rgb(255, 255, 255),
    dim: rgb(190, 190, 190),
    sel_bg: rgb(61, 61, 61),
    sel_fg: rgb(255, 255, 255),
    sel_dim: rgb(205, 205, 205),
    accent: Some(rgb(76, 194, 255)),
    on_accent: rgb(0, 0, 0),
    chip: Some(rgb(58, 58, 58)),
    chip_sel: Some(rgb(78, 78, 78)),
};

fn high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW { cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, Some(&mut hc as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    }
    .is_ok();
    ok && (hc.dwFlags & HCF_HIGHCONTRASTON).0 != 0
}

/// Windows 앱 테마가 밝음인가 (읽지 못하면 밝음)
fn apps_light() -> bool {
    let mut v = 1u32;
    let mut n = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut n),
        )
    };
    r.is_err() || v != 0
}

fn theme() -> Theme {
    if high_contrast() {
        let c = |i| COLORREF(unsafe { GetSysColor(i) });
        return Theme {
            bg: c(COLOR_WINDOW),
            border: c(COLOR_WINDOWTEXT),
            fg: c(COLOR_WINDOWTEXT),
            dim: c(COLOR_WINDOWTEXT),
            sel_bg: c(COLOR_HIGHLIGHT),
            sel_fg: c(COLOR_HIGHLIGHTTEXT),
            sel_dim: c(COLOR_HIGHLIGHTTEXT),
            accent: None,
            on_accent: c(COLOR_WINDOWTEXT),
            chip: None,
            chip_sel: None,
        };
    }
    if apps_light() { LIGHT } else { DARK }
}

// --- 글꼴 ---

#[derive(Clone, Copy)]
struct Fonts {
    dpi: i32,
    ko: HFONT,
    ja: HFONT,
    small: HFONT,
    small_ja: HFONT,
    badge: HFONT,
}

fn make_font(px: i32, weight: i32, face: PCWSTR) -> HFONT {
    unsafe {
        CreateFontW(-px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                    (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, face)
    }
}

/// 이 창의 DPI에 맞는 글꼴 (스레드마다 하나, DPI가 바뀌면 다시 만든다)
fn fonts(hwnd: HWND) -> Fonts {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    FONTS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(f) = *c {
            if f.dpi == dpi {
                return f;
            }
            for h in [f.ko, f.ja, f.small, f.small_ja, f.badge] {
                unsafe {
                    let _ = DeleteObject(h.into());
                }
            }
        }
        let px = |v: i32| v * dpi / 96;
        let f = Fonts {
            dpi,
            ko: make_font(px(15), 400, w!("Malgun Gothic")),
            ja: make_font(px(15), 400, w!("Yu Gothic UI")),
            small: make_font(px(12), 400, w!("Malgun Gothic")),
            small_ja: make_font(px(12), 400, w!("Yu Gothic UI")),
            badge: make_font(px(12), 700, w!("Malgun Gothic")),
        };
        *c = Some(f);
        f
    })
}

/// 가나가 든 글자인가 (일본어 글꼴로 그린다)
fn has_kana(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}'))
}

/// 후보 줄의 글꼴: 가나가 있거나 일본어 후보(언어 표시에 ja)면 일본어 글꼴
fn item_font(f: &Fonts, text: &str, label: &str) -> HFONT {
    if has_kana(text) || label.split('+').any(|l| l == "ja") { f.ja } else { f.ko }
}

fn label_font(f: &Fonts, label: &str) -> HFONT {
    if has_kana(label) { f.small_ja } else { f.small }
}

fn extent(hdc: HDC, font: HFONT, s: &[u16]) -> SIZE {
    let mut sz = SIZE::default();
    unsafe {
        let old = SelectObject(hdc, font.into());
        let _ = GetTextExtentPoint32W(hdc, s, &mut sz);
        SelectObject(hdc, old);
    }
    sz
}

// --- 배치 ---

/// 창 안의 배치 (모두 픽셀, 창의 DPI 기준).
struct Layout {
    width: i32,
    height: i32,
    /// 바깥 여백 (줄 배경이 창 가장자리에서 떨어진 거리)
    margin: i32,
    row_h: i32,
    header_h: i32,
    footer_h: i32,
    /// 번호 / 후보 글자의 시작 x
    num_x: i32,
    text_x: i32,
    /// 조합 글자(맨 윗줄)의 왼쪽 위와 높이: 조합 창은 이 상자가 커서 자리에 오도록 놓인다
    header_x: i32,
    header_y: i32,
    header_th: i32,
    /// 언어 표시 칩의 좌우 여백, 높이
    chip_pad: i32,
    chip_h: i32,
    /// 알림 배지 한 변
    badge: i32,
}

fn layout(hwnd: HWND, list: &CandidateList, header: Option<&str>, toast: bool) -> Layout {
    let f = fonts(hwnd);
    let s = |v: i32| v * f.dpi / 96;
    let hdc = unsafe { GetDC(Some(hwnd)) };
    let margin = s(4);
    let chip_pad = s(6);
    let small_h = extent(hdc, f.small, &wide("9Ag")).cy;
    let chip_h = small_h + s(2);
    let num_w = extent(hdc, f.small, &wide("9")).cx;
    let num_x = margin + s(10);
    let text_x = num_x + num_w + s(9);
    let (mut w_main, mut w_lab, mut th) = (0, 0, extent(hdc, f.ko, &wide("가Ag")).cy);
    for (i, t) in list.items.iter().enumerate() {
        let lab = list.labels.get(i).map(String::as_str).unwrap_or("");
        let sz = extent(hdc, item_font(&f, t, lab), &wide(t));
        w_main = w_main.max(sz.cx);
        th = th.max(sz.cy);
        if !lab.is_empty() {
            w_lab = w_lab.max(extent(hdc, label_font(&f, lab), &wide(lab)).cx + chip_pad * 2);
        }
    }
    let row_h = th + s(10);
    let badge = if toast { small_h + s(4) } else { 0 };
    let header_x = if toast { margin + s(8) + badge + s(8) } else { margin + s(10) };
    let (mut header_w, mut header_th) = (0, 0);
    if let Some(h) = header {
        let sz = extent(hdc, if has_kana(h) { f.ja } else { f.ko }, &wide(h));
        header_w = sz.cx;
        header_th = sz.cy;
    }
    let header_h = if header.is_some() { header_th.max(th) + s(12) } else { 0 };
    let header_y = margin + (header_h - header_th) / 2;
    let footer_h = if list.pages > 1 { small_h + s(12) } else { 0 };
    let mut width = header_x + header_w + s(14) + margin;
    if !list.items.is_empty() {
        width = width.max(text_x + w_main + s(24) + w_lab + s(10) + margin);
    }
    if list.pages > 1 {
        let foot = extent(hdc, f.small, &wide(&footer_left(list))).cx + extent(hdc, f.small, &wide(FOOTER_RIGHT)).cx;
        width = width.max(num_x + foot + s(32) + s(10) + margin);
    }
    unsafe {
        ReleaseDC(Some(hwnd), hdc);
    }
    let height = margin * 2 + header_h + row_h * list.items.len() as i32 + footer_h;
    Layout {
        width,
        height,
        margin,
        row_h,
        header_h,
        footer_h,
        num_x,
        text_x,
        header_x,
        header_y,
        header_th,
        chip_pad,
        chip_h,
        badge,
    }
}

fn footer_left(list: &CandidateList) -> String {
    format!("{} / {}", list.page, list.pages)
}
const FOOTER_RIGHT: &str = "PgUp / PgDn";

// --- 그리기 ---

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            WIN.with(|w| {
                if let Ok(w) = w.try_borrow() {
                    paint_buffered(hwnd, hdc, &w);
                }
            });
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        // 배경은 WM_PAINT에서 메모리 DC에 함께 그린다 (깜빡임 방지)
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wparam.0 == TOAST_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TOAST_TIMER);
            }
            let still_toast = WIN.with(|w| w.try_borrow().map(|w| w.toast).unwrap_or(false));
            if still_toast {
                hide_local();
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 메모리 DC에 다 그린 뒤 한 번에 옮긴다.
fn paint_buffered(hwnd: HWND, hdc: HDC, w: &Win) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, rc.right, rc.bottom);
        let old = SelectObject(mem, bmp.into());
        paint(hwnd, mem, rc, w);
        let _ = BitBlt(hdc, 0, 0, rc.right, rc.bottom, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
    }
}

fn fill(hdc: HDC, r: RECT, c: COLORREF) {
    unsafe {
        let b = CreateSolidBrush(c);
        FillRect(hdc, &r, b);
        let _ = DeleteObject(b.into());
    }
}

/// 둥근 사각형 채우기 (테두리 없이)
fn fill_round(hdc: HDC, r: RECT, radius: i32, c: COLORREF) {
    unsafe {
        let b = CreateSolidBrush(c);
        let ob = SelectObject(hdc, b.into());
        let op = SelectObject(hdc, GetStockObject(NULL_PEN));
        // NULL_PEN이면 오른쪽·아래 1픽셀이 빠지므로 1 더한다
        let _ = RoundRect(hdc, r.left, r.top, r.right + 1, r.bottom + 1, radius * 2, radius * 2);
        SelectObject(hdc, op);
        SelectObject(hdc, ob);
        let _ = DeleteObject(b.into());
    }
}

fn text(hdc: HDC, font: HFONT, color: COLORREF, x: i32, y: i32, s: &[u16]) {
    unsafe {
        let old = SelectObject(hdc, font.into());
        SetTextColor(hdc, color);
        let _ = TextOutW(hdc, x, y, s);
        SelectObject(hdc, old);
    }
}

fn paint(hwnd: HWND, hdc: HDC, rc: RECT, w: &Win) {
    let t = theme();
    let f = fonts(hwnd);
    let s = |v: i32| v * f.dpi / 96;
    let list = &w.list;
    let header = w.header.as_deref();
    let l = layout(hwnd, list, header, w.toast);
    fill(hdc, rc, t.bg);
    if !w.rounded {
        unsafe {
            let b = CreateSolidBrush(t.border);
            FrameRect(hdc, &rc, b);
            let _ = DeleteObject(b.into());
        }
    }
    unsafe { SetBkMode(hdc, TRANSPARENT) };
    let mut y = l.margin;

    // 맨 윗줄: 조합 글자(밑줄) 또는 알림(배지 + 문구)
    if let Some(h) = header {
        let hw = wide(h);
        let font = if has_kana(h) { f.ja } else { f.ko };
        if w.toast {
            let by = y + (l.header_h - l.badge) / 2;
            let br = RECT { left: l.margin + s(8), top: by, right: l.margin + s(8) + l.badge, bottom: by + l.badge };
            match t.accent {
                Some(a) => fill_round(hdc, br, s(4), a),
                None => unsafe {
                    let b = CreateSolidBrush(t.fg);
                    FrameRect(hdc, &br, b);
                    let _ = DeleteObject(b.into());
                },
            }
            let bw = wide(&w.badge);
            let bsz = extent(hdc, f.badge, &bw);
            text(hdc, f.badge, t.on_accent, br.left + (l.badge - bsz.cx) / 2, br.top + (l.badge - bsz.cy) / 2, &bw);
        }
        text(hdc, font, t.fg, l.header_x, l.header_y, &hw);
        if !w.toast {
            let sz = extent(hdc, font, &hw);
            let ub = l.header_y + l.header_th;
            fill(hdc, RECT { left: l.header_x, top: ub - s(1).max(1), right: l.header_x + sz.cx, bottom: ub }, t.fg);
        }
        y += l.header_h;
        if !list.items.is_empty() {
            fill(hdc, RECT { left: 0, top: y - 1, right: rc.right, bottom: y }, t.border);
        }
    }

    // 후보 줄
    for (i, item) in list.items.iter().enumerate() {
        let lab = list.labels.get(i).map(String::as_str).unwrap_or("");
        let selected = i == list.selected;
        let row = RECT { left: l.margin, top: y + s(1), right: rc.right - l.margin, bottom: y + l.row_h - s(1) };
        let (fg, dim) = if selected { (t.sel_fg, t.sel_dim) } else { (t.fg, t.dim) };
        if selected {
            if t.accent.is_some() {
                fill_round(hdc, row, s(4), t.sel_bg);
            } else {
                fill(hdc, row, t.sel_bg);
            }
            if let Some(a) = t.accent {
                let bar_h = l.row_h / 2 - s(2);
                let top = y + (l.row_h - bar_h) / 2;
                fill_round(hdc, RECT { left: l.margin, top, right: l.margin + s(3), bottom: top + bar_h }, s(2).max(1), a);
            }
        }
        let font = item_font(&f, item, lab);
        let iw = wide(item);
        let isz = extent(hdc, font, &iw);
        if i < 9 {
            let nw = wide(&(i + 1).to_string());
            let nsz = extent(hdc, f.small, &nw);
            text(hdc, f.small, dim, l.num_x, y + (l.row_h - nsz.cy) / 2, &nw);
        }
        text(hdc, font, fg, l.text_x, y + (l.row_h - isz.cy) / 2, &iw);
        if !lab.is_empty() {
            let lf = label_font(&f, lab);
            let lw = wide(lab);
            let lsz = extent(hdc, lf, &lw);
            let right = rc.right - l.margin - s(6);
            let chip = RECT {
                left: right - lsz.cx - l.chip_pad * 2,
                top: y + (l.row_h - l.chip_h) / 2,
                right,
                bottom: y + (l.row_h - l.chip_h) / 2 + l.chip_h,
            };
            if let Some(c) = if selected { t.chip_sel } else { t.chip } {
                fill_round(hdc, chip, s(4), c);
            }
            text(hdc, lf, dim, chip.left + l.chip_pad, chip.top + (l.chip_h - lsz.cy) / 2, &lw);
        }
        y += l.row_h;
    }

    // 쪽 표시 (Page Up/Down, ↑↓로 넘긴다)
    if l.footer_h > 0 {
        fill(hdc, RECT { left: 0, top: y, right: rc.right, bottom: y + 1 }, t.border);
        let left = wide(&footer_left(list));
        let right = wide(FOOTER_RIGHT);
        let lsz = extent(hdc, f.small, &left);
        let rsz = extent(hdc, f.small, &right);
        let ty = y + (l.footer_h - lsz.cy) / 2;
        text(hdc, f.small, t.dim, l.num_x, ty, &left);
        text(hdc, f.small, t.dim, rc.right - l.margin - s(10) - rsz.cx, ty, &right);
    }
}

// --- 창 ---

fn ensure_window() -> Option<HWND> {
    if let Some(h) = WIN.with(|w| w.try_borrow().ok().and_then(|w| w.hwnd)) {
        // 소유자(follow_owner로 맞춘 앱의 팝업 창)가 닫히면 Windows가 소유된 이 창도 같이 없앤다
        // (Cubase 플러그인 검색창). 그때는 새로 만든다.
        if unsafe { IsWindow(Some(h)) }.as_bool() {
            return Some(h);
        }
        crate::debug_note("candwin destroyed with its owner: recreating");
    }
    let module = crate::module_handle();
    if !REGISTERED.swap(true, Ordering::SeqCst) {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: CLASS,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            ..Default::default()
        };
        unsafe { RegisterClassExW(&wc) };
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!(""),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .inspect_err(|e| crate::debug_note(&format!("candwin create failed {e:?}")))
    .ok()?;
    // Windows 11: 둥근 모서리 (그 전 Windows는 이 속성을 모르므로 실패하고 각진 창에 테두리를 직접 그린다)
    let pref = DWMWCP_ROUNDSMALL;
    let rounded = unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as *const _, std::mem::size_of_val(&pref) as u32)
    }
    .is_ok();
    crate::debug_note(&format!("candwin created {:?} rounded={rounded}", hwnd.0));
    WIN.with(|w| {
        if let Ok(mut w) = w.try_borrow_mut() {
            w.hwnd = Some(hwnd);
            w.rounded = rounded;
        }
    });
    Some(hwnd)
}

/// 후보창을 조합 영역(anchor, 화면 좌표) 바로 아래에 띄운다.
pub fn show(list: &CandidateList, anchor: RECT) {
    // 셸 검색창 등: 후보창 대행 프로세스(UIAccess)가 있으면 맡긴다 (tip/src/host.rs)
    #[cfg(not(candhost))]
    if crate::host::show(Some(list), None, anchor) {
        hide_local();
        return;
    }
    show_inner(list, None, anchor);
}

/// 조합 창: 조합 문자열(맨 윗줄)과 후보(있으면)를 커서(anchor) 아래에 띄운다. 조합을 입력칸 안에 넣을 수 없는 옛 입력칸용.
pub fn show_floating(preedit: &str, list: Option<&CandidateList>, anchor: RECT) {
    #[cfg(not(candhost))]
    if crate::host::show(list, Some(preedit), anchor) {
        hide_local();
        return;
    }
    let empty = CandidateList::default();
    show_inner(list.unwrap_or(&empty), Some(preedit), anchor);
}

/// 후보창의 소유자를 지금 입력 중인 창(포커스 창의 최상위 창)으로 맞춘다.
/// 작업 표시줄 검색창 같은 몰입형(immersive) 화면은 일반 창보다 위 층에 그려지므로, 소유자가 없으면
/// 맨 위(topmost)로 띄워도 그 뒤에 가려진다. 소유된 창은 소유자보다 위, 같은 층에 그려진다.
#[cfg(not(candhost))]
fn follow_owner(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
    let focus = unsafe { GetFocus() };
    if focus.0.is_null() {
        return;
    }
    let root = unsafe { GetAncestor(focus, GA_ROOT) };
    if root.0.is_null() || root == hwnd {
        return;
    }
    let current = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(root, &mut buf) } as usize;
    crate::debug_note(&format!("candwin owner root={:?} class={}", root.0, String::from_utf16_lossy(&buf[..n])));
    if current != root {
        #[cfg(target_pointer_width = "64")]
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, root.0 as isize);
        }
        #[cfg(target_pointer_width = "32")]
        unsafe {
            SetWindowLongW(hwnd, GWL_HWNDPARENT, root.0 as i32);
        }
    }
}

/// 커서(anchor) 아래에 짧은 알림을 잠깐 보인다 (한/영 키로 바꾼 입력 언어 고정 상태).
/// `badge`: 문구 앞 배지의 글자 (지금 언어: 가 / A / あ).
#[allow(dead_code)] // the candidate host (candhost/) does not show toasts
pub fn show_toast(text: &str, badge: &str, anchor: RECT) {
    WIN.with(|w| {
        if let Ok(mut w) = w.try_borrow_mut() {
            w.badge = badge.to_string();
        }
    });
    show_inner_mode(&CandidateList::default(), Some(text), anchor, true);
    WIN.with(|w| {
        if let Ok(w) = w.try_borrow() {
            if let Some(h) = w.hwnd {
                unsafe {
                    SetTimer(Some(h), TOAST_TIMER, TOAST_MS, None);
                }
            }
        }
    });
}

fn show_inner(list: &CandidateList, header: Option<&str>, anchor: RECT) {
    show_inner_mode(list, header, anchor, false);
}

fn show_inner_mode(list: &CandidateList, header: Option<&str>, anchor: RECT, toast: bool) {
    let Some(hwnd) = ensure_window() else { return };
    #[cfg(not(candhost))]
    follow_owner(hwnd);
    let rounded = WIN.with(|w| {
        w.try_borrow_mut()
            .map(|mut w| {
                w.list = list.clone();
                w.header = header.map(str::to_string);
                w.toast = toast;
                w.rounded
            })
            .unwrap_or(false)
    });
    if rounded {
        // 둥근 창의 테두리는 DWM이 그린다: 테마에 맞는 색으로 (테마는 쓰는 중에도 바뀔 수 있다)
        let border = theme().border;
        unsafe {
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const _ as *const _, 4);
        }
    }
    let l = layout(hwnd, list, header, toast);
    let (width, height) = (l.width, l.height);
    // 조합 창(header): 맨 윗줄의 조합 글자가 커서 자리에 겹쳐 보이게 (입력칸에 직접 치는 것처럼),
    // 후보는 그 아래로 이어진다. 글자 상자는 창 안 (header_x, header_y)에서 시작하고 높이는 header_th.
    let overlay = header.is_some() && !toast;
    let (mut x, mut y) = if overlay {
        let caret_mid = (anchor.top + anchor.bottom) / 2;
        (anchor.left - l.header_x, caret_mid - l.header_y - l.header_th / 2)
    } else {
        (anchor.left, anchor.bottom + 2)
    };
    // 화면 밖으로 나가지 않게 (아래 공간이 없으면: 후보창은 조합 위로, 조합 창은 화면 안으로 올린다)
    let mon = unsafe { MonitorFromRect(&anchor, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if unsafe { GetMonitorInfoW(mon, &mut mi) }.as_bool() {
        let wa = mi.rcWork;
        if y + height > wa.bottom {
            y = if overlay { wa.bottom - height } else { anchor.top - height - 2 };
        }
        x = x.min(wa.right - width).max(wa.left);
        y = y.max(wa.top);
    }
    unsafe {
        let r = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        crate::debug_note(&format!("candwin show at {x},{y} {width}x{height} -> {r:?} visible={}", IsWindowVisible(hwnd).as_bool()));
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn hide() {
    #[cfg(not(candhost))]
    crate::host::hide();
    hide_local();
}

fn hide_local() {
    WIN.with(|w| {
        if let Ok(w) = w.try_borrow() {
            if let Some(h) = w.hwnd {
                unsafe {
                    let _ = ShowWindow(h, SW_HIDE);
                }
            }
        }
    });
}

/// 화면 좌표 기준 점이 들어 있는 모니터 확인용 (미사용 경고 방지용 보조)
#[allow(dead_code)]
fn point_in(rc: &RECT, p: POINT) -> bool {
    p.x >= rc.left && p.x < rc.right && p.y >= rc.top && p.y < rc.bottom
}
