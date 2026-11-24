//! 사용자 설정 (`%LOCALAPPDATA%\multilang-ime\settings.json`). 설정 화면이 생기기 전까지는 이 파일을 직접 고친다.
//! 파일이 바뀌면 1초 안에 다시 읽는다. 없으면 기본값으로 만든다.
//!
//! ```json
//! { "ja_space": "commit", "languages": { "ko": true, "ja": true }, "disabled_apps": ["game.exe"] }
//! ```
//! - `languages`: 자동 판정·후보에 넣을 언어 (영어는 늘 켜짐).
//! - `disabled_apps`: IME를 쓰지 않을 프로그램 실행 파일 이름 (대소문자 무시, `.exe` 생략 가능).
//! - `focus_notice`: 언어를 고정해 쓸 때 입력칸에 들어가면 고정 상태를 커서 옆에 잠깐 보인다 (기본 켬).
//! - `ja_space`: 일본어 문맥의 Space.
//!   - `"commit"` (기본): 확정 + 전각 공백(U+3000)
//!   - `"continue"`: 확정하지 않고 조합을 이어 간다 (문장을 한 번에 변환). 확정은 구두점·Enter·Shift+Space.

use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum JaSpace {
    #[default]
    Commit,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Languages {
    pub ko: bool,
    pub ja: bool,
}

impl Default for Languages {
    fn default() -> Self {
        Self { ko: true, ja: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ja_space: JaSpace,
    pub languages: Languages,
    pub disabled_apps: Vec<String>,
    pub focus_notice: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { ja_space: JaSpace::default(), languages: Languages::default(), disabled_apps: Vec::new(), focus_notice: true }
    }
}

impl Settings {
    /// 이 프로그램(실행 파일 이름)에서 IME를 끄는가.
    pub fn app_disabled(&self, app: &str) -> bool {
        fn norm(s: &str) -> String {
            let s = s.trim().to_lowercase();
            s.strip_suffix(".exe").map(str::to_string).unwrap_or(s)
        }
        let app = norm(app);
        !app.is_empty() && self.disabled_apps.iter().any(|a| norm(a) == app)
    }
}

struct Cache {
    path: Option<PathBuf>,
    settings: Settings,
    mtime: Option<SystemTime>,
    checked: Option<Instant>,
}

static CACHE: LazyLock<Mutex<Cache>> =
    LazyLock::new(|| Mutex::new(Cache { path: None, settings: Settings::default(), mtime: None, checked: None }));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_notice_defaults_on() {
        // 예전 설정 파일(항목 없음)도 켜짐으로 읽는다
        let s: Settings = serde_json::from_str(r#"{ "ja_space": "commit" }"#).unwrap();
        assert!(s.focus_notice);
        let s: Settings = serde_json::from_str(r#"{ "focus_notice": false }"#).unwrap();
        assert!(!s.focus_notice);
    }
}
