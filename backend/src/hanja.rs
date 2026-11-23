//! 한국어 발음 → 한자 후보 (단어 단위).
//!
//! 사전: libhangul `hanja.txt` (BSD-3-Clause, "읽기:한자:뜻" 한 줄에 하나, 읽기마다 자주 쓰는 순).
//! 고른 한자는 읽기별로 횟수를 세어 `hanja_learn.json`에 저장하고 다음에 위로 올린다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static DICT: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);
static LEARN: Mutex<Option<(PathBuf, HashMap<String, HashMap<String, u32>>)>> = Mutex::new(None);

fn is_syllable(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

fn is_hanja(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3400}'..='\u{4DBF}').contains(&c) || ('\u{F900}'..='\u{FAFF}').contains(&c)
}

/// 사전을 읽는다 (백엔드 시작 때 별도 스레드에서). 읽기와 한자가 글자 수가 같은 항목만.
pub fn load(dict: &Path, learn: PathBuf) {
    if let Ok(text) = std::fs::read_to_string(dict) {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut it = line.split(':');
            let (Some(r), Some(h)) = (it.next(), it.next()) else { continue };
            if r.is_empty() || !r.chars().all(is_syllable) || !h.chars().all(is_hanja) || r.chars().count() != h.chars().count() {
                continue;
            }
            let v = map.entry(r.to_string()).or_default();
            if !v.iter().any(|x| x == h) {
                v.push(h.to_string());
            }
        }
        crate::log(&format!("hanja: {} readings", map.len()));
        if let Ok(mut d) = DICT.lock() {
            *d = Some(map);
        }
    } else {
        crate::log("hanja: dictionary not found");
    }
    let learned = std::fs::read_to_string(&learn).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if let Ok(mut l) = LEARN.lock() {
        *l = Some((learn, learned));
    }
}

/// 읽기의 한자 후보 (학습한 것 먼저). 사전이 아직 없으면 빈 목록.
pub fn candidates(reading: &str, max: usize) -> Vec<String> {
    let Some(mut out) = DICT.lock().ok().and_then(|d| d.as_ref().and_then(|d| d.get(reading).cloned())) else {
        return Vec::new();
    };
    if let Ok(l) = LEARN.lock() {
        if let Some(counts) = l.as_ref().and_then(|(_, m)| m.get(reading)) {
            // 안정 정렬: 학습 횟수 많은 순, 같으면 사전 순서
            out.sort_by_key(|h| std::cmp::Reverse(counts.get(h).copied().unwrap_or(0)));
        }
    }
    out.truncate(max);
    out
}

/// 한자 선택 학습을 지운다.
pub fn clear_learning() {
    if let Ok(mut l) = LEARN.lock() {
        if let Some((path, m)) = l.as_mut() {
            m.clear();
            let _ = std::fs::write(path, "{}");
        }
    }
}

/// 한자를 골랐다: 학습하고 저장한다.
pub fn learn(reading: &str, hanja: &str) {
    if let Ok(mut l) = LEARN.lock() {
        if let Some((path, m)) = l.as_mut() {
            *m.entry(reading.to_string()).or_default().entry(hanja.to_string()).or_default() += 1;
            if let Ok(t) = serde_json::to_string(m) {
                let _ = std::fs::write(path, t);
            }
        }
    }
}

#[cfg(test)]
pub fn load_for_test(entries: &[(&str, &[&str])]) {
    // 테스트끼리 병렬로 돌므로 덮어쓰지 않고 더한다
    if let Ok(mut d) = DICT.lock() {
        let map = d.get_or_insert_with(HashMap::new);
        for (r, hs) in entries {
            map.insert(r.to_string(), hs.iter().map(|s| s.to_string()).collect());
        }
    }
    if let Ok(mut l) = LEARN.lock() {
        if l.is_none() {
            *l = Some((std::env::temp_dir().join("multilang-ime-test-hanja.json"), HashMap::new()));
        }
    }
}
