//! Python v0 판정기 결과(data/parity_koen.jsonl)와 Rust 엔진 결과 비교.
//! 사용: cargo run --release -p engine --example parity

use std::path::Path;
use std::time::Instant;

use engine::detector::{Detector, Lang};
use engine::model::Models;
use serde::Deserialize;

#[derive(Deserialize)]
struct Row {
    keys: String,
    segments: Vec<(Option<String>, String, String)>,
    top3: Vec<Vec<Vec<(String, usize)>>>,
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let t0 = Instant::now();
    let models = match Models::load(&root.join("data/engine_model.json")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("model load failed: {e}");
            std::process::exit(1);
        }
    };
    println!("model load: {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
    let det = Detector::new(&models, vec![Lang::En, Lang::Ko]);
    let text = std::fs::read_to_string(root.join("data/parity_koen.jsonl")).unwrap_or_default();

    let (mut n, mut seg_ok, mut top3_ok, mut tokens) = (0, 0, 0, 0usize);
    let mut shown = 0;
    let t1 = Instant::now();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<Row>(line) else { continue };
        n += 1;
        let dec = det.decide(&row.keys, None);
        tokens += dec.tokens.len();
        let got: Vec<(Option<String>, String, String)> = dec
            .segments
            .iter()
            .map(|s| (s.lang.map(|l| l.code().to_string()), s.keys.clone(), s.text.clone()))
            .collect();
        if got == row.segments {
            seg_ok += 1;
        } else if shown < 5 {
            shown += 1;
            println!("DIFF segments: {}\n  py: {:?}\n  rs: {:?}", row.keys, row.segments, got);
        }
        let got3: Vec<Vec<Vec<(String, usize)>>> = dec
            .tokens
            .iter()
            .map(|t| {
                t.ranked
                    .iter()
                    .take(3)
                    .map(|c| c.form.iter().map(|(l, k)| (l.code().to_string(), *k)).collect())
                    .collect()
            })
            .collect();
        if got3 == row.top3 {
            top3_ok += 1;
        }
    }
    let dt = t1.elapsed().as_secs_f64();
    println!(
        "sentences {n}: segments match {seg_ok}, top-3 candidates match {top3_ok}; {:.3} ms/token",
        dt * 1000.0 / tokens.max(1) as f64
    );
}
