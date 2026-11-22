//! 사용자 설정 (`%LOCALAPPDATA%\multilang-ime\settings.json`). 설정 화면이 생기기 전까지는 이 파일을 직접 고친다.
//! 파일이 바뀌면 1초 안에 다시 읽는다. 없으면 기본값으로 만든다.
//!
//! ```json
//! { "ja_space": "commit" }
//! ```
//! - `ja_space`: 일본어 문맥의 Space.
//!   - `"commit"` (기본): 확정 + 전각 공백(U+3000)
//!   - `"continue"`: 확정하지 않고 조합을 이어 간다 (문장을 한 번에 변환). 확정은 구두점·Enter·Shift+Space.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum JaSpace {
    #[default]
    Commit,
    Continue,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub ja_space: JaSpace,
}

struct Cache {
    path: Option<PathBuf>,
    settings: Settings,
    mtime: Option<SystemTime>,
    checked: Option<Instant>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { path: None, settings: Settings { ja_space: JaSpace::Commit }, mtime: None, checked: None });

/// 설정 파일 위치를 정하고 읽는다 (없으면 기본값으로 만든다). 백엔드 시작 때 한 번.
pub fn init(path: PathBuf) {
    if !path.exists() {
        if let Ok(t) = serde_json::to_string_pretty(&Settings::default()) {
            let _ = std::fs::write(&path, t);
        }
    }
    if let Ok(mut c) = CACHE.lock() {
        c.path = Some(path);
        c.checked = None;
    }
}

/// 지금 설정 (파일이 바뀌었으면 다시 읽는다, 확인은 1초에 한 번).
pub fn current() -> Settings {
    let Ok(mut c) = CACHE.lock() else { return Settings::default() };
    if c.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
        return c.settings.clone();
    }
    c.checked = Some(Instant::now());
    let Some(path) = c.path.clone() else { return c.settings.clone() };
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if mtime != c.mtime {
        c.mtime = mtime;
        match std::fs::read_to_string(&path).map(|t| serde_json::from_str::<Settings>(t.trim_start_matches('\u{feff}'))) {
            Ok(Ok(s)) => {
                crate::log(&format!("settings: {s:?}"));
                c.settings = s;
            }
            Ok(Err(e)) => crate::log(&format!("settings: parse error {e}")),
            Err(_) => {}
        }
    }
    c.settings.clone()
}
