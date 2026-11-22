//! 언어 판정기 (ime_v0/detector.py 이식, 한국어·영어).
//!
//! 토큰(알파벳 덩어리)마다 후보(언어별 단일 + 토큰 안 언어 래티스)를 만들고,
//! 문장 전체는 언어가 바뀔 때 벌점을 주는 비터비로 고른다.
//! 일본어는 M4에서 로마자 변환기와 함께 붙인다 (Lang::Ja 점수는 아직 없음).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::hangul::compose;
use crate::model::{Models, Params};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ko,
    Ja,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ko => "ko",
            Lang::Ja => "ja",
        }
    }
}

const EXPRESSIVE_JAMO: &str = "ㅋㅎㅠㅜㄷㅇㄱㅅㅂㅈㄴㅊ";
const KO_SHIFT_KEYS: &str = "REQTWOP";
const KO_PARTICLES: &str = "을 를 이 가 은 는 의 에 에서 으로 로 와 과 도 만 까지 부터 이나 나 이랑 랑 하고 처럼 보다 한테 에게 께 \
이다 이고 이라 이라서 이라고 라고 인 인데 이면 면 이야 야 이에요 예요 입니다 같은 같이 쪽 용 적 적인 하다 하는 해서 했다 한 할 함 해 됨 된 되는";
const LATTICE_K: usize = 3;

pub type Form = Vec<(Lang, usize)>;

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub lang: Option<Lang>, // None = 알파벳이 아닌 덩어리 (공백, 숫자, 기호)
    pub keys: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub form: Form,
    pub segments: Vec<Segment>,
    pub score: f64,
}

impl Candidate {
    pub fn start(&self) -> Lang {
        self.form[0].0
    }
    pub fn end(&self) -> Lang {
        self.form[self.form.len() - 1].0
    }
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }
}

#[derive(Clone, Debug)]
pub struct TokenResult {
    pub keys: String,
    pub ranked: Vec<Candidate>,
    pub chosen: Candidate,
}

impl TokenResult {
    pub fn margin(&self) -> f64 {
        if self.ranked.len() > 1 { self.ranked[0].score - self.ranked[1].score } else { f64::INFINITY }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Decision {
    pub segments: Vec<Segment>,
    pub tokens: Vec<TokenResult>,
}

impl Decision {
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect()
    }
}

/// 사용자 선택 기록 (대소문자를 구분해 기억, 대소문자만 다른 기록은 절반 반영).
#[derive(Default)]
pub struct History {
    exact: HashMap<String, HashMap<Form, u32>>,
    folded: HashMap<String, HashMap<Form, u32>>,
}

impl History {
    pub fn record(&mut self, keys: &str, form: &Form) {
        *self.exact.entry(keys.to_string()).or_default().entry(form.clone()).or_default() += 1;
        *self.folded.entry(keys.to_lowercase()).or_default().entry(form.clone()).or_default() += 1;
    }

    pub fn count(&self, keys: &str, form: &Form) -> f64 {
        let ne = self.exact.get(keys).and_then(|m| m.get(form)).copied().unwrap_or(0) as f64;
        let nf = self.folded.get(&keys.to_lowercase()).and_then(|m| m.get(form)).copied().unwrap_or(0) as f64;
        ne + 0.5 * (nf - ne)
    }

    /// 파일로 저장 (항목: [키 입력, 형태, 횟수]). 대소문자 무시 집계는 읽을 때 다시 만든다.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let rows: Vec<(&String, &Form, u32)> =
            self.exact.iter().flat_map(|(k, m)| m.iter().map(move |(f, n)| (k, f, *n))).collect();
        let text = serde_json::to_string(&rows).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn load(path: &Path) -> Self {
        let mut h = History::default();
        let Ok(text) = std::fs::read_to_string(path) else { return h };
        let Ok(rows) = serde_json::from_str::<Vec<(String, Form, u32)>>(&text) else { return h };
        for (k, f, n) in rows {
            *h.exact.entry(k.clone()).or_default().entry(f.clone()).or_default() += n;
            *h.folded.entry(k.to_lowercase()).or_default().entry(f).or_default() += n;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.exact.values().map(|m| m.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.exact.is_empty()
    }
}

/// 여러 연결(앱)이 함께 쓰는 학습 기록.
pub type SharedHistory = Arc<Mutex<History>>;

pub struct Detector<'m> {
    m: &'m Models,
    p: Params,
    pub langs: Vec<Lang>,
    pub history: Option<SharedHistory>,
    memo: RefCell<HashMap<(Lang, String), f64>>,
    particles_ko: HashSet<&'static str>,
    /// 언어 전환 벌점 배율. 직전 단어들이 같은 언어로 이어질수록 키운다 (set_language_run).
    switch_scale: f64,
}

/// 같은 언어가 이어진 단어 수에 따른 전환 벌점 가중치 (공개·트윗 데이터에서 0.3이 어느 셋도 나빠지지 않음).
pub const MOMENTUM: f64 = 0.3;

fn is_upper(s: &str) -> bool {
    // Python str.isupper(): 대소문자가 있는 글자가 하나 이상이고 모두 대문자
    s.chars().any(|c| c.is_ascii_alphabetic()) && !s.chars().any(|c| c.is_ascii_lowercase())
}

fn lower_after_caps(s: &str) -> bool {
    // [A-Z]{2,}[a-z]{3,}$
    let cs: Vec<char> = s.chars().collect();
    let mut i = cs.len();
    let mut lower = 0;
    while i > 0 && cs[i - 1].is_ascii_lowercase() {
        i -= 1;
        lower += 1;
    }
    let mut upper = 0;
    while i > 0 && cs[i - 1].is_ascii_uppercase() {
        i -= 1;
        upper += 1;
    }
    lower >= 3 && upper >= 2
}

impl<'m> Detector<'m> {
    pub fn new(m: &'m Models, langs: Vec<Lang>) -> Self {
        Self {
            m,
            p: m.params.clone(),
            langs,
            history: None,
            memo: RefCell::new(HashMap::new()),
            particles_ko: KO_PARTICLES.split_whitespace().collect(),
            switch_scale: 1.0,
        }
    }

    pub fn params(&self) -> &Params {
        &self.p
    }

    /// 직전에 같은 언어로 확정한 단어 수(run)를 알려 준다. 길게 이어질수록 다른 언어로 바뀌기 어렵게 한다.
    pub fn set_language_run(&mut self, run: usize) {
        self.switch_scale = 1.0 + MOMENTUM * (run.saturating_sub(1).min(3) as f64);
    }

    fn memo_get(&self, lang: Lang, keys: &str) -> Option<f64> {
        self.memo.try_borrow().ok()?.get(&(lang, keys.to_string())).copied()
    }

    fn memo_put(&self, lang: Lang, keys: &str, v: f64) -> f64 {
        if let Ok(mut m) = self.memo.try_borrow_mut() {
            m.insert((lang, keys.to_string()), v);
        }
        v
    }

    fn score_en(&self, keys: &str) -> f64 {
        if let Some(v) = self.memo_get(Lang::En, keys) {
            return v;
        }
        let low = keys.to_lowercase();
        let mut s = self.m.en.logprob(&low);
        let z = self.m.en_zipf.get(&low).copied().unwrap_or(0.0);
        s += self.p.en_dict * (z - self.p.zipf_floor).max(0.0);
        if is_upper(keys) && (keys.chars().count() >= 2 || !KO_SHIFT_KEYS.contains(keys)) {
            s += self.p.caps_en;
        } else if lower_after_caps(keys) {
            s -= self.p.case_shape;
        }
        self.memo_put(Lang::En, keys, s)
    }

    fn score_ko(&self, keys: &str) -> f64 {
        if let Some(v) = self.memo_get(Lang::Ko, keys) {
            return v;
        }
        let mut s = self.m.ko.logprob(&keys.to_lowercase());
        let c = compose(keys);
        let expr = c.orphan_list.iter().filter(|j| EXPRESSIVE_JAMO.contains(**j)).count() as f64;
        s -= self.p.orphan * (c.orphans() as f64 - expr) + self.p.orphan_expr * expr;
        let noise = keys.chars().filter(|ch| ch.is_ascii_uppercase() && !KO_SHIFT_KEYS.contains(*ch)).count() as f64;
        s -= self.p.shift_noise * noise;
        let z = self.m.ko_zipf.get(&c.text).copied().unwrap_or(0.0);
        s += self.p.ko_dict * (z - self.p.zipf_floor).max(0.0);
        self.memo_put(Lang::Ko, keys, s)
    }

    pub fn score(&self, lang: Lang, keys: &str) -> f64 {
        match lang {
            Lang::En => self.score_en(keys),
            Lang::Ko => self.score_ko(keys),
            Lang::Ja => f64::NEG_INFINITY, // M4에서 구현
        }
    }

    pub fn render(lang: Lang, keys: &str) -> String {
        match lang {
            Lang::Ko => compose(keys).text,
            Lang::En | Lang::Ja => keys.to_string(),
        }
    }

    fn suffix(&self, lang: Lang, keys: &str) -> f64 {
        let mut s = self.score(lang, keys);
        if lang == Lang::Ko && self.particles_ko.contains(Self::render(lang, keys).as_str()) {
            s += self.p.particle;
        }
        s
    }

    fn en_entry(keys: &[char], j: usize, prev: Lang) -> bool {
        if prev == Lang::Ko {
            return keys[j].is_ascii_uppercase();
        }
        keys[j..(j + 4).min(keys.len())].iter().any(|c| c.is_ascii_uppercase())
    }

    fn lattice(&self, keys: &str, start: Lang) -> Vec<(f64, Vec<(Lang, usize, usize)>)> {
        type Path = Vec<(Lang, usize, usize)>;
        let cs: Vec<char> = keys.chars().collect();
        let n = cs.len();
        let ms = self.p.min_split;
        // best[k] = 언어별 경로 목록 (삽입 순서 유지: Python dict와 같은 동점 처리)
        let mut best: Vec<Vec<(Lang, Vec<(f64, Path)>)>> = vec![Vec::new(); n + 1];
        let upper: Vec<bool> = cs.iter().map(|c| c.is_ascii_uppercase()).collect();
        let entry = |l: Lang, j: usize| -> bool { j == n || Self::en_entry(&cs, j, l) };
        let has_ko = self.langs.contains(&Lang::Ko);
        let has_ja = self.langs.contains(&Lang::Ja);

        let push = |best: &mut Vec<Vec<(Lang, Vec<(f64, Path)>)>>, k: usize, lang: Lang, v: f64, path: Path| {
            let slot = match best[k].iter().position(|(l, _)| *l == lang) {
                Some(i) => i,
                None => {
                    best[k].push((lang, Vec::new()));
                    best[k].len() - 1
                }
            };
            let lst = &mut best[k][slot].1;
            lst.push((v, path));
            lst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            lst.truncate(LATTICE_K);
        };
        let min_len = |lang: Lang| if lang == Lang::En { ms } else { ms.max(2) };
        let may_end = |lang: Lang, i: usize, j: usize| -> bool {
            if j == n {
                return true;
            }
            if lang != Lang::En {
                return entry(lang, j);
            }
            if upper[i..j].iter().any(|&u| u) {
                return true;
            }
            (has_ko && n - j <= self.p.lower_suffix_max) || (has_ja && n - j <= 8)
        };

        for j in min_len(start)..=n {
            if may_end(start, 0, j) {
                let s = self.score(start, &keys_slice(&cs, 0, j));
                push(&mut best, j, start, s, vec![(start, 0, j)]);
            }
        }
        if n >= ms {
            for j in ms..=(n - ms) {
                let snapshot: Vec<(Lang, Vec<(f64, Path)>)> = best[j].clone();
                for (l, lst) in snapshot {
                    for (sc, path) in lst {
                        for &l2 in &self.langs {
                            if l2 == l || matches!((l, l2), (Lang::Ko, Lang::Ja) | (Lang::Ja, Lang::Ko)) {
                                continue;
                            }
                            if l2 == Lang::En && !entry(l, j) {
                                continue;
                            }
                            let mut ks: Vec<usize> = ((j + min_len(l2))..=n).collect();
                            let mut extra = 0.0;
                            let seg_start = path[path.len() - 1].1;
                            if l == Lang::En && !upper[seg_start..j].iter().any(|&u| u) {
                                if l2 == Lang::Ko && n - j <= self.p.lower_suffix_max {
                                    // 소문자 영어 + 한국어 어미 (vga카드로는)
                                } else {
                                    continue; // 일본어 조사 경우는 M4에서
                                }
                                ks = vec![n];
                                extra = self.p.lower_en_split;
                            }
                            for k in ks {
                                if !may_end(l2, j, k) {
                                    continue;
                                }
                                let seg = keys_slice(&cs, j, k);
                                let v = sc + self.suffix(l2, &seg) - self.p.split - extra;
                                let mut p2 = path.clone();
                                p2.push((l2, j, k));
                                push(&mut best, k, l2, v, p2);
                            }
                        }
                    }
                }
            }
        }
        best.swap_remove(n).into_iter().flat_map(|(_, lst)| lst).collect()
    }

    pub fn candidates(&self, keys: &str) -> Vec<Candidate> {
        let cs: Vec<char> = keys.chars().collect();
        let n = cs.len();
        let mut out: Vec<Candidate> = self
            .langs
            .iter()
            .map(|&l| Candidate {
                form: vec![(l, n)],
                segments: vec![Segment { lang: Some(l), keys: keys.to_string(), text: Self::render(l, keys) }],
                score: self.score(l, keys),
            })
            .collect();
        if n >= 2 * self.p.min_split {
            let mut seen: Vec<Form> = out.iter().map(|c| c.form.clone()).collect();
            for &start in &self.langs {
                for (sc, path) in self.lattice(keys, start) {
                    let form: Form = path.iter().map(|&(l, i, j)| (l, j - i)).collect();
                    if path.len() < 2 || seen.contains(&form) {
                        continue;
                    }
                    seen.push(form.clone());
                    let segments = path
                        .iter()
                        .map(|&(l, i, j)| {
                            let k = keys_slice(&cs, i, j);
                            Segment { lang: Some(l), text: Self::render(l, &k), keys: k }
                        })
                        .collect();
                    out.push(Candidate { form, segments, score: sc });
                }
            }
        }
        if let Some(h) = self.history.as_ref().and_then(|h| h.lock().ok()) {
            for c in &mut out {
                let cnt = h.count(keys, &c.form);
                if cnt > 0.0 {
                    c.score += self.p.history * cnt.ln_1p();
                }
            }
        }
        out
    }

    fn ctx(&self, c: &Candidate, prev: Option<Lang>) -> f64 {
        let Some(prev) = prev else { return c.score };
        if prev == c.start() {
            return c.score;
        }
        let mut pen = self.p.switch * self.switch_scale;
        let first = &c.segments[0].keys;
        if first.chars().count() <= 4 && !is_upper(first) {
            pen += self.p.short_switch;
        }
        if matches!((prev, c.start()), (Lang::Ko, Lang::Ja) | (Lang::Ja, Lang::Ko)) {
            pen += self.p.switch_cjk;
        }
        c.score - pen
    }

    pub fn decide(&self, keys: &str, prev_lang: Option<Lang>) -> Decision {
        enum Item {
            Other(String),
            Cand(usize, usize), // (토큰 번호, 후보 번호)
        }
        let toks = tokenize(keys);
        let mut states: Vec<(Option<Lang>, f64, Vec<Item>)> = vec![(prev_lang, 0.0, Vec::new())];
        let mut cand_cache: Vec<Vec<Candidate>> = Vec::new();
        for t in toks {
            if !t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                for st in &mut states {
                    st.2.push(Item::Other(t.clone()));
                }
                continue;
            }
            let cands = self.candidates(&t);
            let ti = cand_cache.len();
            let mut new: Vec<(Option<Lang>, f64, usize, usize)> = Vec::new(); // (끝 언어, 점수, 상태 번호, 후보 번호)
            for (ci, c) in cands.iter().enumerate() {
                for (si, (l, acc, _)) in states.iter().enumerate() {
                    let tot = acc + self.ctx(c, *l);
                    let end = Some(c.end());
                    match new.iter_mut().find(|x| x.0 == end) {
                        Some(x) => {
                            if tot > x.1 {
                                *x = (end, tot, si, ci);
                            }
                        }
                        None => new.push((end, tot, si, ci)),
                    }
                }
            }
            let mut next = Vec::with_capacity(new.len());
            for (end, tot, si, ci) in new {
                let mut hist: Vec<Item> = Vec::with_capacity(states[si].2.len() + 1);
                for it in &states[si].2 {
                    hist.push(match it {
                        Item::Other(s) => Item::Other(s.clone()),
                        Item::Cand(a, b) => Item::Cand(*a, *b),
                    });
                }
                hist.push(Item::Cand(ti, ci));
                next.push((end, tot, hist));
            }
            cand_cache.push(cands);
            states = next;
        }
        // Python max(): 같은 점수면 먼저 나온 것
        let mut best_i = 0;
        for (i, st) in states.iter().enumerate() {
            if st.1 > states[best_i].1 {
                best_i = i;
            }
        }
        let best = std::mem::take(&mut states[best_i].2);

        let mut dec = Decision::default();
        let mut prev = prev_lang;
        for item in best {
            match item {
                Item::Other(s) => dec.segments.push(Segment { lang: None, keys: s.clone(), text: s }),
                Item::Cand(ti, ci) => {
                    let cands = &cand_cache[ti];
                    let chosen = cands[ci].clone();
                    let mut ranked: Vec<Candidate> = cands
                        .iter()
                        .map(|c| Candidate { form: c.form.clone(), segments: c.segments.clone(), score: self.ctx(c, prev) })
                        .collect();
                    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                    let keys: String = chosen.segments.iter().map(|s| s.keys.as_str()).collect();
                    dec.segments.extend(chosen.segments.iter().cloned());
                    prev = Some(chosen.end());
                    dec.tokens.push(TokenResult { keys, ranked, chosen });
                }
            }
        }
        dec
    }
}

fn keys_slice(cs: &[char], i: usize, j: usize) -> String {
    cs[i..j].iter().collect()
}

/// 알파벳 덩어리와 그 외 덩어리로 나눈다 ([A-Za-z]+|[^A-Za-z]+).
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_alpha: Option<bool> = None;
    for ch in s.chars() {
        let a = ch.is_ascii_alphabetic();
        if cur_alpha.is_some_and(|x| x != a) {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
        cur_alpha = Some(a);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
