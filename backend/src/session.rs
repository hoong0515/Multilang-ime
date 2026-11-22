//! 연결(TIP 인스턴스) 하나의 조합 상태와 키 처리.
//!
//! 키 설계 (docs/v1-tsf-frontend.md):
//! - 왼쪽 Shift 탭: 후보창 열기(2위 선택) / 다음 후보
//! - 숫자 1~5: 후보창이 열려 있으면 그 후보를 고르고 창을 닫는다 (확정은 Space·Enter)
//! - Space: 확정 + 공백, Enter: 확정
//! - Backspace: 후보창이 열려 있으면 닫기(자동 결과로), 고른 후보가 있으면 해제, 아니면 키 하나 지우기
//! - Esc: 후보창이 열려 있으면 닫기, 아니면 친 키 그대로 보기 → 한 번 더 누르면 조합 취소
//! - 후보창이 열린 채 글자를 치면 고른 후보로 확정하고 새 조합을 시작한다
//!
//! 확정한 형태는 학습 기록(여러 앱이 공유, 파일에 저장)에 쌓여 다음 판정에 반영된다.

use std::path::PathBuf;

use engine::detector::{Candidate, Detector, Lang, SharedHistory};
use engine::model::Models;
use ipc::{CandidateList, Key, PreeditSegment, Response};

const MAX_CANDIDATES: usize = 9;
const PICK_KEYS: u16 = 5; // 숫자 1~5

pub struct Session<'m> {
    det: Detector<'m>,
    keys: String,                 // 아직 확정하지 않은 친 키 그대로
    raw: bool,                    // Esc로 "친 키 그대로" 보기 중
    prev: Option<Lang>,           // 직전에 확정한 언어 (문맥)
    run: usize,                   // 같은 언어로 이어서 확정한 단어 수 (언어 가중치)
    open: Option<(Vec<Candidate>, usize)>, // 후보창: 후보 목록과 선택 위치
    pinned: Option<Candidate>,    // 숫자로 고른 후보 (확정 전까지 표시)
    history: SharedHistory,
    history_path: Option<PathBuf>,
}

fn is_letter(k: &Key) -> bool {
    (0x41..=0x5A).contains(&k.vk) && !k.ctrl && !k.alt
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

fn digit(k: &Key) -> Option<usize> {
    if !k.ctrl && !k.alt && (0x31..0x31 + PICK_KEYS).contains(&k.vk) {
        Some((k.vk - 0x31) as usize)
    } else {
        None
    }
}

const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;

fn label(c: &Candidate) -> String {
    c.form.iter().map(|(l, _)| l.code()).collect::<Vec<_>>().join("+")
}

fn segments(c: &Candidate, focused: bool, ambiguous: bool) -> Vec<PreeditSegment> {
    c.segments
        .iter()
        .map(|s| PreeditSegment {
            text: s.text.clone(),
            lang: s.lang.map(|l| l.code()).unwrap_or("other").into(),
            ambiguous,
            focused,
        })
        .collect()
}

impl<'m> Session<'m> {
    pub fn new(models: &'m Models, history: SharedHistory, history_path: Option<PathBuf>) -> Self {
        let mut det = Detector::new(models, vec![Lang::En, Lang::Ko]);
        det.history = Some(history.clone());
        Self {
            det,
            keys: String::new(),
            raw: false,
            prev: None,
            run: 0,
            open: None,
            pinned: None,
            history,
            history_path,
        }
    }

    fn composing(&self) -> bool {
        !self.keys.is_empty()
    }

    pub fn test(&self, k: &Key) -> Response {
        let eat = Response { eaten: true, ..Default::default() };
        if is_letter(k) {
            return eat;
        }
        if !self.composing() || is_modifier(k.vk) {
            return Response::default();
        }
        if let (Some(d), Some((list, _))) = (digit(k), &self.open) {
            if d < list.len() {
                return eat;
            }
        }
        match k.vk {
            VK_BACK | VK_SPACE | VK_RETURN | VK_ESCAPE => eat,
            _ => Response { flush_first: true, ..Default::default() },
        }
    }

    pub fn key(&mut self, k: &Key) -> Response {
        if is_letter(k) {
            let c = (k.vk as u8) as char;
            let ch = if k.shift ^ k.caps { c } else { c.to_ascii_lowercase() };
            // 후보창이 열린 채 글자를 치면 고른 후보로 확정하고 새 조합을 시작한다
            let committed = if self.open.is_some() { self.commit("").commit } else { None };
            self.pinned = None;
            self.keys.push(ch);
            let mut r = self.respond();
            r.commit = committed;
            return r;
        }
        if !self.composing() {
            return Response::default();
        }
        if let Some(d) = digit(k) {
            if let Some((list, _)) = &self.open {
                if let Some(c) = list.get(d) {
                    self.pinned = Some(c.clone());
                    self.open = None;
                    return self.respond();
                }
            }
        }
        match k.vk {
            VK_BACK => {
                if self.open.is_some() {
                    self.open = None;
                } else if self.pinned.is_some() {
                    self.pinned = None;
                } else {
                    self.keys.pop();
                    if self.keys.is_empty() {
                        self.raw = false;
                    }
                }
                self.respond()
            }
            VK_SPACE => self.commit(" "),
            VK_RETURN => self.commit(""),
            VK_ESCAPE => {
                if self.open.is_some() || self.pinned.is_some() {
                    self.open = None;
                    self.pinned = None;
                } else if self.raw {
                    self.clear();
                } else {
                    self.raw = true;
                }
                self.respond()
            }
            _ => Response::default(),
        }
    }

    /// 왼쪽 Shift 탭: 후보창을 열거나 다음 후보로.
    pub fn shift_tap(&mut self) -> Response {
        if !self.composing() {
            return Response::default();
        }
        match &mut self.open {
            Some((list, sel)) => *sel = (*sel + 1) % list.len().max(1),
            None => {
                let list = self.candidate_list();
                if list.is_empty() {
                    return self.respond();
                }
                let sel = if list.len() > 1 { 1 } else { 0 };
                self.raw = false;
                self.pinned = None;
                self.open = Some((list, sel));
            }
        }
        self.respond()
    }

    pub fn flush(&mut self) -> Response {
        if self.composing() { self.commit("") } else { Response::default() }
    }

    pub fn reset(&mut self) -> Response {
        self.clear();
        Response::default()
    }

    fn clear(&mut self) {
        self.keys.clear();
        self.raw = false;
        self.open = None;
        self.pinned = None;
    }

    /// 현재 키 입력의 후보 (텍스트가 같은 후보는 하나만, 최대 9개).
    fn candidate_list(&self) -> Vec<Candidate> {
        let dec = self.det.decide(&self.keys, self.prev);
        let Some(t) = dec.tokens.first() else { return Vec::new() };
        let mut out: Vec<Candidate> = Vec::new();
        for c in std::iter::once(&t.chosen).chain(t.ranked.iter()) {
            if out.len() >= MAX_CANDIDATES {
                break;
            }
            if !out.iter().any(|o| o.text() == c.text()) {
                out.push(c.clone());
            }
        }
        out
    }

    /// 지금 표시할 후보: 후보창 선택 > 숫자로 고른 후보 > 자동 판정. (raw는 None)
    fn current(&self) -> Option<(Candidate, bool, bool)> {
        if let Some((list, sel)) = &self.open {
            return list.get(*sel).map(|c| (c.clone(), true, false));
        }
        if let Some(c) = &self.pinned {
            return Some((c.clone(), false, false));
        }
        let dec = self.det.decide(&self.keys, self.prev);
        let t = dec.tokens.first()?;
        Some((t.chosen.clone(), false, t.margin() < self.det.params().ambiguous))
    }

    fn respond(&self) -> Response {
        let mut r = Response { eaten: true, ..Default::default() };
        if self.keys.is_empty() {
            return r;
        }
        if self.raw {
            r.preedit = vec![PreeditSegment { text: self.keys.clone(), lang: "raw".into(), ambiguous: false, focused: false }];
            return r;
        }
        if let Some((c, focused, ambiguous)) = self.current() {
            r.preedit = segments(&c, focused, ambiguous);
        }
        if let Some((list, sel)) = &self.open {
            r.candidates = Some(CandidateList {
                items: list.iter().map(|c| c.text()).collect(),
                labels: list.iter().map(label).collect(),
                selected: *sel,
            });
        }
        r
    }

    fn commit(&mut self, tail: &str) -> Response {
        let mut text;
        if self.raw {
            text = self.keys.clone();
        } else if let Some((c, _, _)) = self.current() {
            text = c.text();
            // 학습: 확정한 형태를 기록하고 저장한다
            if let Ok(mut h) = self.history.lock() {
                h.record(&self.keys, &c.form);
                if let Some(p) = &self.history_path {
                    let _ = h.save(p);
                }
            }
            let l = c.end();
            self.run = if Some(l) == self.prev { self.run + 1 } else { 1 };
            self.prev = Some(l);
            self.det.set_language_run(self.run);
        } else {
            text = self.keys.clone();
        }
        text.push_str(tail);
        self.clear();
        Response { eaten: true, commit: Some(text), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use engine::detector::History;

    fn models() -> Option<Models> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/engine_model.json");
        Models::load(&p).ok()
    }

    fn session(m: &Models) -> Session<'_> {
        Session::new(m, Arc::new(Mutex::new(History::default())), None)
    }

    fn key(vk: u16, shift: bool) -> Key {
        Key { vk, shift, caps: false, ctrl: false, alt: false }
    }

    fn type_keys(s: &mut Session, text: &str) -> Response {
        let mut r = Response::default();
        for ch in text.chars() {
            r = s.key(&key(ch.to_ascii_uppercase() as u16, ch.is_ascii_uppercase()));
        }
        r
    }

    #[test]
    fn korean_and_english() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        assert_eq!(type_keys(&mut s, "dkssudgktpdy").preedit_text(), "안녕하세요");
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("안녕하세요 "));
        assert_eq!(type_keys(&mut s, "hello").preedit_text(), "hello");
        assert_eq!(s.key(&key(VK_RETURN, false)).commit.as_deref(), Some("hello"));
        assert_eq!(type_keys(&mut s, "APIfmf").preedit_text(), "API를");
    }

    #[test]
    fn escape_backspace_and_flush() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        type_keys(&mut s, "dkssud");
        let r = s.key(&key(VK_ESCAPE, false));
        assert_eq!(r.preedit_text(), "dkssud");
        assert_eq!(r.preedit[0].lang, "raw");
        assert!(s.key(&key(VK_ESCAPE, false)).preedit.is_empty());
        type_keys(&mut s, "gks");
        assert_eq!(s.key(&key(VK_BACK, false)).preedit_text(), "하");
        assert!(s.test(&key(0xBE, false)).flush_first);
        assert_eq!(s.flush().commit.as_deref(), Some("하"));
        assert!(!s.test(&key(VK_SPACE, false)).eaten);
    }

    #[test]
    fn candidate_window_and_learning() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        let first = type_keys(&mut s, "go").preedit_text();
        // Shift 탭: 후보창이 열리고 2위가 선택된다
        let r = s.shift_tap();
        let list = r.candidates.clone().expect("candidate window");
        assert!(list.items.len() >= 2);
        assert_eq!(list.selected, 1);
        assert_eq!(r.preedit_text(), list.items[1]);
        assert!(r.preedit[0].focused);
        let second = list.items[1].clone();
        assert_ne!(first, second);
        // Backspace: 후보창 닫고 자동 결과로
        assert_eq!(s.key(&key(VK_BACK, false)).preedit_text(), first);
        // 다시 열고 숫자 2로 고른 뒤 Space로 확정
        s.shift_tap();
        assert_eq!(s.key(&key(0x32, false)).preedit_text(), second);
        assert_eq!(s.key(&key(VK_SPACE, false)).commit, Some(format!("{second} ")));
        // 학습: 같은 키를 다시 치면 고른 후보가 1위
        s.reset();
        assert_eq!(type_keys(&mut s, "go").preedit_text(), second);
    }

    #[test]
    fn typing_with_window_open_commits_selection() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        type_keys(&mut s, "go");
        let sel = s.shift_tap().preedit_text();
        let r = s.key(&key(b'D' as u16, false));
        assert_eq!(r.commit.as_deref(), Some(sel.as_str()));
        assert!(["ㅇ", "d"].contains(&r.preedit_text().as_str())); // 새 조합이 시작됨
    }
}
