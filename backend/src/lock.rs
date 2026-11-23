//! 입력 언어 고정 (모든 앱 공통). 한/영 키로 자동 → 한국어 → English → 日本語 → 자동 순서로 바꾼다.
//!
//! 고정하면 자동 판정을 끄고 그 언어로만 입력한다. English 고정은 IME가 키를 받지 않는다 (그대로 앱에).
//! 상태는 `%LOCALAPPDATA%\multilang-ime\lock.txt`(ko / en / ja, 없으면 자동)에 남아 백엔드를 다시 띄워도 이어진다.

use std::path::PathBuf;
use std::sync::Mutex;

use engine::detector::Lang;

struct State {
    path: Option<PathBuf>,
    lock: Option<Lang>,
}

static STATE: Mutex<State> = Mutex::new(State { path: None, lock: None });

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
}
