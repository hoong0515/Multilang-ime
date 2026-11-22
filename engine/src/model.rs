//! 모델 파일(data/engine_model.json, ime_v0/export_model.py가 생성) 읽기와 키 입력 n-gram.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde::Deserialize;

const BOS: char = '^';
const EOS: char = '$';

#[derive(Deserialize)]
struct NgramFile {
    order: usize,
    discount: f64,
    vocab_size: usize,
    levels: Vec<HashMap<String, HashMap<String, f64>>>,
    totals: Vec<HashMap<String, (f64, f64)>>,
}

/// 보간 절대 할인 문자 n-gram (ime_v0/ngram.py와 같은 계산).
pub struct CharNgram {
    n: usize,
    d: f64,
    v: f64,
    counts: Vec<HashMap<String, HashMap<char, f64>>>,
    totals: Vec<HashMap<String, (f64, f64)>>,
    cache: Mutex<HashMap<(String, char), f64>>,
}

impl CharNgram {
    fn from_file(f: NgramFile) -> Self {
        let counts = f
            .levels
            .into_iter()
            .map(|lvl| {
                lvl.into_iter()
                    .map(|(ctx, chars)| {
                        let m = chars.into_iter().filter_map(|(c, v)| c.chars().next().map(|ch| (ch, v))).collect();
                        (ctx, m)
                    })
                    .collect()
            })
            .collect();
        Self {
            n: f.order,
            d: f.discount,
            v: f.vocab_size as f64,
            counts,
            totals: f.totals,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn prob(&self, ctx: &[char], ch: char) -> f64 {
        let ctx_s: String = ctx.iter().collect();
        if let Ok(c) = self.cache.lock() {
            if let Some(p) = c.get(&(ctx_s.clone(), ch)) {
                return *p;
            }
        }
        let mut p = 1.0 / self.v;
        for k in 0..self.n {
            let c: String = if k == 0 { String::new() } else { ctx[ctx.len().saturating_sub(k)..].iter().collect() };
            let Some(&(total, types)) = self.totals[k].get(&c) else { continue };
            let cnt = self.counts[k].get(&c).and_then(|m| m.get(&ch)).copied().unwrap_or(0.0);
            p = (cnt - self.d).max(0.0) / total + (self.d * types / total) * p;
        }
        if let Ok(mut c) = self.cache.lock() {
            c.insert((ctx_s, ch), p);
        }
        p
    }

    pub fn logprob(&self, word: &str) -> f64 {
        let mut s: Vec<char> = std::iter::repeat_n(BOS, self.n - 1).collect();
        s.extend(word.chars());
        s.push(EOS);
        let mut total = 0.0;
        for i in (self.n - 1)..s.len() {
            total += self.prob(&s[i + 1 - self.n..i], s[i]).ln();
        }
        total
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Params {
    pub en_dict: f64,
    pub ko_dict: f64,
    pub zipf_floor: f64,
    pub orphan: f64,
    pub orphan_expr: f64,
    pub shift_noise: f64,
    pub switch: f64,
    pub split: f64,
    pub particle: f64,
    pub min_split: usize,
    pub ja_dict: f64,
    pub ja_leftover: f64,
    pub lower_suffix_max: usize,
    pub lower_en_split: f64,
    pub case_shape: f64,
    pub ja_edge: f64,
    pub caps_en: f64,
    pub short_switch: f64,
    pub switch_cjk: f64,
    pub history: f64,
    pub ambiguous: f64,
}

#[derive(Deserialize)]
struct ModelFile {
    params: Params,
    ngram: HashMap<String, NgramFile>,
    zipf: HashMap<String, HashMap<String, f64>>,
}

pub struct Models {
    pub params: Params,
    pub en: CharNgram,
    pub ko: CharNgram,
    pub ja: Option<CharNgram>,
    pub en_zipf: HashMap<String, f64>,
    pub ko_zipf: HashMap<String, f64>,
    pub ja_zipf: HashMap<String, f64>,
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Missing(&'static str),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "io: {e}"),
            LoadError::Json(e) => write!(f, "json: {e}"),
            LoadError::Missing(w) => write!(f, "missing: {w}"),
        }
    }
}

impl Models {
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let text = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        let mut f: ModelFile = serde_json::from_str(&text).map_err(LoadError::Json)?;
        let en = f.ngram.remove("en").ok_or(LoadError::Missing("ngram.en"))?;
        let ko = f.ngram.remove("ko").ok_or(LoadError::Missing("ngram.ko"))?;
        let ja = f.ngram.remove("ja");
        Ok(Self {
            params: f.params,
            en: CharNgram::from_file(en),
            ko: CharNgram::from_file(ko),
            ja: ja.map(CharNgram::from_file),
            en_zipf: f.zipf.remove("en").unwrap_or_default(),
            ko_zipf: f.zipf.remove("ko").unwrap_or_default(),
            ja_zipf: f.zipf.remove("ja").unwrap_or_default(),
        })
    }
}
