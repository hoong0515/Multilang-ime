//! 긴 토큰(띄어쓰기 없이 길게 친 입력)에서 키마다 다시 판정할 때의 시간과 후보.
//! 사용: cargo run --release -p engine --example long_token

use std::path::Path;
use std::time::Instant;

use engine::detector::{Detector, Lang};
use engine::model::Models;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let Ok(models) = Models::load(&root.join("data/engine_model.json")) else {
        eprintln!("model missing");
        return;
    };
    let det = Detector::new(&models, vec![Lang::En, Lang::Ko]);
    let cases = [
        ("한국어 띄어쓰기 없이 (안녕하세요반갑습니다오늘날씨가좋네요)", "dkssudgktpdyqksrkqtmqslekdhsmfskfTlrkwhgspdy"),
        ("영어 띄어쓰기 없이 (thisisaverylongsentencewithoutspaces)", "thisisaverylongsentencewithoutspaces"),
        ("영어 약어 + 한국어 (ESG경영을강화하는방안을발표했다)", "ESGrudduddmfrkdghkgksmsqkddksdmfqkfvygoTek"),
        ("대문자 단어 + 한국어 (YouTube채널을운영하고있습니다)", "YouTubecoskfdmfdnsdudgkrhdlTtmqslek"),
    ];
    for (name, keys) in cases {
        let chars: Vec<char> = keys.chars().collect();
        let mut worst = 0.0f64;
        let mut total = 0.0f64;
        for i in 1..=chars.len() {
            let prefix: String = chars[..i].iter().collect();
            let t = Instant::now();
            let _ = det.decide(&prefix, None);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            worst = worst.max(ms);
            total += ms;
        }
        let dec = det.decide(keys, None);
        let tok = &dec.tokens[0];
        let mut seen: Vec<String> = Vec::new();
        for c in std::iter::once(&tok.chosen).chain(tok.ranked.iter()) {
            if !seen.contains(&c.text()) {
                seen.push(c.text());
            }
        }
        println!("== {name}");
        println!("  keys {} / per-key decide: avg {:.2} ms, worst {:.2} ms", chars.len(), total / chars.len() as f64, worst);
        println!("  candidates {}:", seen.len());
        for (i, t) in seen.iter().take(5).enumerate() {
            println!("    {} {}", i + 1, t);
        }
    }
}
