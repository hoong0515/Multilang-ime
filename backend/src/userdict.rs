//! 사용자 사전 (`%LOCALAPPDATA%\multilang-ime\user_dict.json`): 친 키 → 변환.
//!
//! ```json
//! [ { "keys": "gh", "text": "GitHub" }, { "keys": "yoroshiku", "text": "よろしくお願いします" } ]
//! ```
//! - `keys`는 실제로 치는 키 그대로 (로마자, 두벌식 키, 영문). 대소문자는 가리지 않는다.
//! - 같은 키에 여러 변환을 둘 수 있다 (적은 순서대로 후보에 나온다).
//! - 후보창 맨 앞 "사전" 묶음에 나오고, 첫 Shift 탭이 그 후보를 고른다. 자동 변환은 바꾸지 않는다.
//! - 파일이 바뀌면 1초 안에 다시 읽는다 (설정 화면에서 고친다).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub keys: String,
    pub text: String,
}

struct Cache {
    path: Option<PathBuf>,
    map: HashMap<String, Vec<String>>,
    mtime: Option<SystemTime>,
    checked: Option<Instant>,
}

static CACHE: LazyLock<Mutex<Cache>> =
    LazyLock::new(|| Mutex::new(Cache { path: None, map: HashMap::new(), mtime: None, checked: None }));

pub fn init(path: PathBuf) {
    if let Ok(mut c) = CACHE.lock() {
        c.path = Some(path);
        c.checked = None;
        c.mtime = None;
    }
}

fn build(entries: &[Entry]) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for e in entries {
        let k = e.keys.trim().to_lowercase();
        if k.is_empty() || e.text.is_empty() {
            continue;
        }
        let v = map.entry(k).or_default();
        if !v.contains(&e.text) {
            v.push(e.text.clone());
        }
    }
    map
}

/// 키 문자열에 등록된 변환 (없으면 빈 목록).
pub fn lookup(keys: &str) -> Vec<String> {
    let Ok(mut c) = CACHE.lock() else { return Vec::new() };
    if !c.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
        c.checked = Some(Instant::now());
        if let Some(path) = c.path.clone() {
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            if mtime != c.mtime {
                c.mtime = mtime;
                let entries: Vec<Entry> = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| match serde_json::from_str(t.trim_start_matches('\u{feff}')) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            crate::log(&format!("user_dict: parse error {e}"));
                            None
                        }
                    })
                    .unwrap_or_default();
                c.map = build(&entries);
                crate::log(&format!("user_dict: {} keys", c.map.len()));
            }
        }
    }
    c.map.get(&keys.to_lowercase()).cloned().unwrap_or_default()
}

#[cfg(test)]
pub fn set_for_test(entries: &[(&str, &str)]) {
    let list: Vec<Entry> = entries.iter().map(|(k, t)| Entry { keys: k.to_string(), text: t.to_string() }).collect();
    if let Ok(mut c) = CACHE.lock() {
        c.path = None;
        c.checked = Some(Instant::now() + Duration::from_secs(3600));
        let add = build(&list);
        for (k, v) in add {
            c.map.insert(k, v);
        }
    }
}
