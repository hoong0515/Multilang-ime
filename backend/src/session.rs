//! 연결(TIP 인스턴스) 하나의 조합 상태와 키 처리.

use engine::detector::{Detector, Lang};
use engine::model::Models;
use ipc::{Key, PreeditSegment, Response};

pub struct Session<'m> {
    det: Detector<'m>,
    keys: String,       // 아직 확정하지 않은 친 키 그대로
    raw: bool,          // Esc로 "친 키 그대로" 보기 중
    prev: Option<Lang>, // 직전에 확정한 언어 (문맥)
}

fn is_letter(k: &Key) -> bool {
    (0x41..=0x5A).contains(&k.vk) && !k.ctrl && !k.alt
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;

impl<'m> Session<'m> {
    pub fn new(models: &'m Models) -> Self {
        Self { det: Detector::new(models, vec![Lang::En, Lang::Ko]), keys: String::new(), raw: false, prev: None }
    }

    fn composing(&self) -> bool {
        !self.keys.is_empty()
    }

    pub fn test(&self, k: &Key) -> Response {
        if is_letter(k) {
            return Response { eaten: true, ..Default::default() };
        }
        if !self.composing() || is_modifier(k.vk) {
            return Response::default();
        }
        match k.vk {
            VK_BACK | VK_SPACE | VK_RETURN | VK_ESCAPE => Response { eaten: true, ..Default::default() },
            _ => Response { flush_first: true, ..Default::default() },
        }
    }

    pub fn key(&mut self, k: &Key) -> Response {
        if is_letter(k) {
            let c = (k.vk as u8) as char;
            let upper = k.shift ^ k.caps;
            self.keys.push(if upper { c } else { c.to_ascii_lowercase() });
            return self.preedit_response(true);
        }
        if !self.composing() {
            return Response::default();
        }
        match k.vk {
            VK_BACK => {
                self.keys.pop();
                if self.keys.is_empty() {
                    self.raw = false;
                }
                self.preedit_response(true)
            }
            VK_SPACE => self.commit(" "),
            VK_RETURN => self.commit(""),
            VK_ESCAPE => {
                if self.raw {
                    self.clear();
                } else {
                    self.raw = true;
                }
                self.preedit_response(true)
            }
            _ => Response::default(),
        }
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
    }

    fn preedit(&self) -> (Vec<PreeditSegment>, Option<Lang>) {
        if self.keys.is_empty() {
            return (Vec::new(), None);
        }
        if self.raw {
            let seg = PreeditSegment { text: self.keys.clone(), lang: "raw".into(), ambiguous: false };
            return (vec![seg], Some(Lang::En));
        }
        let dec = self.det.decide(&self.keys, self.prev);
        let amb = self.det.params().ambiguous;
        let mut out = Vec::new();
        let mut last = None;
        for t in &dec.tokens {
            let ambiguous = t.margin() < amb;
            for s in &t.chosen.segments {
                let lang = s.lang.map(|l| l.code()).unwrap_or("other");
                out.push(PreeditSegment { text: s.text.clone(), lang: lang.into(), ambiguous });
                last = s.lang.or(last);
            }
        }
        (out, last)
    }

    fn preedit_response(&self, eaten: bool) -> Response {
        Response { eaten, preedit: self.preedit().0, ..Default::default() }
    }

    fn commit(&mut self, tail: &str) -> Response {
        let (segs, last) = self.preedit();
        let mut text: String = segs.iter().map(|s| s.text.as_str()).collect();
        text.push_str(tail);
        if !self.raw {
            self.prev = last.or(self.prev);
        }
        self.clear();
        Response { eaten: true, commit: Some(text), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn models() -> Option<Models> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/engine_model.json");
        Models::load(&p).ok()
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
        let mut s = Session::new(&m);
        assert_eq!(type_keys(&mut s, "dkssudgktpdy").preedit_text(), "안녕하세요");
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("안녕하세요 "));
        assert_eq!(type_keys(&mut s, "hello").preedit_text(), "hello");
        assert_eq!(s.key(&key(VK_RETURN, false)).commit.as_deref(), Some("hello"));
        assert_eq!(type_keys(&mut s, "APIfmf").preedit_text(), "API를");
    }

    #[test]
    fn escape_backspace_and_flush() {
        let Some(m) = models() else { return };
        let mut s = Session::new(&m);
        type_keys(&mut s, "dkssud");
        let r = s.key(&key(VK_ESCAPE, false));
        assert_eq!(r.preedit_text(), "dkssud");
        assert_eq!(r.preedit[0].lang, "raw");
        assert!(s.key(&key(VK_ESCAPE, false)).preedit.is_empty()); // 두 번째 Esc: 취소
        type_keys(&mut s, "gks");
        assert_eq!(s.key(&key(VK_BACK, false)).preedit_text(), "하");
        assert!(s.test(&key(0xBE, false)).flush_first); // 마침표: 먼저 확정
        assert_eq!(s.flush().commit.as_deref(), Some("하"));
        assert!(!s.test(&key(VK_SPACE, false)).eaten); // 조합 없으면 Space는 앱으로
    }
}
