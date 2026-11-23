//! 입력 언어 고정 (모든 앱 공통). 한/영 키로 자동 → 한국어 → English → 日本語 → 자동 순서로 바꾼다.
//!
//! 연달아 누르면(앞 누름에서 `QUICK` 안) 순서대로 돌고, 고정한 채 한동안 쓰다가 누르면 먼저 자동으로 돌아간다
//! (그다음 빠르게 누르면 한국어 → English → 日本語).
//! 고정하면 자동 판정을 끄고 그 언어로만 입력한다. English 고정은 IME가 키를 받지 않는다 (그대로 앱에).
//! 상태는 `%LOCALAPPDATA%\multilang-ime\lock.txt`(ko / en / ja, 없으면 자동)에 남아 백엔드를 다시 띄워도 이어진다.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use engine::detector::Lang;

struct State {
    path: Option<PathBuf>,
    lock: Option<Lang>,
    last_press: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State { path: None, lock: None, last_press: None });

/// 이 시간 안에 다시 누르면 "연달아 누름"으로 보고 순서대로 돈다.
pub const QUICK: Duration = Duration::from_millis(1500);

/// 한/영 키를 한 번 눌렀을 때의 다음 상태. quick: 앞 누름에서 QUICK 안에 다시 눌렀는가.
pub fn press(cur: Option<Lang>, quick: bool, ko_on: bool, ja_on: bool) -> Option<Lang> {
    if cur.is_some() && !quick {
        return None; // 한동안 고정해 쓰다가 누르면 먼저 자동으로
    }
    next(cur, ko_on, ja_on)
}

/// 누른 시각을 기록하고, 앞 누름에서 QUICK 안이었는지 돌려준다.
pub fn note_press() -> bool {
    let Ok(mut s) = STATE.lock() else { return false };
    let now = Instant::now();
    let quick = s.last_press.is_some_and(|t| now.duration_since(t) <= QUICK);
    s.last_press = Some(now);
    quick
}

fn parse(s: &str) -> Option<Lang> {
    match s.trim() {
        "ko" => Some(Lang::Ko),
        "en" => Some(Lang::En),
        "ja" => Some(Lang::Ja),
        _ => None,
    }
}

/// 저장된 고정 상태를 읽는다 (백엔드 시작 때).
pub fn init(path: PathBuf) {
    let lock = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t));
    if let Ok(mut s) = STATE.lock() {
        s.lock = lock;
        s.path = Some(path);
    }
    if let Some(l) = lock {
        crate::log(&format!("lock: {}", l.code()));
    }
}

pub fn get() -> Option<Lang> {
    STATE.lock().ok().and_then(|s| s.lock)
}

pub fn set(lock: Option<Lang>) {
    let Ok(mut s) = STATE.lock() else { return };
    s.lock = lock;
    if let Some(p) = &s.path {
        let _ = match lock {
            Some(l) => std::fs::write(p, l.code()),
            None => std::fs::remove_file(p),
        };
    }
    crate::log(&format!("lock: {}", lock.map(|l| l.code()).unwrap_or("auto")));
}

/// 다음 상태: 자동 → 한국어 → English → 日本語 → 자동 (설정에서 끈 언어는 건너뛴다).
pub fn next(cur: Option<Lang>, ko_on: bool, ja_on: bool) -> Option<Lang> {
    let order = [None, Some(Lang::Ko), Some(Lang::En), Some(Lang::Ja)];
    let usable = |l: &Option<Lang>| match l {
        Some(Lang::Ko) => ko_on,
        Some(Lang::Ja) => ja_on,
        _ => true,
    };
    let i = order.iter().position(|l| *l == cur).unwrap_or(0);
    (1..=order.len()).map(|k| order[(i + k) % order.len()]).find(usable).unwrap_or(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_order_and_skips() {
        assert_eq!(next(None, true, true), Some(Lang::Ko));
        assert_eq!(next(Some(Lang::Ko), true, true), Some(Lang::En));
        assert_eq!(next(Some(Lang::En), true, true), Some(Lang::Ja));
        assert_eq!(next(Some(Lang::Ja), true, true), None);
        // 일본어를 끄면 English 다음은 자동
        assert_eq!(next(Some(Lang::En), true, false), None);
        // 한국어를 끄면 자동 다음은 English
        assert_eq!(next(None, false, true), Some(Lang::En));
    }

    #[test]
    fn press_after_idle_returns_to_auto() {
        // 연달아 누르면 지금처럼 순환
        assert_eq!(press(None, true, true, true), Some(Lang::Ko));
        assert_eq!(press(Some(Lang::Ko), true, true, true), Some(Lang::En));
        assert_eq!(press(Some(Lang::Ja), true, true, true), None);
        // 고정해 쓰다가 한참 뒤 누르면 먼저 자동, 이어서 빠르게 누르면 한국어부터
        assert_eq!(press(Some(Lang::En), false, true, true), None);
        assert_eq!(press(None, true, true, true), Some(Lang::Ko));
        // 자동 상태에서 한참 뒤 누르면 한국어
        assert_eq!(press(None, false, true, true), Some(Lang::Ko));
    }
}
