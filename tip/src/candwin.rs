//! 후보창: 조합 바로 아래에 뜨는, 포커스를 가져가지 않는 팝업.
//!
//! 앱의 UI 스레드에서 TIP가 직접 그린다 (스레드마다 창 하나). 게임 등 UI-less 앱 지원은 M5.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use ipc::CandidateList;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
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
}

thread_local! {
    static WIN: RefCell<Win> = RefCell::new(Win::default());
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn lines(list: &CandidateList, header: Option<&str>) -> Vec<(Vec<u16>, Vec<u16>)> {
    let mut out: Vec<(Vec<u16>, Vec<u16>)> = header.map(|h| (wide(h), Vec::new())).into_iter().collect();
    out.extend(list.items.iter().enumerate().map(|(i, t)| {
        let num = if i < 9 { format!("{} ", i + 1) } else { "  ".to_string() };
        let lab = list.labels.get(i).cloned().unwrap_or_default();
        (wide(&format!("{num}{t}")), wide(&lab))
    }));
    // 여러 쪽이면 맨 아래에 쪽 표시 (Page Up/Down, ↑↓로 넘긴다)
    if list.pages > 1 {
        out.push((wide(&format!("◀ {} / {} ▶", list.page, list.pages)), wide("PgUp/PgDn")));
    }
    out
}

fn font(hwnd: HWND) -> HFONT {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let height = -(15 * dpi / 96);
    unsafe {
        CreateFontW(height, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Malgun Gothic"))
    }
}

/// 창 크기 계산 (가로: 가장 긴 줄 + 언어 표시, 세로: 줄 수).
fn measure(hwnd: HWND, list: &CandidateList, header: Option<&str>) -> (i32, i32, i32) {
    let hdc = unsafe { GetDC(Some(hwnd)) };
    let f = font(hwnd);
    let old = unsafe { SelectObject(hdc, f.into()) };
    let (mut w_main, mut w_lab, mut h_line) = (0, 0, 0);
    for (main, lab) in lines(list, header) {
        let mut sz = SIZE::default();
        unsafe {
            let _ = GetTextExtentPoint32W(hdc, &main, &mut sz);
        }
        w_main = w_main.max(sz.cx);
        h_line = h_line.max(sz.cy);
        unsafe {
            let _ = GetTextExtentPoint32W(hdc, &lab, &mut sz);
        }
        w_lab = w_lab.max(sz.cx);
    }
    unsafe {
        SelectObject(hdc, old);
        let _ = DeleteObject(f.into());
        ReleaseDC(Some(hwnd), hdc);
    }
    let pad = h_line / 3;
    (w_main + w_lab + pad * 5, h_line + pad, pad)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            WIN.with(|w| {
                if let Ok(w) = w.try_borrow() {
                    paint(hwnd, hdc, &w.list, w.header.as_deref());
                }
            });
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint(hwnd: HWND, hdc: HDC, list: &CandidateList, header: Option<&str>) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
        FillRect(hdc, &rc, GetSysColorBrush(COLOR_WINDOW));
        FrameRect(hdc, &rc, GetSysColorBrush(COLOR_BTNSHADOW));
    }
    let (_, line_h, pad) = measure(hwnd, list, header);
    let skip = header.is_some() as usize;
    let f = font(hwnd);
    let old = unsafe { SelectObject(hdc, f.into()) };
    unsafe { SetBkMode(hdc, TRANSPARENT) };
    for (i, (main, lab)) in lines(list, header).iter().enumerate() {
        let y = pad / 2 + i as i32 * line_h;
        let selected = i >= skip && i - skip == list.selected && !list.items.is_empty();
        let footer = list.pages > 1 && i == skip + list.items.len();
        if footer {
            // 쪽 표시 줄: 위에 구분선, 흐린 글자
            unsafe {
                let d = RECT { left: 1, top: y, right: rc.right - 1, bottom: y + 1 };
                FillRect(hdc, &d, GetSysColorBrush(COLOR_BTNSHADOW));
                let dim = COLORREF(GetSysColor(COLOR_GRAYTEXT));
                SetTextColor(hdc, dim);
                let _ = TextOutW(hdc, pad, y + pad / 2, main);
                let mut sz = SIZE::default();
                let _ = GetTextExtentPoint32W(hdc, lab, &mut sz);
                let _ = TextOutW(hdc, rc.right - pad - sz.cx, y + pad / 2, lab);
            }
            continue;
        }
        if i == 0 && skip == 1 {
            // 조합 줄: 밑줄을 긋고, 후보가 있으면 아래에 구분선
            unsafe {
                SetTextColor(hdc, COLORREF(GetSysColor(COLOR_WINDOWTEXT)));
                let _ = TextOutW(hdc, pad, y + pad / 2, main);
                let mut sz = SIZE::default();
                let _ = GetTextExtentPoint32W(hdc, main, &mut sz);
                let u = RECT { left: pad, top: y + pad / 2 + sz.cy - 1, right: pad + sz.cx, bottom: y + pad / 2 + sz.cy };
                FillRect(hdc, &u, GetSysColorBrush(COLOR_WINDOWTEXT));
                if !list.items.is_empty() {
                    let d = RECT { left: 1, top: y + line_h - 1, right: rc.right - 1, bottom: y + line_h };
                    FillRect(hdc, &d, GetSysColorBrush(COLOR_BTNSHADOW));
                }
            }
            continue;
        }
        if selected {
            let r = RECT { left: 1, top: y, right: rc.right - 1, bottom: y + line_h };
            unsafe { FillRect(hdc, &r, GetSysColorBrush(COLOR_HIGHLIGHT)) };
        }
        let (fg, dim) = if selected {
            let c = COLORREF(unsafe { GetSysColor(COLOR_HIGHLIGHTTEXT) });
            (c, c)
        } else {
            (COLORREF(unsafe { GetSysColor(COLOR_WINDOWTEXT) }), COLORREF(unsafe { GetSysColor(COLOR_GRAYTEXT) }))
        };
        let mut sz = SIZE::default();
        unsafe {
            SetTextColor(hdc, fg);
            let _ = TextOutW(hdc, pad, y + pad / 2, main);
            let _ = GetTextExtentPoint32W(hdc, lab, &mut sz);
            SetTextColor(hdc, dim);
            let _ = TextOutW(hdc, rc.right - pad - sz.cx, y + pad / 2, lab);
        }
    }
    unsafe {
        SelectObject(hdc, old);
        let _ = DeleteObject(f.into());
    }
}

fn ensure_window() -> Option<HWND> {
    if let Some(h) = WIN.with(|w| w.try_borrow().ok().and_then(|w| w.hwnd)) {
        return Some(h);
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
    crate::debug_note(&format!("candwin created {:?}", hwnd.0));
    WIN.with(|w| {
        if let Ok(mut w) = w.try_borrow_mut() {
            w.hwnd = Some(hwnd);
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

fn show_inner(list: &CandidateList, header: Option<&str>, anchor: RECT) {
    let Some(hwnd) = ensure_window() else { return };
    #[cfg(not(candhost))]
    follow_owner(hwnd);
    WIN.with(|w| {
        if let Ok(mut w) = w.try_borrow_mut() {
            w.list = list.clone();
            w.header = header.map(str::to_string);
        }
    });
    let (width, line_h, pad) = measure(hwnd, list, header);
    let rows = list.items.len() + header.is_some() as usize + (list.pages > 1) as usize;
    let height = line_h * rows as i32 + pad;
    // 조합 창(header): 맨 윗줄의 조합 글자가 커서 자리에 겹쳐 보이게 (입력칸에 직접 치는 것처럼),
    // 후보는 그 아래로 이어진다. 글자 상자는 창 안 (pad, pad)에서 시작하고 높이는 line_h - pad.
    let overlay = header.is_some();
    let (mut x, mut y) = if overlay {
        let caret_mid = (anchor.top + anchor.bottom) / 2;
        (anchor.left - pad, caret_mid - pad - (line_h - pad) / 2)
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
        let _ = InvalidateRect(Some(hwnd), None, true);
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
