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
//! - 일본어 문맥의 기호: . , [ ] / ? ! → 。、「」・？！, 글자 뒤 '-' → ー (단어 안. 앞 단어가 가나로 온전히 읽히면 영어로 판정돼도)
//! - 발음대로 친 장음(koohii)은 ー로 바꾼 후보(コーヒー)를 가나 후보 뒤에 더한다
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

const MAX_CANDIDATES: usize = 160;
/// 후보창 한 쪽의 후보 수 (숫자 1~9로 고른다)
const PAGE: usize = 9;
/// 언어별 후보 수 상한 (Mozc 후보, 한자)
const MAX_JA: usize = 100;
/// 발음대로 친 장음(koohii)을 ー로 바꾼 읽기의 Mozc 후보 수 (가타카나 다음에)
const MAX_LONG_JA: usize = 3;
const MAX_HANJA: usize = 40;
const PICK_KEYS: u16 = PAGE as u16; // 숫자 1~9 (지금 쪽 안에서)
/// 낱자로 흔히 쓰는 자모 (ㅋㅋ, ㅎㅎ, ㅠㅠ 등). 이것만 남는 후보는 정상 후보로 본다.
const EXPRESSIVE_JAMO: &str = "ㅋㅎㅠㅜㄷㅇㄱㅅㅂㅈㄴㅊ";
const MOZC_RETRY: Duration = Duration::from_secs(30);
const FULLWIDTH: &str = "全角";
const HANJA: &str = "漢";
const SYMBOL: &str = "記号";
/// 가나 후보 표시 (후보창 일본어 묶음 맨 앞)
const HIRAGANA: &str = "ひらがな";
const KATAKANA: &str = "カタカナ";
const HALF_KANA: &str = "半角カナ";

/// 발음대로 친 장음을 ー로 바꾼 로마자 (koohii → ko-hi-, raamen → ra-men). 같은 모음이 이어질 때만.
/// 가타카나 외래어 후보를 위한 것이라 ou·ei는 바꾸지 않는다 (とうきょう 같은 고유어가 많다).
fn long_vowel_keys(keys: &str) -> Option<String> {
    let cs: Vec<char> = keys.chars().collect();
    let mut out = String::with_capacity(keys.len());
    let mut changed = false;
    for (i, &c) in cs.iter().enumerate() {
        let v = c.to_ascii_lowercase();
        if i > 0 && matches!(v, 'a' | 'i' | 'u' | 'e' | 'o') && cs[i - 1].to_ascii_lowercase() == v {
            out.push('-');
            changed = true;
        } else {
            out.push(c);
        }
    }
    changed.then_some(out)
}

fn is_kana_label(l: &str) -> bool {
    l == HIRAGANA || l == KATAKANA || l == HALF_KANA
}

/// 히라가나 → 가타카나 (그 밖의 글자는 그대로).
fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' | '\u{309D}' | '\u{309E}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// 가타카나 → 반각 가타카나 (탁점·반탁점은 따로 붙인다).
fn to_halfwidth_katakana(s: &str) -> String {
    const BASE: &str = "ｧｱｨｲｩｳｪｴｫｵｶｶｷｷｸｸｹｹｺｺｻｻｼｼｽｽｾｾｿｿﾀﾀﾁﾁｯﾂﾂﾃﾃﾄﾄﾅﾆﾇﾈﾉﾊﾊﾊﾋﾋﾋﾌﾌﾌﾍﾍﾍﾎﾎﾎﾏﾐﾑﾒﾓｬﾔｭﾕｮﾖﾗﾘﾙﾚﾛﾜﾜｲｴｦﾝｳｶｹ";
    // ァ(30A1)..ヶ(30F6) 각 글자의 반각 + 탁점(1)/반탁점(2)
    const MARK: &str = "00000000000101010101010101010101010010101000000120120120120120000000000000000000000100";
    let base: Vec<char> = BASE.chars().collect();
    let mark: Vec<u8> = MARK.bytes().collect();
    let mut out = String::new();
    for c in s.chars() {
        let i = (c as u32).wrapping_sub(0x30A1) as usize;
        match c {
            '\u{30A1}'..='\u{30F6}' if i < base.len() => {
                out.push(base[i]);
                match mark.get(i) {
                    Some(b'1') => out.push('ﾞ'),
                    Some(b'2') => out.push('ﾟ'),
                    _ => {}
                }
            }
            'ー' => out.push('ｰ'),
            '・' => out.push('･'),
            '。' => out.push('｡'),
            '、' => out.push('､'),
            '「' => out.push('｢'),
            '」' => out.push('｣'),
            '\u{30F7}' => out.push_str("ﾜﾞ"),
            '\u{30FA}' => out.push_str("ｦﾞ"),
            _ => out.push(c),
        }
    }
    out
}

/// 사용자 사전 후보의 표시·묶음 이름
const DICT: &str = "사전";
const VK_TAB: u16 = 0x09;

const VK_BACK: u16 = 0x08;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_OEM_MINUS: u16 = 0xBD;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_PRIOR: u16 = 0x21; // Page Up
const VK_NEXT: u16 = 0x22; // Page Down

/// 선택 위치가 있는 쪽의 첫 후보 위치.
fn page_start(sel: usize) -> usize {
    sel / PAGE * PAGE
}

/// Page Down / Page Up: 다음(이전) 쪽의 첫 후보로 (끝에서는 처음으로 돈다).
fn page_move(sel: usize, n: usize, forward: bool) -> usize {
    if n == 0 {
        return 0;
    }
    let last_page = page_start(n - 1);
    let cur = page_start(sel);
    if forward {
        if cur >= last_page { 0 } else { cur + PAGE }
    } else if cur == 0 {
        last_page
    } else {
        cur - PAGE
    }
}

/// 후보 목록에서 지금 쪽만 잘라 보낸다.
fn page_list(items: Vec<String>, labels: Vec<String>, sel: usize) -> CandidateList {
    let n = items.len();
    let start = page_start(sel).min(n);
    let end = (start + PAGE).min(n);
    let pages = n.div_ceil(PAGE);
    CandidateList {
        items: items[start..end].to_vec(),
        labels: labels.get(start..end.min(labels.len())).map(|l| l.to_vec()).unwrap_or_default(),
        selected: sel - start,
        page: if pages > 1 { start / PAGE + 1 } else { 0 },
        pages: if pages > 1 { pages } else { 0 },
    }
}

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
    /// false면 확정해도 Mozc가 학습하지 않는다 (샌드박스·낮은 무결성 클라이언트).
    learn: bool,
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
        Self { server, session: None, failed_at: None, cache: HashMap::new(), typed: None, learn: true }
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
        match mozc::Client::attach(&server).and_then(mozc::Session::new) {
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

    /// Mozc 사용자 학습을 지운다 (서버가 준비돼 있을 때).
    pub fn clear_learning(&mut self) {
        self.cache.clear();
        if !self.ensure() {
            crate::log("mozc: not ready, learning not cleared");
            return;
        }
        if let Some(s) = self.session.as_mut() {
            if let Err(e) = s.clear_learning() {
                self.drop_session("clear learning", e);
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
    /// 학습하지 않는 연결이면 확정(submit) 대신 조합을 버리고 None (호출한 쪽은 보이던 글자를 쓴다).
    pub fn commit(&mut self, kana: &str, keys: &str, pick: Option<i32>) -> Option<String> {
        if !self.ensure() {
            return None;
        }
        if !self.learn {
            let r = self.session.as_mut()?.revert();
            self.typed = Some(String::new());
            if let Err(e) = r {
                self.drop_session("revert", e);
            }
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
    /// 후보창에서 Tab으로 건너뛰는 묶음 (en, ko, ja, mix, 기호 종류 등)
    group: String,
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

/// 부분 변환의 한 부분 (언어 구간, 일본어는 Mozc 문절).
#[derive(Clone, Debug)]
struct Part {
    keys: String,
    after_noun: bool,
    choices: Vec<Choice>,
    sel: usize,
}

/// 부분 변환 상태: 부분들과 지금 고치는 부분.
#[derive(Clone, Debug)]
struct Parts {
    parts: Vec<Part>,
    focus: usize,
}

/// 후보의 표시 언어 (밑줄·언어 속성용).
fn choice_lang(c: &Choice) -> &'static str {
    match c.group.as_str() {
        "ja" | "en+ja" => "ja",
        "ko" => "ko",
        "en" => "en",
        _ => match c.cand.form.last() {
            Some((l, _)) => l.code(),
            None => "other",
        },
    }
}

/// 로마자 키를 Mozc 문절 읽기에 맞춰 나눈다 (바이트 위치 범위). 맞출 수 없으면 None.
fn split_by_readings(keys: &str, readings: &[String]) -> Option<Vec<(usize, usize)>> {
    if readings.len() < 2 || !keys.is_ascii() {
        return None;
    }
    let n = keys.len();
    let mut out = Vec::new();
    let mut start = 0;
    let mut acc = String::new();
    for (k, r) in readings.iter().enumerate() {
        acc.push_str(r);
        if k == readings.len() - 1 {
            if start >= n {
                return None;
            }
            out.push((start, n));
            break;
        }
        // 이 문절까지의 읽기와 정확히 같은 가나가 되는 가장 긴 앞부분 (nn, っ 등을 다 먹도록)
        let mut best = None;
        for i in start + 1..n {
            let p = parse(&keys[..i]);
            if p.leftover == 0 && p.kana == acc {
                best = Some(i);
            }
        }
        let i = best?;
        out.push((start, i));
        start = i;
    }
    Some(out)
}

/// Tab: 다음(forward) 또는 이전 묶음의 첫 후보 위치.
fn group_jump(list: &[Choice], sel: usize, forward: bool) -> usize {
    let n = list.len();
    if n == 0 {
        return 0;
    }
    let starts: Vec<usize> = (0..n).filter(|&i| i == 0 || list[i].group != list[i - 1].group).collect();
    let cur = starts.iter().rposition(|&st| st <= sel).unwrap_or(0);
    let next = if forward { (cur + 1) % starts.len() } else { (cur + starts.len() - 1) % starts.len() };
    starts[next]
}

/// Shift 탭으로 처음 열 때의 선택: 일본어·기호 묶음이면 다음 후보, 그 밖에는 다음 언어 묶음의 첫 후보.
fn initial_selection(list: &[Choice]) -> usize {
    // 사용자 사전에 등록한 변환이 있으면 그것부터
    if list.first().is_some_and(|c| c.group == DICT) {
        return 0;
    }
    if list.len() < 2 {
        return 0;
    }
    if list[0].group == "ja" || list[0].group == "en+ja" || list[0].cand.form.is_empty() {
        return 1;
    }
    match group_jump(list, 0, true) {
        0 => 1,
        i => i,
    }
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

fn is_fullwidth_space(k: &Key) -> bool {
    k.vk == VK_SPACE && k.shift && !k.ctrl && !k.alt
}

fn is_dash(k: &Key) -> bool {
    k.vk == VK_OEM_MINUS && !k.shift && !k.ctrl && !k.alt
}

use ipc::symbol_char;

/// 기호 조합의 후보: (텍스트, 라벨). 첫 번째가 기본 표시.
fn symbol_variants(sym: &str, ja: bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |t: String, l: &str| {
        if !t.is_empty() && !out.iter().any(|(x, _)| *x == t) {
            out.push((t, l.to_string()));
        }
    };
    let ja_all: Option<String> = sym.chars().map(ja_punct).collect();
    if ja {
        if let Some(j) = &ja_all {
            push(j.clone(), "ja");
        }
    }
    push(sym.to_string(), "en");
    if let Some(j) = &ja_all {
        push(j.clone(), "ja");
    }
    push(fullwidth(sym), FULLWIDTH);
    let special: &[&str] = match sym {
        "." => &["…", "・", "·"],
        "," => &["、", "，"],
        "!" => &["‼", "¡"],
        "?" => &["⁇", "⁉", "¿"],
        "-" => &["ー", "—", "–", "―", "‐"],
        "~" => &["〜", "～", "≈"],
        "/" => &["・", "÷", "／"],
        "<" => &["〈", "《", "≪", "←", "≤"],
        ">" => &["〉", "》", "≫", "→", "≥"],
        "[" => &["「", "『", "【", "〔"],
        "]" => &["」", "』", "】", "〕"],
        "{" => &["〈"],
        "}" => &["〉"],
        "(" => &["「"],
        ")" => &["」"],
        "*" => &["※", "×", "★", "☆"],
        "\"" => &["“", "”", "「", "」"],
        "'" => &["‘", "’", "『", "』"],
        "^" => &["↑"],
        "+" => &["±"],
        "=" => &["≠", "≒", "≈"],
        "%" => &["‰"],
        "#" => &["♯"],
        "$" => &["￦", "￥", "€", "£"],
        "\\" => &["￦", "￥"],
        "|" => &["│"],
        ":" => &["∵", "∴"],
        "->" => &["→", "⇒"],
        "<-" => &["←", "⇐"],
        "=>" => &["⇒"],
        "<=" => &["≤", "⇐"],
        ">=" => &["≥"],
        "<>" => &["↔", "⇔"],
        "..." => &["…"],
        "!?" => &["⁉"],
        "!!" => &["‼"],
        "--" => &["—", "―"],
        _ => &[],
    };
    for x in special {
        push(x.to_string(), SYMBOL);
    }
    // 한 자리 숫자: 원 숫자, 한자 숫자
    if let [c] = sym.chars().collect::<Vec<_>>()[..] {
        if let Some(d) = c.to_digit(10) {
            push(["⓪", "①", "②", "③", "④", "⑤", "⑥", "⑦", "⑧", "⑨"][d as usize].to_string(), SYMBOL);
            push(["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"][d as usize].to_string(), "ja");
        }
    }
    out
}

fn symbol_choice(sym: &str, text: String, label: String) -> Choice {
    let cand = Candidate {
        form: Vec::new(),
        segments: vec![engine::detector::Segment { lang: None, keys: sym.to_string(), text: text.clone() }],
        score: 0.0,
    };
    Choice { cand, texts: vec![text], mozc_pick: None, group: label.clone(), label }
}

/// 사용자 사전 후보. 언어(문맥용)는 변환 글자로 정한다. 구간 언어는 비워 두어 Mozc 확정·학습을 거치지 않는다.
fn dict_choice(keys: &str, text: String) -> Choice {
    let lang = if text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}')) {
        Lang::Ja
    } else if text.chars().any(|c| matches!(c, '\u{AC00}'..='\u{D7A3}' | '\u{3131}'..='\u{318E}')) {
        Lang::Ko
    } else {
        Lang::En
    };
    let cand = Candidate {
        form: vec![(lang, keys.chars().count())],
        segments: vec![engine::detector::Segment { lang: None, keys: keys.to_string(), text: text.clone() }],
        score: 0.0,
    };
    Choice { cand, texts: vec![text], mozc_pick: None, label: DICT.into(), group: DICT.into() }
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
    sym: String,                        // 조합 중인 기호·숫자 (keys와 동시에 쓰지 않는다)
    parts: Option<Parts>,               // 부분 변환 중 (긴 입력을 나눠 부분마다 후보를 고른다)
    /// 이어서 변환(continue) 중 Space를 친 위치 (keys 안의 글자 위치)
    ja_breaks: Vec<usize>,
    /// 시험용: 설정 파일 대신 쓸 일본어 Space 동작
    ja_space_override: Option<crate::settings::JaSpace>,
    raw: bool,                          // 친 키 그대로 보기 중
    prev: Option<Lang>,                 // 직전에 확정한 언어 (문맥)
    run: usize,                         // 같은 언어로 이어서 확정한 단어 수 (언어 가중치)
    open: Option<(Vec<Choice>, usize)>, // 후보창: 후보 목록과 선택 위치
    pinned: Option<Choice>,             // 숫자로 고른 후보 (확정 전까지 표시)
    history: SharedHistory,
    history_path: Option<PathBuf>,
    mozc: MozcBridge,
    /// false면 확정을 학습하지 않는다 (판정 기록·한자·Mozc). 샌드박스(AppContainer)·낮은 무결성 연결.
    learn: bool,
    /// 이 연결의 프로그램 실행 파일 이름 (Hello)
    pub app: String,
}

impl<'m> Session<'m> {
    pub fn new(models: &'m Models, history: SharedHistory, history_path: Option<PathBuf>, mozc_server: Option<PathBuf>) -> Self {
        let mut det = Detector::new(models, vec![Lang::En, Lang::Ko, Lang::Ja]);
        det.history = Some(history.clone());
        det.dash_in_tokens = true;
        Self {
            det,
            keys: String::new(),
            sym: String::new(),
            parts: None,
            ja_space_override: None,
            ja_breaks: Vec::new(),
            raw: false,
            prev: None,
            run: 0,
            open: None,
            pinned: None,
            history,
            history_path,
            mozc: MozcBridge::new(mozc_server),
            learn: true,
            app: String::new(),
        }
    }

    /// 이 연결의 확정을 학습할지 (샌드박스 앱이 가짜 입력으로 공용 학습을 바꾸지 못하게, 보안 검토 2026-11-24 R2).
    pub fn set_learning(&mut self, on: bool) {
        self.learn = on;
        self.mozc.learn = on;
    }

    /// 설정을 반영한다 (요청마다): 사용할 언어. 조합 중에는 바꾸지 않는다.
    pub fn apply_settings(&mut self, s: &crate::settings::Settings) {
        if self.composing() {
            return;
        }
        let mut langs = vec![Lang::En];
        if s.languages.ko {
            langs.push(Lang::Ko);
        }
        if s.languages.ja {
            langs.push(Lang::Ja);
        }
        // 언어 고정: 한국어·일본어는 그 언어로만 판정 (English 고정은 키를 받지 않는다: main)
        match crate::lock::get() {
            Some(l @ (Lang::Ko | Lang::Ja)) => langs = vec![l],
            _ => {}
        }
        if self.det.langs != langs {
            crate::log(&format!("languages: {langs:?}"));
            self.det.langs = langs;
            if self.prev.is_some_and(|l| !self.det.langs.contains(&l)) {
                self.prev = None;
                self.run = 0;
            }
        }
    }

    /// 조합 중인가 (외부에서 English 고정 판단용).
    pub fn is_composing(&self) -> bool {
        self.composing()
    }

    /// 한/영 키: 지금 조합을 확정하고 언어 고정을 다음 상태로 바꾼다.
    pub fn cycle_lock(&mut self, s: &crate::settings::Settings) -> Response {
        let mut r = self.flush();
        let quick = crate::lock::note_press();
        let next = crate::lock::press(crate::lock::get(), quick, s.languages.ko, s.languages.ja);
        crate::lock::set(next);
        self.apply_settings(s);
        r.eaten = true;
        r
    }

    /// 입력 표시기: 고정이면 그 언어, 아니면 조합 중인 마지막 구간의 언어, 없으면 직전에 확정한 언어.
    pub fn indicator(&self, resp: &Response) -> ipc::Indicator {
        if let Some(l) = crate::lock::get() {
            return ipc::Indicator { lang: l.code().into(), locked: true };
        }
        let composing = resp.preedit.iter().rev().map(|p| p.lang.as_str()).find(|l| matches!(*l, "ko" | "en" | "ja"));
        let lang = composing.map(str::to_string).or_else(|| self.prev.map(|l| l.code().to_string())).unwrap_or_else(|| "ko".into());
        ipc::Indicator { lang, locked: false }
    }

    /// 이 프로그램에서 IME를 끈 상태인가 (조합 중이면 끝까지 받는다).
    pub fn app_off(&self, s: &crate::settings::Settings) -> bool {
        !self.composing() && s.app_disabled(&self.app)
    }

    /// 학습 기록을 모두 지운다: 판정 기록(history.json), 한자 선택, Mozc 사용자 학습.
    pub fn clear_learning(&mut self) -> Response {
        self.clear();
        if let Ok(mut h) = self.history.lock() {
            *h = engine::detector::History::default();
            if let Some(p) = &self.history_path {
                let _ = h.save(p);
            }
        }
        crate::hanja::clear_learning();
        self.mozc.clear_learning();
        crate::log("learning cleared");
        Response::default()
    }

    fn composing(&self) -> bool {
        !self.keys.is_empty() || !self.sym.is_empty()
    }

    /// 이어서 변환 중 마지막 Space 뒤의 키가 (2키 이상) 일본어가 아닌 것으로 판정되면,
    /// 그 앞(일본어)을 확정하고 뒤만 조합으로 남긴다. 확정 응답을 돌려준다.
    fn split_off_non_ja(&mut self) -> Option<Response> {
        let &b = self.ja_breaks.last()?;
        let suffix = self.keys.get(b..)?.to_string();
        // 로마자로 가나가 되지 않는 글자가 2개 이상일 때만 (일본어 단어의 앞부분은 대개 가나로 풀린다: ga, gak)
        if suffix.chars().count() < 2 || parse(&suffix).leftover < 2 {
            return None;
        }
        let dec = self.det.decide(&suffix, Some(Lang::Ja));
        if dec.tokens.first().is_none_or(|t| t.chosen.start() == Lang::Ja) {
            return None;
        }
        self.keys.truncate(b);
        let c = self.commit("");
        self.keys = suffix;
        Some(c)
    }

    fn ja_space(&self) -> crate::settings::JaSpace {
        self.ja_space_override.unwrap_or_else(|| crate::settings::current().ja_space)
    }


    /// 지금 일본어 문맥인가: 조합 중이면 표시 중인 후보의 마지막 구간, 아니면 직전에 확정한 언어.
    /// '-'를 장음(ー)으로 조합에 넣을까: 일본어 문맥이거나, 일본어를 쓰는 중이고 지금 단어가 가나로 온전히 읽힐 때.
    /// 앞 단어가 영어로 판정돼도(예: me → me) 장음을 칠 수 있게 한다 (me-ru → めーる). 판정은 '-'까지 넣은 키로 다시 한다.
    fn dash_as_long_vowel(&self) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        if self.ja_context() {
            return true;
        }
        if self.raw || !self.sym.is_empty() || self.parts.is_some() || !self.det.langs.contains(&Lang::Ja) {
            return false;
        }
        let word = self.keys.rsplit(|c: char| !c.is_ascii_alphabetic() && c != '-').next().unwrap_or("");
        word.ends_with(|c: char| c.is_ascii_alphabetic()) && parse(word).leftover == 0
    }

    fn ja_context(&self) -> bool {
        if let Some(ps) = &self.parts {
            return ps.parts.last().is_some_and(|p| choice_lang(&p.choices[p.sel]) == "ja");
        }
        if !self.sym.is_empty() {
            let c = self.open.as_ref().and_then(|(l, i)| l.get(*i)).or(self.pinned.as_ref());
            // 일본어 기호(。、？ 등)·전각을 골랐거나, 직전 단어가 일본어면 일본어 문맥
            return c.is_some_and(|c| c.label == "ja" || c.label == FULLWIDTH) || self.prev == Some(Lang::Ja);
        }
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
        if is_dash(k) && self.dash_as_long_vowel() {
            return eat; // 장음 ー
        }
        if let (Some(d), Some((list, sel))) = (digit(k), &self.open) {
            if page_start(*sel) + d < list.len() {
                return eat; // 후보 고르기
            }
        }
        if let (Some(d), Some(ps)) = (digit(k), &self.parts) {
            let p = &ps.parts[ps.focus];
            if page_start(p.sel) + d < p.choices.len() {
                return eat;
            }
        }
        if symbol_char(k).is_some() {
            return eat; // 기호·숫자도 조합에 넣는다 (Shift 탭으로 변형)
        }
        if is_fullwidth_space(k) {
            return eat; // Shift+Space: 전각 공백
        }
        if !self.composing() || is_modifier(k.vk) {
            return Response::default();
        }
        // 후보창이 열려 있으면 방향키·Tab으로 후보를 옮긴다 (없으면 아래에서 먼저 확정하고 앱에 넘긴다)
        if (self.open.is_some() || self.parts.is_some()) && (matches!(k.vk, VK_PRIOR..=VK_DOWN) || k.vk == VK_TAB) && !k.ctrl && !k.alt {
            return eat;
        }
        match k.vk {
            VK_BACK | VK_SPACE | VK_RETURN | VK_ESCAPE => eat,
            _ => Response { flush_first: true, ..Default::default() },
        }
    }

    pub fn key(&mut self, k: &Key) -> Response {
        if let Some(ps) = &mut self.parts {
            if !k.ctrl && !k.alt {
                let last = ps.parts.len() - 1;
                let part = &mut ps.parts[ps.focus];
                let n = part.choices.len().max(1);
                match k.vk {
                    VK_LEFT => {
                        ps.focus = ps.focus.saturating_sub(1);
                        return self.respond();
                    }
                    VK_RIGHT => {
                        ps.focus = (ps.focus + 1).min(last);
                        return self.respond();
                    }
                    VK_DOWN => {
                        part.sel = (part.sel + 1) % n;
                        return self.respond();
                    }
                    VK_UP => {
                        part.sel = (part.sel + n - 1) % n;
                        return self.respond();
                    }
                    VK_TAB => {
                        part.sel = group_jump(&part.choices, part.sel, !k.shift);
                        return self.respond();
                    }
                    VK_NEXT | VK_PRIOR => {
                        part.sel = page_move(part.sel, part.choices.len(), k.vk == VK_NEXT);
                        return self.respond();
                    }
                    VK_BACK => {
                        // 부분 변환을 닫고 친 키 그대로
                        self.parts = None;
                        self.raw = true;
                        return self.respond();
                    }
                    VK_ESCAPE => {
                        self.parts = None;
                        return self.respond();
                    }
                    _ => {
                        if let Some(d) = digit(k) {
                            let i = page_start(part.sel) + d;
                            if i < part.choices.len() {
                                part.sel = i;
                                return self.respond();
                            }
                        }
                    }
                }
            }
        }
        if is_letter(k) {
            let c = (k.vk as u8) as char;
            let ch = if k.shift ^ k.caps { c } else { c.to_ascii_lowercase() };
            // 후보창이 열린 채 글자를 치면 고른 후보로 확정하고 새 조합을 시작한다
            let committed = if self.open.is_some() || self.parts.is_some() || !self.sym.is_empty() { Some(self.commit("")) } else { None };
            self.pinned = None;
            self.keys.push(ch);
            // 이어서 변환 중: Space 뒤 새 단어가 일본어가 아니면 앞의 일본어를 먼저 확정하고 새 단어만 남긴다
            let committed = committed.or_else(|| self.split_off_non_ja());
            let mut r = self.respond();
            if let Some(c) = committed {
                r.commit = c.commit;
                r.commit_segments = c.commit_segments;
            }
            return r;
        }
        if is_dash(k) && self.dash_as_long_vowel() {
            self.open = None;
            self.pinned = None;
            self.keys.push('-');
            return self.respond();
        }
        if let Some(d) = digit(k) {
            if let Some((list, sel)) = &self.open {
                if let Some(c) = list.get(page_start(*sel) + d) {
                    self.pinned = Some(c.clone());
                    self.open = None;
                    return self.respond();
                }
            }
        }
        if is_fullwidth_space(k) {
            // Shift+Space: 조합이 있으면 확정하고 전각 공백(U+3000)을 붙인다
            let mut r = if self.composing() { self.commit("") } else { Response { eaten: true, ..Default::default() } };
            let mut text = r.commit.take().unwrap_or_default();
            text.push('\u{3000}');
            r.commit = Some(text);
            r.commit_segments.push(seg("\u{3000}", "ja"));
            return r;
        }
        if let Some(ch) = symbol_char(k) {
            // 글자 조합, 또는 후보를 고른 기호 조합은 먼저 확정하고 기호 조합을 시작/이어 간다
            let committed = if !self.keys.is_empty() || self.open.is_some() || self.pinned.is_some() || self.parts.is_some() {
                Some(self.commit(""))
            } else {
                None
            };
            self.sym.push(ch);
            let mut r = self.respond();
            if let Some(c) = committed {
                r.commit = c.commit;
                r.commit_segments = c.commit_segments;
            }
            return r;
        }
        if !self.composing() {
            return Response::default();
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
                    VK_NEXT | VK_PRIOR => {
                        *sel = page_move(*sel, list.len(), k.vk == VK_NEXT);
                        return self.respond();
                    }
                    VK_TAB => {
                        // 다음(Shift+Tab: 이전) 언어 묶음의 첫 후보로
                        *sel = group_jump(list, *sel, !k.shift);
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
                } else if !self.sym.is_empty() {
                    self.pinned = None;
                    self.sym.pop();
                } else {
                    self.pinned = None;
                    self.keys.pop();
                    while self.ja_breaks.last().is_some_and(|&b| b >= self.keys.len()) {
                        self.ja_breaks.pop();
                    }
                    if self.keys.is_empty() {
                        self.raw = false;
                    }
                }
                self.respond()
            }
            VK_SPACE => {
                if self.ja_context() {
                    // 이어서 변환 설정: 글자 조합 중이면 확정하지 않고 이어 간다 (문장을 한 번에 변환)
                    let plain = self.open.is_none() && self.parts.is_none() && self.pinned.is_none() && self.sym.is_empty();
                    if plain && !self.keys.is_empty() && self.ja_space() == crate::settings::JaSpace::Continue {
                        if self.ja_breaks.last() != Some(&self.keys.len()) {
                            self.ja_breaks.push(self.keys.len());
                        }
                        return self.respond();
                    }
                    // 기본: 확정 + 전각 공백
                    return self.commit("\u{3000}");
                }
                self.commit(" ")
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
        // 부분 변환 중: 지금 부분의 다음 후보
        if let Some(ps) = &mut self.parts {
            let part = &mut ps.parts[ps.focus];
            part.sel = (part.sel + 1) % part.choices.len().max(1);
            return self.respond();
        }
        // 여러 부분으로 나뉘는 입력이면 부분 변환을 시작한다
        if self.open.is_none() && self.sym.is_empty() {
            self.raw = false;
            if let Some(ps) = self.build_parts() {
                self.pinned = None;
                self.parts = Some(ps);
                return self.respond();
            }
        }
        match &mut self.open {
            Some((list, sel)) => *sel = (*sel + 1) % list.len().max(1),
            None => {
                let list = self.candidate_list();
                if list.is_empty() {
                    return self.respond();
                }
                let mut sel = initial_selection(&list);
                if list[0].group == "ja" {
                    // 일본어: 맨 앞(가나)부터, 지금 보이는 것과 같으면 다음
                    let shown = self.current().map(|x| x.0.text()).unwrap_or_default();
                    sel = list.iter().position(|c| c.text() != shown).unwrap_or(0);
                }
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
        self.ja_breaks.clear();
        self.sym.clear();
        self.parts = None;
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
        let group = if c.form.len() == 1 { c.start().code().to_string() } else { "mix".to_string() };
        Choice { texts: self.display_texts(c), cand: c.clone(), mozc_pick: None, label: label(c), group }
    }

    /// 현재 입력의 후보. 문맥과 상관없이 언어마다 묶음을 만든다 (1위 언어 묶음 먼저):
    /// 영어 = 그대로 + 전각, 한국어 = 한글 + 한자, 일본어 = Mozc 후보. 그 뒤 섞인 형태, 깨진 한글.
    fn candidate_list(&mut self) -> Vec<Choice> {
        if !self.sym.is_empty() {
            let sym = self.sym.clone();
            return symbol_variants(&sym, self.prev == Some(Lang::Ja))
                .into_iter()
                .map(|(t, l)| symbol_choice(&sym, t, l))
                .collect();
        }
        let keys = self.keys.clone();
        self.candidate_list_for(&keys, false)
    }

    /// 키 문자열 하나의 후보 (언어 묶음). after_noun_ja: 영단어 뒤 일본어 구간 (Mozc에 これ를 붙여 변환).
    fn candidate_list_for(&mut self, keys: &str, after_noun_ja: bool) -> Vec<Choice> {
        let dec = self.det.decide(keys, self.prev);
        let Some(t) = dec.tokens.first().cloned() else { return Vec::new() };
        let mut out: Vec<Choice> = Vec::new();
        fn add(out: &mut Vec<Choice>, ch: Choice) {
            if out.len() < MAX_CANDIDATES && !out.iter().any(|o| o.text() == ch.text()) {
                out.push(ch);
            }
        }

        // 영단어 + 일본어로 판정됐으면 그 Mozc 후보 묶음을 먼저
        // 사용자 사전 (키 전체가 등록된 것)
        for text in crate::userdict::lookup(keys) {
            add(&mut out, dict_choice(keys, text));
        }
        let segs = t.chosen.segments.clone();
        if let [(Lang::En, _), (Lang::Ja, _)] = t.chosen.form.as_slice() {
            let p = parse(&segs[1].keys);
            if p.leftover == 0 {
                if let Some(conv) = self.mozc.conversion_ctx(&p.kana, &segs[1].keys, true) {
                    let rest: String = conv.segments.iter().skip(1).cloned().collect();
                    for (id, v) in conv.candidates.iter().take(MAX_JA) {
                        let texts = vec![segs[0].text.clone(), format!("{v}{rest}")];
                        let ch = Choice { cand: t.chosen.clone(), texts, mozc_pick: Some(*id), label: "en+ja".into(), group: "en+ja".into() };
                        add(&mut out, ch);
                    }
                }
            }
        }
        // 한 언어 후보: 순위대로, 언어마다 한 번
        let all: Vec<Candidate> = std::iter::once(t.chosen.clone()).chain(t.ranked.iter().cloned()).collect();
        let mut seen_langs: Vec<Lang> = Vec::new();
        let mut broken: Vec<Choice> = Vec::new();
        for c in all.iter().filter(|c| c.form.len() == 1) {
            let lang = c.start();
            if seen_langs.contains(&lang) {
                continue;
            }
            seen_langs.push(lang);
            match lang {
                Lang::Ja => {
                    let keys = c.segments[0].keys.clone();
                    let p = parse(&keys);
                    let mut any = false;
                    if p.leftover == 0 {
                        let conv = self.mozc.conversion_ctx(&p.kana, &keys, after_noun_ja);
                        // 가나 후보를 일본어 묶음 맨 앞에: 히라가나, 가타카나, 반각 가타카나 (Mozc가 아직 없어도).
                        // 한 문절이고 Mozc 후보에 같은 글자가 있으면 그 id로 확정(학습), 없으면 Mozc를 거치지 않는다.
                        let kata = to_katakana(&p.kana);
                        for (text, label) in [(p.kana.clone(), HIRAGANA), (kata.clone(), KATAKANA), (to_halfwidth_katakana(&kata), HALF_KANA)] {
                            let pick = conv
                                .as_ref()
                                .filter(|cv| cv.segments.len() == 1)
                                .and_then(|cv| cv.candidates.iter().find(|(_, v)| *v == text).map(|(id, _)| *id));
                            let ch = Choice { cand: c.clone(), texts: vec![text], mozc_pick: pick, label: label.into(), group: "ja".into() };
                            add(&mut out, ch);
                            any = true;
                        }
                        // 발음대로 친 장음 (koohii): ー로 바꾼 가타카나와 그 읽기의 Mozc 후보 몇 개 (コーヒー, 珈琲)
                        if let Some(lk) = long_vowel_keys(&keys) {
                            let lp = parse(&lk);
                            if lp.leftover == 0 {
                                let mut lc = c.clone();
                                lc.segments[0].keys = lk.clone();
                                lc.segments[0].text = lp.kana.clone();
                                let lconv = self.mozc.conversion_ctx(&lp.kana, &lk, after_noun_ja);
                                let kata = to_katakana(&lp.kana);
                                let pick = lconv
                                    .as_ref()
                                    .filter(|cv| cv.segments.len() == 1)
                                    .and_then(|cv| cv.candidates.iter().find(|(_, v)| *v == kata).map(|(id, _)| *id));
                                add(&mut out, Choice { cand: lc.clone(), texts: vec![kata], mozc_pick: pick, label: KATAKANA.into(), group: "ja".into() });
                                if let Some(lconv) = lconv {
                                    let rest: String = lconv.segments.iter().skip(1).cloned().collect();
                                    for (id, v) in lconv.candidates.iter().take(MAX_LONG_JA) {
                                        let ch = Choice { cand: lc.clone(), texts: vec![format!("{v}{rest}")], mozc_pick: Some(*id), label: "ja".into(), group: "ja".into() };
                                        add(&mut out, ch);
                                    }
                                }
                            }
                        }
                        if let Some(conv) = conv {
                            let rest: String = conv.segments.iter().skip(1).cloned().collect();
                            for (id, v) in conv.candidates.iter().take(MAX_JA) {
                                any = true;
                                let ch = Choice { cand: c.clone(), texts: vec![format!("{v}{rest}")], mozc_pick: Some(*id), label: "ja".into(), group: "ja".into() };
                                add(&mut out, ch);
                            }
                        }
                    }
                    if !any {
                        let ch = self.choice(c);
                        add(&mut out, ch);
                    }
                }
                Lang::Ko => {
                    let ch = self.choice(c);
                    if has_broken_hangul(c) {
                        broken.push(ch);
                        continue;
                    }
                    add(&mut out, ch);
                    for h in crate::hanja::candidates(&c.segments[0].text, MAX_HANJA) {
                        let ch = Choice { cand: c.clone(), texts: vec![h], mozc_pick: None, label: HANJA.into(), group: "ko".into() };
                        add(&mut out, ch);
                    }
                }
                Lang::En => {
                    let ch = self.choice(c);
                    let fw = fullwidth(&c.segments[0].keys);
                    add(&mut out, ch.clone());
                    add(&mut out, Choice { texts: vec![fw], label: FULLWIDTH.into(), ..ch });
                }
            }
        }
        // 여러 언어가 섞인 후보 (깨진 한글이 섞인 것은 맨 뒤)
        let mut mixed = 0;
        for c in all.iter().filter(|c| c.form.len() > 1) {
            if mixed >= 3 {
                break;
            }
            let ch = self.choice(c);
            if has_broken_hangul(c) {
                broken.push(ch);
            } else {
                mixed += 1;
                add(&mut out, ch);
            }
        }
        for ch in broken.into_iter().take(2) {
            add(&mut out, Choice { group: "mix".into(), ..ch });
        }
        out
    }

    /// 지금 표시 중인 결과를 부분으로 나눈다: 언어 구간마다, 일본어는 Mozc 문절마다. 두 부분 이상일 때만.
    fn build_parts(&mut self) -> Option<Parts> {
        let (c, _, _) = self.current()?;
        if c.cand.form.is_empty() {
            return None;
        }
        // (키, 지금 보이는 텍스트, 영단어 뒤 일본어인가)
        let mut specs: Vec<(String, String, bool)> = Vec::new();
        for (i, (s, shown)) in c.cand.segments.iter().zip(c.texts.iter()).enumerate() {
            let after = i > 0 && c.cand.segments[i - 1].lang == Some(Lang::En) && s.lang == Some(Lang::Ja);
            if s.lang == Some(Lang::Ja) {
                let p = parse(&s.keys);
                if p.leftover == 0 {
                    if let Some(conv) = self.mozc.conversion_ctx(&p.kana, &s.keys, after) {
                        if let Some(ranges) = split_by_readings(&s.keys, &conv.readings) {
                            for (k, (a, b)) in ranges.iter().enumerate() {
                                specs.push((s.keys[*a..*b].to_string(), conv.segments[k].clone(), after && k == 0));
                            }
                            continue;
                        }
                    }
                }
            }
            specs.push((s.keys.clone(), shown.clone(), after));
        }
        if specs.len() < 2 {
            return None;
        }
        let mut parts = Vec::new();
        for (keys, default, after) in specs {
            let mut choices = self.candidate_list_for(&keys, after);
            // 지금 보이던 결과를 첫 후보로
            if let Some(i) = choices.iter().position(|c| c.text() == default) {
                let ch = choices.remove(i);
                choices.insert(0, ch);
            } else if let Some(first) = choices.first().cloned() {
                choices.insert(0, Choice { texts: vec![default], mozc_pick: None, ..first });
            } else {
                return None;
            }
            parts.push(Part { keys, after_noun: after, choices, sel: 0 });
        }
        Some(Parts { parts, focus: 0 })
    }

    /// 지금 표시할 후보: 후보창 선택 > 숫자로 고른 후보 > 자동 판정. (focused, ambiguous)
    fn current(&mut self) -> Option<(Choice, bool, bool)> {
        if let Some((list, sel)) = &self.open {
            return list.get(*sel).map(|c| (c.clone(), true, false));
        }
        if let Some(c) = &self.pinned {
            return Some((c.clone(), false, false));
        }
        if !self.sym.is_empty() {
            let sym = self.sym.clone();
            let (t, l) = symbol_variants(&sym, self.prev == Some(Lang::Ja)).into_iter().next()?;
            return Some((symbol_choice(&sym, t, l), false, false));
        }
        let dec = self.det.decide(&self.keys, self.prev);
        let t = dec.tokens.first()?.clone();
        let ambiguous = t.margin() < self.det.params().ambiguous;
        Some((self.choice(&t.chosen), false, ambiguous))
    }

    fn respond(&mut self) -> Response {
        let mut r = Response { eaten: true, ..Default::default() };
        if !self.composing() {
            return r;
        }
        if self.raw {
            r.preedit = vec![PreeditSegment { text: self.keys.clone(), lang: "raw".into(), ambiguous: false, focused: false }];
            return r;
        }
        if let Some(ps) = &self.parts {
            r.preedit = ps
                .parts
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let c = &p.choices[p.sel];
                    PreeditSegment { text: c.text(), lang: choice_lang(c).into(), ambiguous: false, focused: i == ps.focus }
                })
                .collect();
            let p = &ps.parts[ps.focus];
            r.candidates = Some(page_list(
                p.choices.iter().map(|c| c.text()).collect(),
                p.choices.iter().map(|c| c.label.clone()).collect(),
                p.sel,
            ));
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
            r.candidates = Some(page_list(
                list.iter().map(|c| c.text()).collect(),
                list.iter().map(|c| c.label.clone()).collect(),
                *sel,
            ));
        }
        r
    }

    fn commit(&mut self, tail: &str) -> Response {
        let mut text;
        let mut segs: Vec<PreeditSegment> = Vec::new();
        if let Some(ps) = self.parts.take() {
            // 부분 변환: 부분마다 고른 후보를 이어 붙이고, 일본어 부분은 Mozc로 학습시킨다
            text = String::new();
            let mut last: Option<Lang> = None;
            for p in &ps.parts {
                let ch = &p.choices[p.sel];
                let t = ch.text();
                if ch.group == "ja" && ch.cand.form.len() == 1 && ch.cand.start() == Lang::Ja && !(is_kana_label(&ch.label) && ch.mozc_pick.is_none()) {
                    let kp = parse(&p.keys);
                    if kp.leftover == 0 {
                        let _ = self.mozc.commit_ctx(&kp.kana, &p.keys, ch.mozc_pick, p.after_noun);
                    }
                }
                if ch.label == HANJA && self.learn {
                    crate::hanja::learn(&ch.cand.segments[0].text, &t);
                }
                segs.push(seg(&t, choice_lang(ch)));
                text.push_str(&t);
                if let Some((l, _)) = ch.cand.form.last() {
                    last = Some(*l);
                }
            }
            if let Some(l) = last {
                self.run = if Some(l) == self.prev { self.run + 1 } else { 1 };
                self.prev = Some(l);
                self.det.set_language_run(self.run);
            }
        } else if !self.sym.is_empty() {
            // 기호 조합: 언어 문맥·학습은 바꾸지 않는다
            let c = self.current().map(|x| x.0);
            text = c.as_ref().map(|c| c.text()).unwrap_or_else(|| self.sym.clone());
            let lang = match c.as_ref().map(|c| c.label.as_str()) {
                Some("ja") | Some(FULLWIDTH) => "ja",
                _ => "other",
            };
            segs.push(seg(&text, lang));
        } else if self.raw {
            text = self.keys.clone();
            segs.push(seg(&text, "raw"));
        } else if let Some((c, _, _)) = self.current() {
            // 일본어 구간은 Mozc로 확정 (학습). 전각 후보는 그대로.
            text = String::new();
            for (i, (s, shown)) in c.cand.segments.iter().zip(c.texts.iter()).enumerate() {
                if s.lang == Some(Lang::Ja) && c.label != FULLWIDTH && !(is_kana_label(&c.label) && c.mozc_pick.is_none()) {
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
            if c.label == HANJA && self.learn {
                crate::hanja::learn(&c.cand.segments[0].text, &text);
            }
            // 학습: 확정한 형태를 기록하고 저장한다 (사전 후보는 판정 학습에 넣지 않는다)
            if c.label == DICT || !self.learn {
            } else if let Ok(mut h) = self.history.lock() {
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
            segs.push(seg(tail, if tail == "\u{3000}" { "ja" } else { "other" }));
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

    /// 전역 상태(언어 고정)를 건드리는 시험은 하나씩 돈다
    static GLOBAL: Mutex<()> = Mutex::new(());

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
    fn indicator_and_lock() {
        let _g = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(m) = models() else { return };
        let mut s = session(&m);
        let st = crate::settings::Settings::default();
        let r = type_keys(&mut s, "dkssud");
        assert_eq!(s.indicator(&r), ipc::Indicator { lang: "ko".into(), locked: false });
        // 한/영: 조합을 확정하고 한국어 고정
        let r = s.cycle_lock(&st);
        assert_eq!(r.commit.as_deref(), Some("안녕"));
        assert_eq!(s.indicator(&r), ipc::Indicator { lang: "ko".into(), locked: true });
        // 고정 중에는 영어로 보일 키도 한국어
        let r = type_keys(&mut s, "hello");
        assert!(r.preedit.iter().all(|p| p.lang == "ko"), "{:?}", r.preedit);
        s.reset();
        crate::lock::set(None);
        s.apply_settings(&st);
    }

    #[test]
    fn kana_conversions() {
        assert_eq!(to_katakana("きゃっと〜ー"), "キャット〜ー");
        assert_eq!(to_halfwidth_katakana("ガッコウ"), "ｶﾞｯｺｳ");
        assert_eq!(to_halfwidth_katakana("パーティー"), "ﾊﾟｰﾃｨｰ");
        assert_eq!(to_halfwidth_katakana("ヴァイオリン"), "ｳﾞｧｲｵﾘﾝ");
        assert_eq!(to_halfwidth_katakana("ヲンヮ"), "ｦﾝﾜ");
        assert_eq!(to_halfwidth_katakana("ポケモン"), "ﾎﾟｹﾓﾝ");
        assert_eq!(to_halfwidth_katakana("ジュース"), "ｼﾞｭｰｽ");
        assert_eq!(to_halfwidth_katakana("ヅヂブベ"), "ﾂﾞﾁﾞﾌﾞﾍﾞ");
    }

    #[test]
    fn kana_candidates_first() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        type_keys(&mut s, "arigatou");
        let list = s.shift_tap().candidates.expect("window");
        let i = list.items.iter().position(|t| t == "ありがとう").expect("hiragana");
        assert_eq!(list.items[i + 1], "アリガトウ");
        assert_eq!(list.items[i + 2], "ｱﾘｶﾞﾄｳ");
        assert_eq!(list.labels[i], "ひらがな");
        // Mozc 없이 보이는 것은 ありがとう이므로 첫 Shift 탭은 가타카나
        assert_eq!(list.items[list.selected], "アリガトウ");
        let r = s.key(&key(VK_RETURN, false));
        assert_eq!(r.commit.as_deref(), Some("アリガトウ"));
    }

    #[test]
    fn user_dictionary() {
        let Some(m) = models() else { return };
        crate::userdict::set_for_test(&[("ghub", "GitHub"), ("yrsk", "よろしくお願いします"), ("yrsk", "宜しく")]);
        let mut s = session(&m);
        type_keys(&mut s, "yrsk");
        let list = s.shift_tap().candidates.expect("window");
        assert_eq!(list.items[0], "よろしくお願いします");
        assert_eq!(list.items[1], "宜しく");
        assert_eq!(list.labels[0], "사전");
        assert_eq!(list.selected, 0, "첫 Shift 탭이 사전 후보를 고른다");
        let r = s.key(&key(VK_RETURN, false));
        assert_eq!(r.commit.as_deref(), Some("よろしくお願いします"));
        // 대소문자 무시
        type_keys(&mut s, "GHub");
        let list = s.shift_tap().candidates.expect("window");
        assert_eq!(list.items[0], "GitHub");
    }

    #[test]
    fn page_helpers() {
        assert_eq!(page_start(0), 0);
        assert_eq!(page_start(8), 0);
        assert_eq!(page_start(9), 9);
        // 20개 = 3쪽 (0, 9, 18)
        assert_eq!(page_move(3, 20, true), 9);
        assert_eq!(page_move(9, 20, true), 18);
        assert_eq!(page_move(19, 20, true), 0);
        assert_eq!(page_move(0, 20, false), 18);
        let items: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        let l = page_list(items.clone(), items.clone(), 10);
        assert_eq!((l.items.len(), l.selected, l.page, l.pages), (9, 1, 2, 3));
        assert_eq!(l.items[0], "9");
        let l = page_list(items.clone(), items, 19);
        assert_eq!((l.items.len(), l.selected, l.page), (2, 1, 3));
        let few: Vec<String> = (0..5).map(|i| i.to_string()).collect();
        let l = page_list(few.clone(), few, 2);
        assert_eq!((l.items.len(), l.page, l.pages), (5, 0, 0));
    }

    #[test]
    fn hanja_many_pages() {
        let Some(m) = models() else { return };
        // 한자 후보가 많은 읽기: 한 쪽(9개)을 넘어 Page Down과 숫자로 2쪽의 후보를 고른다
        let many: Vec<String> = (0..25).map(|i| char::from_u32(0x4E00 + i).unwrap().to_string()).collect();
        let refs: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
        crate::hanja::load_for_test(&[("가", &refs)]);
        let mut s = session(&m);
        type_keys(&mut s, "rk");
        // 첫 Shift 탭은 다음 언어 묶음(영어)으로 가므로 한자 25개 뒤의 쪽이 열린다
        let first = s.shift_tap().candidates.expect("window");
        assert!(first.pages >= 3 && first.page >= 3, "page {}/{}", first.page, first.pages);
        let r = s.key(&key(VK_PRIOR, false));
        let second = r.candidates.expect("window");
        assert_eq!(second.page, first.page - 1);
        assert_eq!(second.selected, 0);
        let want = second.items[2].clone();
        let r = s.key(&key(0x33, false)); // 숫자 3: 2쪽의 세 번째
        assert_eq!(r.preedit_text(), want);
    }

    #[test]
    fn language_selection() {
        let _g = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(m) = models() else { return };
        let mut s = session(&m);
        let mut st = crate::settings::Settings::default();
        // 일본어를 끄면 로마자 일본어도 일본어로 판정하지 않는다
        st.languages.ja = false;
        s.apply_settings(&st);
        let r = type_keys(&mut s, "watashiha");
        assert!(r.preedit.iter().all(|p| p.lang != "ja"), "{:?}", r.preedit);
        let list = s.shift_tap().candidates.unwrap_or_default();
        assert!(list.labels.iter().all(|l| !l.contains("ja")), "{:?}", list.labels);
        s.reset();
        // 한국어를 끄면 두벌식 한국어도 한국어가 아니다
        st.languages = crate::settings::Languages { ko: false, ja: true };
        s.apply_settings(&st);
        let r = type_keys(&mut s, "dkssud");
        assert!(r.preedit.iter().all(|p| p.lang != "ko"), "{:?}", r.preedit);
        s.reset();
        s.apply_settings(&crate::settings::Settings::default());
        let r = type_keys(&mut s, "dkssud");
        assert_eq!(r.preedit_text(), "안녕");
    }

    #[test]
    fn app_disabled_matching() {
        let st = crate::settings::Settings { disabled_apps: vec!["Game.exe".into(), "photoshop".into()], ..Default::default() };
        assert!(st.app_disabled("game.EXE"));
        assert!(st.app_disabled("Photoshop.exe"));
        assert!(!st.app_disabled("notepad.exe"));
        assert!(!st.app_disabled(""));
    }

    #[test]
    fn hanja_candidates() {
        let Some(m) = models() else { return };
        crate::hanja::load_for_test(&[("학교", &["學校", "學敎"])]);
        let mut s = session(&m);
        type_keys(&mut s, "gkrry");
        let list = s.shift_tap().candidates.expect("window");
        let i = list.items.iter().position(|t| t == "學校").expect("hanja candidate");
        assert_eq!(list.labels[i], "漢");
        assert!(list.items.iter().any(|t| t == "學敎"));
        // 고르면 한자로 확정되고, 다음에는 고른 것이 먼저
        s.key(&key(0x31 + i as u16, false)); // 숫자 (1~5 안일 때만)
        if i < 5 {
            assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("學校 "));
        }
        crate::hanja::learn("학교", "學敎");
        crate::hanja::learn("학교", "學敎");
        assert_eq!(crate::hanja::candidates("학교", 9)[0], "學敎");
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
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("わたしは\u{3000}")); // 일본어는 확정 + 전각 공백
        // 장음: 글자 뒤 '-'는 단어 안의 ー
        assert_eq!(type_keys(&mut s, "ko-hi-").preedit_text(), "こーひー");
        // 일본어 문맥의 마침표: 단어를 확정하고 기호 조합 。을 시작한다
        assert!(s.test(&key(0xBE, false)).eaten);
        let r = s.key(&key(0xBE, false));
        assert_eq!(r.commit.as_deref(), Some("こーひー"));
        assert_eq!(r.preedit_text(), "。");
        // 다음 기호는 이어 붙는다 (、는 따로 확정되지 않고 조합 안에서 。、)
        assert_eq!(s.key(&key(0xBC, false)).preedit_text(), "。、");
        // Space: 일본어 문맥이면 공백 없이 확정
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("。、\u{3000}"));
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
        // 한국어 문맥의 마침표: 단어를 확정하고 기호 조합 '.'
        let r = s.key(&key(0xBE, false));
        assert_eq!(r.commit.as_deref(), Some("하"));
        assert_eq!(r.preedit_text(), ".");
        assert_eq!(s.flush().commit.as_deref(), Some("."));
        assert!(!s.test(&key(VK_SPACE, false)).eaten);
    }

    #[test]
    fn untrusted_connection_does_not_learn() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        s.set_learning(false);
        let first = type_keys(&mut s, "go").preedit_text();
        let list = s.shift_tap().candidates.expect("candidate window");
        let sel = list.selected;
        if is_kana_label(&list.labels[sel]) {
            return;
        }
        s.key(&key(0x31 + sel as u16, false));
        let committed = s.key(&key(VK_SPACE, false)).commit.unwrap_or_default();
        assert!(committed.starts_with(&list.items[sel]));
        // 확정은 되지만 기록되지 않는다: 같은 키의 1위가 그대로
        assert_eq!(s.history.lock().unwrap().len(), 0);
        s.reset();
        assert_eq!(type_keys(&mut s, "go").preedit_text(), first);
    }

    #[test]
    fn candidate_window_and_learning() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        let first = type_keys(&mut s, "go").preedit_text();
        let r = s.shift_tap();
        let list = r.candidates.clone().expect("candidate window");
        assert!(list.items.len() >= 2);
        // 처음 선택은 다음 언어 묶음의 첫 후보 (go → 해)
        let sel = list.selected;
        assert!(sel >= 1 && sel < 5, "{:?}", list.items);
        assert_eq!(r.preedit_text(), list.items[sel]);
        assert!(r.preedit[0].focused);
        let second = list.items[sel].clone();
        assert_ne!(first, second);
        // Backspace: 후보창 닫고 친 키 그대로(로마자)
        let r = s.key(&key(VK_BACK, false));
        assert_eq!(r.preedit_text(), "go");
        assert_eq!(r.preedit[0].lang, "raw");
        assert!(r.candidates.is_none());
        s.reset();
        assert_eq!(type_keys(&mut s, "go").preedit_text(), first);
        // 다시 열고 숫자로 고른 뒤 Space로 확정
        s.shift_tap();
        assert_eq!(s.key(&key(0x31 + sel as u16, false)).preedit_text(), second);
        let committed = s.key(&key(VK_SPACE, false)).commit.unwrap_or_default();
        assert!(committed.starts_with(&second));
        // 학습: 같은 키를 다시 치면 고른 후보가 1위
        // (가나 변환 후보는 Mozc가 학습하므로, Mozc 없는 이 시험에서는 확인하지 않는다)
        s.reset();
        if !is_kana_label(&list.labels[sel]) {
            assert_eq!(type_keys(&mut s, "go").preedit_text(), second);
        }
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
        let n = list.items.len();
        let sel = list.selected;
        assert!(s.test(&key(VK_DOWN, false)).eaten);
        assert_eq!(s.key(&key(VK_DOWN, false)).candidates.unwrap().selected, (sel + 1) % n);
        assert_eq!(s.key(&key(VK_UP, false)).candidates.unwrap().selected, sel);
        for _ in 0..sel {
            s.key(&key(VK_LEFT, false));
        }
        assert_eq!(s.key(&key(VK_UP, false)).candidates.unwrap().selected, n - 1);
    }

    #[test]
    fn all_languages_and_tab_groups() {
        let Some(m) = models() else { return };
        crate::hanja::load_for_test(&[("해", &["海", "害"])]);
        let mut s = session(&m);
        // go: 문맥과 상관없이 영어(go, ｇｏ), 한국어(해 + 한자), 일본어(ご, Mozc 없으면 가나) 묶음이 모두 나온다
        type_keys(&mut s, "go");
        let list = s.shift_tap().candidates.unwrap();
        for want in ["go", "ｇｏ", "해", "海", "ご"] {
            assert!(list.items.iter().any(|t| t == want), "{want} missing: {:?}", list.items);
        }
        // 묶음 시작 위치들
        let lang_of = |t: &str| match t {
            "go" | "ｇｏ" => "en",
            "해" | "海" | "害" => "ko",
            "ご" | "ゴ" | "ｺﾞ" => "ja",
            _ => "mix",
        };
        let starts: Vec<usize> =
            (0..list.items.len()).filter(|&i| i == 0 || lang_of(&list.items[i]) != lang_of(&list.items[i - 1])).collect();
        // 처음 선택은 두 번째 묶음의 첫 후보 (일본어가 1위면 다음 변환 후보)
        assert!(list.selected == starts[1] || (lang_of(&list.items[0]) == "ja" && list.selected == 1), "{:?} sel {}", list.items, list.selected);
        // Tab: 다음 묶음의 첫 후보, Shift+Tab: 이전 묶음
        let cur = starts.iter().rposition(|&x| x <= list.selected).unwrap();
        let t = s.key(&key(VK_TAB, false)).candidates.unwrap();
        assert_eq!(t.selected, starts[(cur + 1) % starts.len()], "{:?}", t.items);
        let b = s.key(&key(VK_TAB, true)).candidates.unwrap();
        assert_eq!(b.selected, starts[cur]);
    }

    #[test]
    fn split_keys_by_mozc_readings() {
        let r = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(split_by_readings("kyouhaiitennkidesu", &r(&["きょうは", "いいてんきです"])), Some(vec![(0, 6), (6, 18)]));
        assert_eq!(split_by_readings("kaigishitsuwoyoyakushita", &r(&["かいぎしつを", "よやくした"])), Some(vec![(0, 13), (13, 24)]));
        // nn (ん) 과 っ 을 앞 문절이 다 먹는다
        assert_eq!(split_by_readings("konnnichiha", &r(&["こんにちは"])), None);
        assert_eq!(split_by_readings("kannnaimatte", &r(&["かんない", "まって"])), Some(vec![(0, 7), (7, 12)]));
        assert_eq!(split_by_readings("abc", &r(&["きょう", "は"])), None);
    }

    #[test]
    fn parts_conversion_with_arrows() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        // API를: 영어 + 한국어 두 부분 (Mozc 없이도 언어 구간으로 나뉜다)
        assert_eq!(type_keys(&mut s, "APIfmf").preedit_text(), "API를");
        let r = s.shift_tap();
        assert_eq!(r.preedit.len(), 2);
        assert!(r.preedit[0].focused && !r.preedit[1].focused);
        assert_eq!(r.candidates.as_ref().unwrap().items[0], "API");
        // → 로 다음 부분, 후보는 그 부분의 것
        let r = s.key(&key(VK_RIGHT, false));
        assert!(r.preedit[1].focused);
        let items = r.candidates.unwrap().items;
        assert_eq!(items[0], "를");
        assert!(items.iter().any(|t| t == "fmf"), "{items:?}");
        // ↓ 로 그 부분의 다음 후보, 앞 부분은 그대로
        let r = s.key(&key(VK_DOWN, false));
        assert_eq!(r.preedit[0].text, "API");
        assert_ne!(r.preedit[1].text, "를");
        let changed = r.preedit_text();
        // ← 로 돌아가도 고른 것은 유지
        let r = s.key(&key(VK_LEFT, false));
        assert!(r.preedit[0].focused);
        assert_eq!(r.preedit_text(), changed);
        // Space: 이어 붙여 확정
        let c = s.key(&key(VK_SPACE, false)).commit.unwrap();
        assert!(c.starts_with(&changed), "{c}");
        // 한 부분짜리 입력은 지금처럼 후보창 하나
        type_keys(&mut s, "go");
        assert!(s.shift_tap().candidates.is_some());
        assert!(s.parts.is_none());
    }

    #[test]
    fn ja_space_modes() {
        let Some(m) = models() else { return };
        // 기본(commit): 확정 + 전각 공백. 조합이 없을 때 Space는 문맥과 상관없이 앱에 넘긴다 (전각은 Shift+Space)
        let mut s = session(&m);
        s.ja_space_override = Some(crate::settings::JaSpace::Commit);
        type_keys(&mut s, "watashiha");
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("わたしは\u{3000}"));
        assert!(!s.test(&key(VK_SPACE, false)).eaten);
        // 한국어 문맥은 그대로 확정 + 공백
        type_keys(&mut s, "dkssud");
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("안녕 "));
        assert!(!s.test(&key(VK_SPACE, false)).eaten);

        // continue: 확정하지 않고 이어 간다 → 문장 한 번에, 마침표에서 확정
        let mut s = session(&m);
        s.ja_space_override = Some(crate::settings::JaSpace::Continue);
        type_keys(&mut s, "watashiha");
        let r = s.key(&key(VK_SPACE, false));
        assert!(r.commit.is_none());
        assert_eq!(r.preedit_text(), "わたしは");
        let r = type_keys(&mut s, "gakuseidesu");
        assert_eq!(r.preedit_text(), "わたしはがくせいです");
        assert_eq!(s.key(&key(0xBE, false)).commit.as_deref(), Some("わたしはがくせいです"));
        s.reset();
        // 이어서 변환 중 한국어 단어가 오면 앞의 일본어를 확정하고 한국어만 조합
        type_keys(&mut s, "watashiha");
        s.key(&key(VK_SPACE, false));
        let r = type_keys(&mut s, "dk");
        assert_eq!(r.preedit_text(), "아");
        let r = type_keys(&mut s, "ssud");
        assert_eq!(r.preedit_text(), "안녕");
        assert_eq!(s.key(&key(VK_SPACE, false)).commit.as_deref(), Some("안녕 "));
    }

    #[test]
    fn shift_space_is_fullwidth_space() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        // 조합이 없을 때도 먹고 전각 공백만 넣는다
        assert!(s.test(&key(VK_SPACE, true)).eaten);
        assert_eq!(s.key(&key(VK_SPACE, true)).commit.as_deref(), Some("\u{3000}"));
        // 조합 중이면 확정 + 전각 공백
        type_keys(&mut s, "watashiha");
        assert_eq!(s.key(&key(VK_SPACE, true)).commit.as_deref(), Some("わたしは\u{3000}"));
        // 조합이 없을 때 보통 Space는 문맥과 상관없이 앱에 넘긴다 (반각 공백)
        assert!(!s.test(&key(VK_SPACE, false)).eaten);
    }

    #[test]
    fn symbol_composition_and_variants() {
        let Some(m) = models() else { return };
        let mut s = session(&m);
        // 기호는 조합으로 들어가고, Shift 탭으로 변형 후보
        assert!(s.test(&key(0xBF, true)).eaten); // ?
        assert_eq!(s.key(&key(0xBF, true)).preedit_text(), "?");
        let list = s.shift_tap().candidates.unwrap();
        assert!(list.items.iter().any(|t| t == "？"), "{:?}", list.items);
        assert!(list.items.iter().any(|t| t == "⁉"), "{:?}", list.items);
        // 글자를 치면 기호를 확정하고 글자 조합 시작
        let r = s.key(&key(b'D' as u16, false));
        assert!(r.commit.is_some());
        s.reset();
        // -> 는 화살표 후보
        s.key(&key(0xBD, false));
        assert_eq!(s.key(&key(0xBE, true)).preedit_text(), "->");
        let list = s.shift_tap().candidates.unwrap();
        assert!(list.items.iter().any(|t| t == "→"), "{:?}", list.items);
        s.reset();
        // 숫자: 전각·원 숫자
        s.key(&key(0x33, false));
        let list = s.shift_tap().candidates.unwrap();
        for want in ["3", "３", "③", "三"] {
            assert!(list.items.iter().any(|t| t == want), "{want}: {:?}", list.items);
        }
        // 기호 조합 중 Backspace는 한 글자씩
        s.reset();
        s.key(&key(0x31, true));
        s.key(&key(0x31, true));
        assert_eq!(s.key(&key(VK_BACK, false)).preedit_text(), "!");
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

    #[test]
    fn long_vowel_after_english_looking_token() {
        let Some(m) = models() else { return };
        // 앞 토큰(me, to, so)이 영어로 판정돼도 '-'는 장음으로 조합에 들어간다
        for (keys, want) in [("me-ru", "めーる"), ("to-ru", "とーる"), ("so-su", "そーす"), ("be-su", "べーす")] {
            let mut s = session(&m);
            let r = type_keys(&mut s, keys);
            assert!(r.commit.is_none(), "{keys}: committed {:?}", r.commit);
            assert_eq!(r.preedit_text(), want, "{keys}");
        }
        // 가나로 읽히지 않는 단어 뒤 '-'는 지금처럼 기호 (영어 하이픈)
        let mut s = session(&m);
        type_keys(&mut s, "well");
        let r = s.key(&key(VK_OEM_MINUS, false));
        assert_eq!(r.commit.as_deref(), Some("well"));
        assert_eq!(r.preedit_text(), "-");
    }

    #[test]
    fn long_vowel_suggestion_from_pronunciation() {
        assert_eq!(long_vowel_keys("koohii").as_deref(), Some("ko-hi-"));
        assert_eq!(long_vowel_keys("raamen").as_deref(), Some("ra-men"));
        assert_eq!(long_vowel_keys("kyuu").as_deref(), Some("kyu-"));
        assert_eq!(long_vowel_keys("toukyou"), None);
        assert_eq!(long_vowel_keys("sakura"), None);
        let Some(m) = models() else { return };
        for (keys, want) in [("koohii", "コーヒー"), ("keeki", "ケーキ"), ("biiru", "ビール"), ("deeta", "データ")] {
            let mut s = session(&m);
            type_keys(&mut s, keys);
            let list = s.shift_tap().candidates.expect("candidate window");
            assert!(list.items.iter().any(|i| i == want), "{keys}: {:?}", list.items);
        }
        // 고르면 ー로 확정된다
        let mut s = session(&m);
        type_keys(&mut s, "koohii");
        let mut r = s.shift_tap();
        for _ in 0..20 {
            if r.preedit_text() == "コーヒー" {
                break;
            }
            r = s.shift_tap();
        }
        assert_eq!(s.key(&key(VK_RETURN, false)).commit.as_deref(), Some("コーヒー"));
    }
}

