//! 사용자 사전 (`%LOCALAPPDATA%\multilang-ime\user_dict.json`): 친 키 → 변환.
//! 숨긴 후보 (`hidden.json`): 친 키 → 후보창에 다시 보이지 않을 변환. 형식은 같다.
//!
//! ```json
//! [ { "keys": "gh", "text": "GitHub" }, { "keys": "yoroshiku", "text": "よろしくお願いします" } ]
//! ```
//! - `keys`는 실제로 치는 키 그대로 (로마자, 두벌식 키, 영문). 대소문자는 가리지 않는다.
//! - 같은 키에 여러 변환을 둘 수 있다 (적은 순서대로 후보에 나온다).
//! - 사전은 후보창 맨 앞 "사전" 묶음에 나오고, 첫 Shift 탭이 그 후보를 고른다. 자동 변환은 바꾸지 않는다.
//! - 후보창에서 Ctrl+Enter로 사전 맨 앞에 넣고, Ctrl+Delete로 숨긴다 (사전 후보면 사전에서 뺀다).
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

/// 파일 하나 (사전 또는 숨긴 후보)의 캐시.
pub struct List {
    name: &'static str,
    path: Option<PathBuf>,
    map: HashMap<String, Vec<String>>,
    mtime: Option<SystemTime>,
    checked: Option<Instant>,
}

impl List {
    fn new(name: &'static str) -> Self {
        List { name, path: None, map: HashMap::new(), mtime: None, checked: None }
    }

    fn init(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.map.clear();
        self.checked = None;
        self.mtime = None;
    }

    fn read_file(&self) -> Vec<Entry> {
        let Some(path) = &self.path else { return Vec::new() };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| match serde_json::from_str(t.trim_start_matches('\u{feff}')) {
                Ok(v) => Some(v),
                Err(e) => {
                    crate::log(&format!("{}: parse error {e}", self.name));
                    None
                }
            })
            .unwrap_or_default()
    }

    fn lookup(&mut self, keys: &str) -> Vec<String> {
        if !self.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            self.checked = Some(Instant::now());
            if let Some(path) = self.path.clone() {
                let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                if mtime != self.mtime {
                    self.mtime = mtime;
                    self.map = build(&self.read_file());
                    crate::log(&format!("{}: {} keys", self.name, self.map.len()));
                }
            }
        }
        self.map.get(&keys.to_lowercase()).cloned().unwrap_or_default()
    }

    /// 파일을 고쳐 쓰고 캐시를 바로 갱신한다. 파일 경로가 없으면 캐시만.
    fn edit(&mut self, f: impl FnOnce(&mut Vec<Entry>) -> bool) -> std::io::Result<bool> {
        let mut entries = self.read_file();
        if !f(&mut entries) {
            return Ok(false);
        }
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, serde_json::to_string_pretty(&entries).map_err(std::io::Error::other)?)?;
            std::fs::rename(&tmp, path)?;
            self.mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        }
        self.map = build(&entries);
        self.checked = Some(Instant::now());
        Ok(true)
    }

    /// 키의 첫 변환으로 넣는다 (이미 있으면 맨 앞으로 옮긴다).
    fn add_front(&mut self, keys: &str, text: &str) -> std::io::Result<bool> {
        let k = keys.trim().to_lowercase();
        if k.is_empty() || text.is_empty() {
            return Ok(false);
        }
        self.edit(|v| {
            v.retain(|e| !(e.keys.trim().to_lowercase() == k && e.text == text));
            let at = v.iter().position(|e| e.keys.trim().to_lowercase() == k).unwrap_or(v.len());
            v.insert(at, Entry { keys: keys.trim().to_string(), text: text.to_string() });
            true
        })
    }

    fn remove(&mut self, keys: &str, text: &str) -> std::io::Result<bool> {
        let k = keys.trim().to_lowercase();
        self.edit(|v| {
            let n = v.len();
            v.retain(|e| !(e.keys.trim().to_lowercase() == k && e.text == text));
            v.len() != n
        })
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

static DICT: LazyLock<Mutex<List>> = LazyLock::new(|| Mutex::new(List::new("user_dict")));
static HIDDEN: LazyLock<Mutex<List>> = LazyLock::new(|| Mutex::new(List::new("hidden")));

pub fn init(path: PathBuf) {
    if let Ok(mut c) = DICT.lock() {
        c.init(path);
    }
}

pub fn init_hidden(path: PathBuf) {
    if let Ok(mut c) = HIDDEN.lock() {
        c.init(path);
    }
}

/// 키 문자열에 등록된 변환 (없으면 빈 목록).
pub fn lookup(keys: &str) -> Vec<String> {
    DICT.lock().map(|mut c| c.lookup(keys)).unwrap_or_default()
}

/// 후보창에서 숨긴 변환.
pub fn hidden(keys: &str) -> Vec<String> {
    HIDDEN.lock().map(|mut c| c.lookup(keys)).unwrap_or_default()
}

fn logged(what: &str, r: std::io::Result<bool>) -> bool {
    match r {
        Ok(changed) => changed,
        Err(e) => {
            crate::log(&format!("{what}: {e}"));
            false
        }
    }
}

/// 사전 맨 앞에 넣는다 (Ctrl+Enter). 숨겨 둔 것이었으면 숨김도 푼다.
pub fn add(keys: &str, text: &str) -> bool {
    if let Ok(mut h) = HIDDEN.lock() {
        logged("hidden", h.remove(keys, text));
    }
    DICT.lock().map(|mut c| logged("user_dict", c.add_front(keys, text))).unwrap_or(false)
}

/// 사전에서 뺀다 (사전 후보에서 Ctrl+Delete).
pub fn remove(keys: &str, text: &str) -> bool {
    DICT.lock().map(|mut c| logged("user_dict", c.remove(keys, text))).unwrap_or(false)
}

/// 후보창에 다시 보이지 않게 한다 (Ctrl+Delete).
pub fn hide(keys: &str, text: &str) -> bool {
    HIDDEN.lock().map(|mut c| logged("hidden", c.add_front(keys, text))).unwrap_or(false)
}

#[cfg(test)]
pub fn set_for_test(entries: &[(&str, &str)]) {
    let list: Vec<Entry> = entries.iter().map(|(k, t)| Entry { keys: k.to_string(), text: t.to_string() }).collect();
    if let Ok(mut c) = DICT.lock() {
        c.path = None;
        c.checked = Some(Instant::now() + Duration::from_secs(3600));
        let add = build(&list);
        for (k, v) in add {
            c.map.insert(k, v);
        }
    }
}

#[cfg(test)]
pub fn hidden_for_test(path: PathBuf) {
    init_hidden(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("multilang-ime-test-{}-{name}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn add_front_remove_and_file() {
        let p = temp("dict");
        let mut l = List::new("t");
        l.init(p.clone());
        assert!(l.add_front("yrsk", "宜しく").unwrap());
        assert!(l.add_front("YRSK", "よろしくお願いします").unwrap());
        assert_eq!(l.lookup("yrsk"), vec!["よろしくお願いします", "宜しく"]);
        // 이미 있는 것을 다시 넣으면 맨 앞으로
        assert!(l.add_front("yrsk", "宜しく").unwrap());
        assert_eq!(l.lookup("yrsk"), vec!["宜しく", "よろしくお願いします"]);
        // 파일에서 다시 읽어도 같다
        let mut l2 = List::new("t");
        l2.init(p.clone());
        assert_eq!(l2.lookup("Yrsk"), vec!["宜しく", "よろしくお願いします"]);
        assert!(l.remove("yrsk", "宜しく").unwrap());
        assert!(!l.remove("yrsk", "宜しく").unwrap());
        assert_eq!(l.lookup("yrsk"), vec!["よろしくお願いします"]);
        let _ = std::fs::remove_file(&p);
    }
}
