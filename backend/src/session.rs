//! 연결(TIP 인스턴스) 하나의 조합 상태와 키 처리.
//!
//! 키 설계 (docs/v1-tsf-frontend.md):
//! - 왼쪽 Shift 탭: 후보창 열기(2위 선택) / 다음 후보
//! - 숫자 1~5: 후보창이 열려 있으면 그 후보를 고르고 창을 닫는다 (확정은 Space·Enter)
//! - Space: 확정 + 공백 (일본어로 끝나면 공백 없이), Enter: 확정
//! - Backspace: 후보창이 있으면 닫고 친 키 그대로(로마자), 없으면 직전에 친 키 하나 지우기
//! - Esc: 후보창이 열려 있으면 닫기, 아니면 친 키 그대로 보기 → 한 번 더 누르면 조합 취소
//! - 후보창이 열린 채 글자를 치면 고른 후보로 확정하고 새 조합을 시작한다
//!
//! 일본어 (M4):
//! - 일본어 구간은 로마자→가나로 판정하고, 가나가 완성되면 Mozc 변환 결과(한자)를 보여준다.
//! - Shift 탭: 일본어 토큰이면 Mozc 후보를 먼저, 그 뒤에 다른 언어 후보.
//! - 확정: 일본어 구간은 Mozc로 확정해 Mozc 학습이 일어나게 한다.
//! - 일본어 문맥의 기호: . , [ ] / ? ! → 。、「」・？！, 글자 뒤 '-' → ー (단어 안)
//! - 영어 토큰에는 전각 후보(ｇｏ)를 덧붙인다.
//!
//! 확정한 형태는 학습 기록(여러 앱이 공유, 파일에 저장)에 쌓여 다음 판정에 반영된다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use engine::detector::{Candidate, Detector, Lang, SharedHistory};
use engine::hangul::compose;
use engine::model::Models;
use engine::romaji::parse;
use ipc::{CandidateList, Key, PreeditSegment, Response};

const MAX_CANDIDATES: usize = 9;
const PICK_KEYS: u16 = 5; // 숫자 1~5
/// 낱자로 흔히 쓰는 자모 (ㅋㅋ, ㅎㅎ, ㅠㅠ 등). 이것만 남는 후보는 정상 후보로 본다.
const EXPRESSIVE_JAMO: &str = "ㅋㅎㅠㅜㄷㅇㄱㅅㅂㅈㄴㅊ";
const MOZC_RETRY: Duration = Duration::from_secs(30);
const FULLWIDTH: &str = "全角";

const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_OEM_MINUS: u16 = 0xBD;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;

// ---------------------------------------------------------------- Mozc 연결

/// Mozc 서버 상태 (백엔드 전체에서 하나). 서버 실행은 별도 스레드에서만 한다.
#[derive(Clone, Copy, PartialEq, Debug)]
enum MozcState {
    Idle,
    Launching,
    Ready,
    Failed(Instant),
}

static MOZC: std::sync::Mutex<MozcState> = std::sync::Mutex::new(MozcState::Idle);

/// 서버를 백그라운드에서 띄운다 (이미 띄우는 중이거나 준비됐으면 아무것도 안 한다).
pub fn start_mozc(server: &std::path::Path) {
    let Ok(mut st) = MOZC.lock() else { return };
    if matches!(*st, MozcState::Launching | MozcState::Ready) {
        return;
    }
    *st = MozcState::Launching;
    drop(st);
    let server = server.to_path_buf();
    std::thread::spawn(move || {
        let t = Instant::now();
        let r = mozc::Client::connect_or_launch(&server).and_then(mozc::Session::new);
        let next = match r {
            Ok(_) => {
                crate::log(&format!("mozc: ready in {} ms", t.elapsed().as_millis()));
                MozcState::Ready
            }
            Err(e) => {
                crate::log(&format!("mozc: launch failed: {e:?}"));
                MozcState::Failed(Instant::now())
            }
        };
        if let Ok(mut st) = MOZC.lock() {
            *st = next;
        }
    });
}

/// 준비됐는가. 아직이면 (필요하면 다시) 백그라운드 실행을 건다.
fn mozc_ready(server: &std::path::Path) -> bool {
    let st = MOZC.lock().map(|s| *s).unwrap_or(MozcState::Idle);
    match st {
        MozcState::Ready => true,
        MozcState::Launching => false,
        MozcState::Failed(t) if t.elapsed() < MOZC_RETRY => false,
        _ => {
            start_mozc(server);
            false
        }
    }
}

/// 연결이 끊겼다 (서버가 죽었을 수 있다): 다시 띄우게 한다.
fn mozc_lost(server: &std::path::Path) {
    if let Ok(mut st) = MOZC.lock() {
        if *st == MozcState::Ready {
            *st = MozcState::Idle;
        }
    }
    start_mozc(server);
}

const NOUN_KEYS: &str = "kore";
const NOUN_KANA: &str = "これ";

/// 변환 결과 첫 문절에서 자리표시 これ를 떼어 낸다. 첫 문절이 これ로 시작하지 않으면 None.
fn strip_noun(c: &mozc::Conversion) -> Option<mozc::Conversion> {
    let first = c.segments.first()?;
    if !first.starts_with(NOUN_KANA) || !c.readings.first()?.starts_with(NOUN_KANA) {
        return None;
    }
    let mut out = c.clone();
    let rest = first[NOUN_KANA.len()..].to_string();
    let rest_reading = c.readings[0][NOUN_KANA.len()..].to_string();
    if rest.is_empty() {
        // これ가 따로 한 문절: 그 문절과 그 후보를 버린다
        out.segments.remove(0);
        out.readings.remove(0);
        out.candidates.clear();
    } else {
        out.segments[0] = rest;
        out.readings[0] = rest_reading;
        out.candidates = c
            .candidates
            .iter()
            .filter_map(|(id, v)| v.strip_prefix(NOUN_KANA).filter(|r| !r.is_empty()).map(|r| (*id, r.to_string())))
            .collect();
    }
    Some(out)
}

/// Mozc 서버와의 연결 (없거나 실패하면 가나로 대신 보인다).
///
/// 지연: 가나를 통째로 넣는 UPDATE_COMPOSITION은 약 14 ms라, Mozc 조합을 우리 로마자 키와 맞춰 두고
/// 새로 친 키만 보낸다 (키 0.5 ms + 변환 0.5 ms + 변환 취소 0.5 ms). 앞부분이 달라지면 지우고 다시 보낸다.
pub struct MozcBridge {
    server: Option<PathBuf>,
    session: Option<mozc::Session>,
    failed_at: Option<Instant>,
    cache: HashMap<String, mozc::Conversion>,
    /// 지금 Mozc 조합에 들어 있는 로마자 키. None이면 조합 상태를 모른다 (다음에 지우고 다시 보낸다).
    typed: Option<String>,
}

/// Mozc의 조합 문자열과 우리 가나가 같은가. Mozc는 끝의 n을 전각 'ｎ'으로 두었다가 변환할 때 ん으로 바꾼다.
fn same_kana(mozc: &str, ours: &str) -> bool {
    if mozc == ours {
        return true;
    }
    match (mozc.strip_suffix(['ｎ', 'n']), ours.strip_suffix('ん')) {
        (Some(m), Some(o)) => m == o,
        _ => false,
    }
}

impl MozcBridge {
    pub fn new(server: Option<PathBuf>) -> Self {
        Self { server, session: None, failed_at: None, cache: HashMap::new(), typed: None }
    }

    fn ensure(&mut self) -> bool {
        if self.session.is_some() {
            return true;
        }
        let Some(server) = self.server.clone() else {
            if self.failed_at.is_none() {
                crate::log("mozc: no server bundled (kana only)");
                self.failed_at = Some(Instant::now());
            }
            return false;
        };
        // 키 처리 중에는 서버를 띄우지 않는다 (TIP는 200 ms만 기다린다). 준비될 때까지 가나로 보인다.
        if !mozc_ready(&server) {
            return false;
        }
        match mozc::Client::connect().and_then(mozc::Session::new) {
            Ok(s) => {
                crate::log("mozc: connected");
                self.session = Some(s);
                self.typed = None;
                true
            }
            Err(e) => {
                crate::log(&format!("mozc: connect failed: {e:?}"));
                mozc_lost(&server);
                false
            }
        }
    }

    fn drop_session(&mut self, why: &str, e: mozc::Error) {
        crate::log(&format!("mozc: {why} failed: {e:?}"));
        self.session = None;
        self.typed = None;
        if let Some(server) = &self.server {
            mozc_lost(server);
        }
    }

    /// Mozc 조합을 keys(가나 kana)로 맞춘다.
    fn sync(s: &mut mozc::Session, typed: &mut Option<String>, kana: &str, keys: &str) -> mozc::Result<()> {
        let keys = keys.to_lowercase();
        let rest = match typed.as_deref() {
            Some(t) if keys.starts_with(t) => keys[t.len()..].to_string(),
            _ => {
                s.revert()?;
                keys.clone()
            }
        };
        *typed = None;
        if !rest.is_empty() {
            let comp = s.type_keys(&rest)?;
            if !same_kana(&comp, kana) {
                // 로마자 해석이 다르면 가나를 통째로 넣는다 (느린 길)
                crate::log(&format!("mozc: slow path, mozc {comp:?} vs ours {kana:?} (keys {keys:?})"));
                s.revert()?;
                s.set_composition(kana)?;
                return Ok(()); // typed = None: 다음에 다시 맞춘다
            }
        }
        *typed = Some(keys);
        Ok(())
    }

    /// 가나의 변환 결과 (캐시). keys는 그 가나를 만든 로마자.
    pub fn conversion(&mut self, kana: &str, keys: &str) -> Option<mozc::Conversion> {
        if let Some(c) = self.cache.get(kana) {
            return Some(c.clone());
        }
        if !self.ensure() {
            return None;
        }
        let s = self.session.as_mut()?;
        let typed = &mut self.typed;
        let r = (|| -> mozc::Result<mozc::Conversion> {
            Self::sync(s, typed, kana, keys)?;
            let c = s.convert_current()?;
            s.cancel_conversion()?; // 조합 상태로 돌아가 다음 키를 이어 받는다
            Ok(c)
        })();
        match r {
            Ok(c) => {
                self.cache.insert(kana.to_string(), c.clone());
                Some(c)
            }
            Err(e) => {
                self.drop_session("convert", e);
                None
            }
        }
    }

    /// 영단어 뒤의 일본어 구간(조사로 시작)은 앞에 자리표시 명사 これ를 붙여 변환하고 떼어 낸다.
    /// (Mozc가 조사로 시작하는 문자열을 혼자 변환하면 に→二 같은 오변환이 난다: Youtube|二動画を上げた)
    pub fn conversion_ctx(&mut self, kana: &str, keys: &str, after_noun: bool) -> Option<mozc::Conversion> {
        if after_noun {
            if let Some(c) = self.conversion(&format!("{NOUN_KANA}{kana}"), &format!("{NOUN_KEYS}{keys}")) {
                if let Some(c) = strip_noun(&c) {
                    return Some(c);
                }
            }
        }
        self.conversion(kana, keys)
    }

    pub fn commit_ctx(&mut self, kana: &str, keys: &str, pick: Option<i32>, after_noun: bool) -> Option<String> {
        if after_noun {
            let t = self.commit(&format!("{NOUN_KANA}{kana}"), &format!("{NOUN_KEYS}{keys}"), pick)?;
            return t.strip_prefix(NOUN_KANA).map(str::to_string);
        }
        self.commit(kana, keys, pick)
    }

    /// Mozc로 확정한다 (학습). pick은 첫 문절 후보 id.
    pub fn commit(&mut self, kana: &str, keys: &str, pick: Option<i32>) -> Option<String> {
        if !self.ensure() {
            return None;
        }
        let s = self.session.as_mut()?;
        let typed = &mut self.typed;
        let r = (|| -> mozc::Result<String> {
            Self::sync(s, typed, kana, keys)?;
            *typed = Some(String::new()); // 확정하면 조합이 빈다
            s.convert_current()?;
            if let Some(id) = pick {
                s.select(id)?;
            }
            s.submit()
        })();
        self.cache.clear(); // 학습으로 결과가 바뀔 수 있다
        match r {
            Ok(t) if !t.is_empty() => Some(t),
            Ok(_) => None,
            Err(e) => {
                self.drop_session("commit", e);
                None
            }
        }
    }
}

// ---------------------------------------------------------------- 후보

/// 화면에 보일 후보: 판정 후보 + 구간별 표시 텍스트 (+ Mozc 후보 id).
#[derive(Clone, Debug)]
struct Choice {
    cand: Candidate,
    texts: Vec<String>,
    mozc_pick: Option<i32>,
    label: String,
}

impl Choice {
    fn text(&self) -> String {
        self.texts.concat()
    }
}

fn label(c: &Candidate) -> String {
    c.form.iter().map(|(l, _)| l.code()).collect::<Vec<_>>().join("+")
}

/// 한국어 구간에 완성되지 않은 글자(표현용 낱자모 제외)가 섞인 후보인가 (예: gㅐ, ㅗ디).
fn has_broken_hangul(c: &Candidate) -> bool {
    c.segments.iter().any(|s| {
        s.lang == Some(Lang::Ko) && compose(&s.keys).orphan_list.iter().any(|j| !EXPRESSIVE_JAMO.contains(*j))
    })
}

fn seg(text: &str, lang: &str) -> PreeditSegment {
    PreeditSegment { text: text.to_string(), lang: lang.to_string(), ambiguous: false, focused: false }
}

fn fullwidth(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_graphic() { char::from_u32(c as u32 - 0x21 + 0xFF01).unwrap_or(c) } else { c })
        .collect()
}

// ---------------------------------------------------------------- 키

fn is_letter(k: &Key) -> bool {
    (0x41..=0x5A).contains(&k.vk) && !k.ctrl && !k.alt
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

fn digit(k: &Key) -> Option<usize> {
    if !k.ctrl && !k.alt && !k.shift && (0x31..0x31 + PICK_KEYS).contains(&k.vk) {
        Some((k.vk - 0x31) as usize)
    } else {
        None
    }
}

fn is_dash(k: &Key) -> bool {
    k.vk == VK_OEM_MINUS && !k.shift && !k.ctrl && !k.alt
}

/// 미국 배열 기준 구두점 키 → 글자.
fn punct(k: &Key) -> Option<char> {
    if k.ctrl || k.alt {
        return None;
    }
    Some(match (k.vk, k.shift) {
        (0xBE, false) => '.',
        (0xBC, false) => ',',
        (0xDB, false) => '[',
        (0xDD, false) => ']',
        (0xBF, false) => '/',
        (0xBF, true) => '?',
        (0x31, true) => '!',
        _ => return None,
    })
}

/// 일본어 문맥에서 바꿔 넣는 기호.
fn ja_punct(c: char) -> Option<char> {
    Some(match c {
        '.' => '。',
        ',' => '、',
        '[' => '「',
        ']' => '」',
        '/' => '・',
        '?' => '？',
        '!' => '！',
        _ => return None,
    })
}

// ---------------------------------------------------------------- 세션

pub struct Session<'m> {
    det: Detector<'m>,
    keys: String,                       // 아직 확정하지 않은 친 키 그대로
    raw: bool,                          // 친 키 그대로 보기 중
    prev: Option<Lang>,                 // 직전에 확정한 언어 (문맥)
    run: usize,                         // 같은 언어로 이어서 확정한 단어 수 (언어 가중치)
    open: Option<(Vec<Choice>, usize)>, // 후보창: 후보 목록과 선택 위치
    pinned: Option<Choice>,             // 숫자로 고른 후보 (확정 전까지 표시)
    history: SharedHistory,
    history_path: Option<PathBuf>,
    mozc: MozcBridge,
}

impl<'m> Session<'m> {
    pub fn new(models: &'m Models, history: SharedHistory, history_path: Option<PathBuf>, mozc_server: Option<PathBuf>) -> Self {
        let mut det = Detector::new(models, vec![Lang::En, Lang::Ko, Lang::Ja]);
        det.history = Some(history.clone());
        det.dash_in_tokens = true;
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
            mozc: MozcBridge::new(mozc_server),
        }
    }

    fn composing(&self) -> bool {
        !self.keys.is_empty()
    }

    /// 지금 일본어 문맥인가: 조합 중이면 표시 중인 후보의 마지막 구간, 아니면 직전에 확정한 언어.
    fn ja_context(&self) -> bool {
        if self.composing() {
            if self.raw {
                return false;
            }
            if let Some((list, sel)) = &self.open {
                return list.get(*sel).is_some_and(|c| c.cand.end() == Lang::Ja && c.label != FULLWIDTH);
            }
            if let Some(c) = &self.pinned {
                return c.cand.end() == Lang::Ja && c.label != FULLWIDTH;
            }
            let dec = self.det.decide(&self.keys, self.prev);
            return dec.tokens.last().is_some_and(|t| t.chosen.end() == Lang::Ja);
        }
        self.prev == Some(Lang::Ja)
    }

    pub fn test(&self, k: &Key) -> Response {
        let eat = Response { eaten: true, ..Default::default() };
        if is_letter(k) {
            return eat;
        }
        if is_dash(k) && self.composing() && self.ja_context() {
            return eat; // 장음 ー
        }
        if let Some(c) = punct(k) {
            if ja_punct(c).is_some() && self.ja_context() {
                return eat;
            }
        }
        if !self.composing() || is_modifier(k.vk) {
            return Response::default();
        }
        if let (Some(d), Some((list, _))) = (digit(k), &self.open) {
            if d < list.len() {
                return eat;
            }
        }
        // 후보창이 열려 있으면 방향키로 후보를 옮긴다 (없으면 아래에서 먼저 확정하고 앱에 넘긴다)
        if self.open.is_some() && matches!(k.vk, VK_LEFT..=VK_DOWN) && !k.ctrl && !k.alt {
            return eat;
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
            let committed = if self.open.is_some() { Some(self.commit("")) } else { None };
            self.pinned = None;
            self.keys.push(ch);
            let mut r = self.respond();
            if let Some(c) = committed {
                r.commit = c.commit;
                r.commit_segments = c.commit_segments;
            }
            return r;
        }
        if is_dash(k) && self.composing() && self.ja_context() {
            self.open = None;
            self.pinned = None;
            self.keys.push('-');
            return self.respond();
        }
        if let Some(c) = punct(k) {
            if let Some(jp) = ja_punct(c) {
                if self.ja_context() {
                    let mut r = if self.composing() { self.commit("") } else { Response { eaten: true, ..Default::default() } };
                    let mut text = r.commit.take().unwrap_or_default();
                    text.push(jp);
                    r.commit_segments.push(seg(&jp.to_string(), "ja"));
                    r.commit = Some(text);
                    return r;
                }
            }
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
        if let Some((list, sel)) = &mut self.open {
            if !k.ctrl && !k.alt {
                let n = list.len().max(1);
                match k.vk {
                    VK_DOWN | VK_RIGHT => {
                        *sel = (*sel + 1) % n;
                        return self.respond();
                    }
                    VK_UP | VK_LEFT => {
                        *sel = (*sel + n - 1) % n;
                        return self.respond();
                    }
                    _ => {}
                }
            }
        }
        match k.vk {
            VK_BACK => {
                // 후보창이 있으면: 창을 닫고 친 키 그대로(로마자)로 보인다
                // 후보창이 없으면: 직전에 친 키 하나를 지운다 (조합이 없을 때는 test에서 먹지 않아 앱이 글자를 지움)
                if self.open.is_some() {
                    self.open = None;
                    self.pinned = None;
                    self.raw = true;
                } else {
                    self.pinned = None;
                    self.keys.pop();
                    if self.keys.is_empty() {
                        self.raw = false;
                    }
                }
                self.respond()
            }
            VK_SPACE => {
                // 일본어로 끝나는 조합은 공백 없이 확정한다
                let tail = if self.ja_context() { "" } else { " " };
                self.commit(tail)
            }
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

    /// 판정 후보의 구간별 표시 텍스트. 일본어는 가나가 완성되면 Mozc 변환 결과.
    fn display_texts(&mut self, c: &Candidate) -> Vec<String> {
        c.segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if s.lang == Some(Lang::Ja) {
                    let p = parse(&s.keys);
                    let after_noun = i > 0 && c.segments[i - 1].lang == Some(Lang::En);
                    if p.leftover == 0 {
                        if let Some(conv) = self.mozc.conversion_ctx(&p.kana, &s.keys, after_noun) {
                            return conv.text();
                        }
                    }
                    p.kana
                } else {
                    s.text.clone()
                }
            })
            .collect()
    }

    fn choice(&mut self, c: &Candidate) -> Choice {
        Choice { texts: self.display_texts(c), cand: c.clone(), mozc_pick: None, label: label(c) }
    }

    /// 현재 키 입력의 후보 (텍스트가 같은 후보는 하나만, 최대 9개).
    fn candidate_list(&mut self) -> Vec<Choice> {
        let dec = self.det.decide(&self.keys, self.prev);
        let Some(t) = dec.tokens.first().cloned() else { return Vec::new() };
        let mut out: Vec<Choice> = Vec::new();

        // 일본어 한 구간(또는 영단어 + 일본어)이면 Mozc 후보를 먼저 (첫 문절 후보 + 나머지 문절)
        let segs = &t.chosen.segments;
        let ja_form = match t.chosen.form.as_slice() {
            [(Lang::Ja, _)] => Some(false),
            [(Lang::En, _), (Lang::Ja, _)] => Some(true),
            _ => None,
        };
        if let Some(after_noun) = ja_form {
            let ja = &segs[segs.len() - 1];
            let p = parse(&ja.keys);
            if p.leftover == 0 {
                if let Some(conv) = self.mozc.conversion_ctx(&p.kana, &ja.keys, after_noun) {
                    let rest: String = conv.segments.iter().skip(1).cloned().collect();
                    let label = if after_noun { "en+ja" } else { "ja" };
                    for (id, v) in conv.candidates.iter().take(MAX_CANDIDATES - 2) {
                        let mut texts = if after_noun { vec![segs[0].text.clone()] } else { Vec::new() };
                        texts.push(format!("{v}{rest}"));
                        let ch = Choice { cand: t.chosen.clone(), texts, mozc_pick: Some(*id), label: label.into() };
                        if !out.iter().any(|o| o.text() == ch.text()) {
                            out.push(ch);
                        }
                    }
                }
            }
        }
        // 언어 후보 (완성되지 않은 한국어 글자가 섞인 후보는 뒤로)
        let mut lang_choices: Vec<Choice> = Vec::new();
        for c in std::iter::once(&t.chosen).chain(t.ranked.iter()) {
            if out.len() + lang_choices.len() >= MAX_CANDIDATES {
                break;
            }
            let ch = self.choice(c);
            if !out.iter().chain(lang_choices.iter()).any(|o| o.text() == ch.text()) {
                lang_choices.push(ch);
            }
        }
        let (good, broken): (Vec<Choice>, Vec<Choice>) = lang_choices.into_iter().partition(|c| !has_broken_hangul(&c.cand));
        out.extend(good);
        out.extend(broken);
        out.truncate(MAX_CANDIDATES - 1);
        // 영어 토큰에는 전각 후보 (항상 자리를 남겨 둔다)
        if let Some(en) = out.iter().find(|c| c.cand.form.len() == 1 && c.cand.start() == Lang::En).cloned() {
            let fw = fullwidth(&en.cand.segments[0].keys);
            if !out.iter().any(|o| o.text() == fw) {
                out.push(Choice { texts: vec![fw], label: FULLWIDTH.into(), ..en });
            }
        }
        out
    }

    /// 지금 표시할 후보: 후보창 선택 > 숫자로 고른 후보 > 자동 판정. (focused, ambiguous)
    fn current(&mut self) -> Option<(Choice, bool, bool)> {
        if let Some((list, sel)) = &self.open {
            return list.get(*sel).map(|c| (c.clone(), true, false));
        }
        if let Some(c) = &self.pinned {
            return Some((c.clone(), false, false));
        }
        let dec = self.det.decide(&self.keys, self.prev);
        let t = dec.tokens.first()?.clone();
        let ambiguous = t.margin() < self.det.params().ambiguous;
        Some((self.choice(&t.chosen), false, ambiguous))
    }

    fn respond(&mut self) -> Response {
        let mut r = Response { eaten: true, ..Default::default() };
        if self.keys.is_empty() {
            return r;
        }
        if self.raw {
            r.preedit = vec![PreeditSegment { text: self.keys.clone(), lang: "raw".into(), ambiguous: false, focused: false }];
            return r;
        }
        if let Some((c, focused, ambiguous)) = self.current() {
            r.preedit = c
                .cand
                .segments
                .iter()
                .zip(c.texts.iter())
                .map(|(s, t)| PreeditSegment {
                    text: t.clone(),
                    lang: s.lang.map(|l| l.code()).unwrap_or("other").into(),
                    ambiguous,
                    focused,
                })
                .collect();
        }
        if let Some((list, sel)) = &self.open {
            r.candidates = Some(CandidateList {
                items: list.iter().map(|c| c.text()).collect(),
                labels: list.iter().map(|c| c.label.clone()).collect(),
                selected: *sel,
            });
        }
        r
    }

    fn commit(&mut self, tail: &str) -> Response {
        let mut text;
        let mut segs: Vec<PreeditSegment> = Vec::new();
        if self.raw {
            text = self.keys.clone();
            segs.push(seg(&text, "raw"));
        } else if let Some((c, _, _)) = self.current() {
            // 일본어 구간은 Mozc로 확정 (학습). 전각 후보는 그대로.
            text = String::new();
            for (i, (s, shown)) in c.cand.segments.iter().zip(c.texts.iter()).enumerate() {
                if s.lang == Some(Lang::Ja) && c.label != FULLWIDTH {
                    let p = parse(&s.keys);
                    let after_noun = i > 0 && c.cand.segments[i - 1].lang == Some(Lang::En);
                    let committed =
                        if p.leftover == 0 { self.mozc.commit_ctx(&p.kana, &s.keys, c.mozc_pick, after_noun) } else { None };
                    let t = committed.as_deref().unwrap_or(shown);
                    text.push_str(t);
                    segs.push(seg(t, "ja"));
                } else {
                    text.push_str(shown);
                    let lang = if c.label == FULLWIDTH { "ja" } else { s.lang.map(|l| l.code()).unwrap_or("other") };
                    segs.push(seg(shown, lang));
                }
            }
            // 학습: 확정한 형태를 기록하고 저장한다
            if let Ok(mut h) = self.history.lock() {
                h.record(&self.keys, &c.cand.form);
                if let Some(p) = &self.history_path {
                    let _ = h.save(p);
                }
            }
            let l = if c.label == FULLWIDTH { Lang::En } else { c.cand.end() };
            self.run = if Some(l) == self.prev { self.run + 1 } else { 1 };
            self.prev = Some(l);
            self.det.set_language_run(self.run);
        } else {
            text = self.keys.clone();
            segs.push(seg(&text, "raw"));
        }
        text.push_str(tail);
        if !tail.is_empty() {
            segs.push(seg(tail, "other"));
        }
        self.clear();
        Response { eaten: true, commit: Some(text), commit_segments: segs, ..Default::default() }
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
        Session::new(m, Arc::new(Mutex::new(History::default())), None, None) // Mozc 없이 (가나 표시)
    }

    fn key(vk: u16, shift: bool) -> Key {
        Key { vk, shift, caps: false, ctrl: false, alt: false }
    }

    fn type_keys(s: &mut Session, text: &str) -> Response {
        let mut r = Response::default();
        for ch in text.chars() {
            r = match ch {
                '-' => s.key(&key(VK_OEM_MINUS, false)),
                _ => s.key(&key(ch.to_ascii_uppercase() as u16, ch.is_ascii_uppercase())),
            };
        }
        r
    }

    #[test]
    fn strip_placeholder_noun() {
        let c = mozc::Conversion {
            segments: vec!["これに".into(), "動画を".into(), "上げた".into()],
            readings: vec!["これに".into(), "どうがを".into(), "あげた".into()],
            candidates: vec![(0, "これに".into()), (1, "此れに".into()), (2, "これ荷".into())],
        };
        let s = strip_noun(&c).unwrap();
        assert_eq!(s.text(), "に動画を上げた");
        assert_eq!(s.candidates, vec![(0, "に".to_string()), (2, "荷".to_string())]);
        let c2 = mozc::Conversion { segments: vec!["此れに".into()], readings: vec!["これに".into()], candidates: vec![] };
        assert!(strip_noun(&c2).is_none());
    }

    #[test]
    fn mozc_kana_compare() {
        assert!(same_kana("こんぴゅー", "こんぴゅー"));
        assert!(same_kana("こｎ", "こん"));
        assert!(!same_kana("こｎ", "こな"));
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
    fn japanese_kana_dash_and_punctuation() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        assert_eq!(type_keys(&mut s, "watashiha").preedit_text(), "わたしは");
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("わたしは")); // 일본어는 공백 없이
        // 장음: 글자 뒤 '-'는 단어 안의 ー
        assert_eq!(type_keys(&mut s, "ko-hi-").preedit_text(), "こーひー");
        // 일본어 문맥의 마침표는 。 (조합을 확정하고 붙인다)
        assert!(s.test(&key(0xBE, false)).eaten);
        assert_eq!(s.key(&key(0xBE, false)).commit.as_deref(), Some("こーひー。"));
        // 확정 직후에도 일본어 문맥이면 、
        assert_eq!(s.key(&key(0xBC, false)).commit.as_deref(), Some("、"));
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
        assert!(s.test(&key(0xBE, false)).flush_first); // 한국어 문맥의 마침표: 먼저 확정
        assert_eq!(s.flush().commit.as_deref(), Some("하"));
        assert!(!s.test(&key(VK_SPACE, false)).eaten);
    }

    #[test]
    fn candidate_window_and_learning() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        let first = type_keys(&mut s, "go").preedit_text();
        let r = s.shift_tap();
        let list = r.candidates.clone().expect("candidate window");
        assert!(list.items.len() >= 2);
        assert_eq!(list.selected, 1);
        assert_eq!(r.preedit_text(), list.items[1]);
        assert!(r.preedit[0].focused);
        let second = list.items[1].clone();
        assert_ne!(first, second);
        // Backspace: 후보창 닫고 친 키 그대로(로마자)
        let r = s.key(&key(VK_BACK, false));
        assert_eq!(r.preedit_text(), "go");
        assert_eq!(r.preedit[0].lang, "raw");
        assert!(r.candidates.is_none());
        s.reset();
        assert_eq!(type_keys(&mut s, "go").preedit_text(), first);
        // 다시 열고 숫자 2로 고른 뒤 Space로 확정
        s.shift_tap();
        assert_eq!(s.key(&key(0x32, false)).preedit_text(), second);
        let committed = s.key(&key(VK_SPACE, false)).commit.unwrap_or_default();
        assert!(committed.starts_with(&second));
        // 학습: 같은 키를 다시 치면 고른 후보가 1위
        s.reset();
        assert_eq!(type_keys(&mut s, "go").preedit_text(), second);
    }

    #[test]
    fn candidate_order_and_fullwidth() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        type_keys(&mut s, "go");
        let list = s.shift_tap().candidates.expect("window");
        // 완성되지 않은 한국어 글자가 섞인 후보(gㅐ)는 정상 후보보다 뒤
        if let (Some(b), Some(g)) = (list.items.iter().position(|t| t.contains('ㅐ')), list.items.iter().position(|t| t == "go")) {
            assert!(b > g, "{:?}", list.items);
        }
        assert!(list.items.iter().any(|t| t == "ｇｏ"), "{:?}", list.items);
        // ㅋㅋ는 낮추지 않는다
        let mut s2 = session(&m);
        type_keys(&mut s2, "zz");
        assert_eq!(s2.shift_tap().candidates.expect("window").items[0], "ㅋㅋ");
    }

    #[test]
    fn arrow_keys_move_selection() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        type_keys(&mut s, "go");
        // 후보창이 없을 때 방향키는 먼저 확정하고 앱에 넘긴다
        assert!(s.test(&key(VK_DOWN, false)).flush_first);
        let list = s.shift_tap().candidates.expect("window");
        assert!(s.test(&key(VK_DOWN, false)).eaten);
        assert_eq!(s.key(&key(VK_DOWN, false)).candidates.unwrap().selected, 2 % list.items.len());
        assert_eq!(s.key(&key(VK_UP, false)).candidates.unwrap().selected, 1);
        assert_eq!(s.key(&key(VK_LEFT, false)).candidates.unwrap().selected, 0);
        assert_eq!(s.key(&key(VK_UP, false)).candidates.unwrap().selected, list.items.len() - 1);
    }

    #[test]
    fn typing_with_window_open_commits_selection() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        type_keys(&mut s, "go");
        let sel = s.shift_tap().preedit_text();
        let r = s.key(&key(b'D' as u16, false));
        assert_eq!(r.commit.as_deref(), Some(sel.as_str()));
        assert!(!r.preedit_text().is_empty());
    }
}
