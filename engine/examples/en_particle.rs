//! 영단어 + 일본어 조사 패턴 판정 확인, 그리고 공개 데이터 토큰 오류율 (회귀 확인).
//! 사용: cargo run --release -p engine --example en_particle
use std::path::Path;
use engine::detector::{Detector, Lang};
use engine::model::Models;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let m = Models::load(&root.join("data/engine_model.json")).expect("model");
    let det = Detector::new(&m, vec![Lang::En, Lang::Ko, Lang::Ja]);
    // (키, 기대 형태) — 직접 만든 IT·일상 문장 조각
    let cases = [
        ("APIwo", "en+ja"), ("PCga", "en+ja"), ("gitde", "en+ja"), ("zoomde", "en+ja"), ("pythonno", "en+ja"),
        ("updatewo", "en+ja"), ("serverga", "en+ja"), ("mailwo", "en+ja"), ("chromede", "en+ja"),
        ("APIwotsukatte", "en+ja"), ("PCgakowareta", "en+ja"), ("Gitdekanrisuru", "en+ja"),
        ("pythondekaita", "en+ja"), ("serverniakusesudekinai", "en+ja"), ("mailwookutta", "en+ja"),
        ("chromegaosoi", "en+ja"), ("updatewoshitekudasai", "en+ja"), ("zoomdekaiginisankashita", "en+ja"),
        ("gamewoshiteita", "en+ja"), ("musicwokiiteiru", "en+ja"), ("filega", "en+ja"), ("filewohozonshita", "en+ja"),
        ("downloadshita", "en+ja"), ("loginga", "en+ja"), ("passwordwowasureta", "en+ja"), ("streamnohaishin", "en+ja"),
        ("into", "en"), ("onto", "en"), ("another", "en"), ("together", "en"), ("anyone", "en"), ("photo", "en"),
        ("tomato", "en"), ("kimono", "ja"), ("tokyo", "en"), ("karaoke", "ja"), ("watashiha", "ja"), ("kanojoga", "ja"),
        ("demo", "en"), ("menu", "en"), ("hello", "en"), ("dekiru", "ja"), ("tabemono", "ja"),
    ];
    let (mut ok, mut n) = (0, 0);
    for (k, want) in cases {
        let d = det.decide(k, None);
        let got = d.tokens[0].chosen.form.iter().map(|(l, _)| l.code()).collect::<Vec<_>>().join("+");
        let txt: Vec<String> = d.segments.iter().map(|s| s.text.clone()).collect();
        n += 1;
        if got == want { ok += 1 } else { println!("  MISS {k}: want {want}, got {got} {txt:?}") }
    }
    println!("pattern set: {ok}/{n}");
    if std::env::args().any(|a| a == "--pattern-only") { return; }
    // 공개 데이터 토큰 오류율
    for f in ["en_news", "ko_news", "ko_review", "ja_news_bunsetsu", "ja_tatoeba_bunsetsu"] {
        let text = std::fs::read_to_string(root.join(format!("data/public/{f}.jsonl"))).unwrap_or_default();
        let (mut err, mut tot) = (0usize, 0usize);
        for line in text.lines().take(2000) {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            let raw = v["keys"].as_str().unwrap();
            let gold: Vec<Vec<(String, usize)>> = serde_json::from_value(v["gold"].clone()).unwrap();
            // 문절 파일은 '|'로 나뉜 조각을 따로 (사용자가 문절마다 띄어 치는 것과 같다)
            let mut toks = Vec::new();
            let mut prev = None;
            for piece in raw.split('|') {
                let d = det.decide(piece, prev);
                if let Some(t) = d.tokens.last() { prev = Some(t.chosen.end()); }
                toks.extend(d.tokens);
            }
            if toks.len() != gold.len() { continue; }
            for (t, g) in toks.iter().zip(gold.iter()) {
                tot += 1;
                let got: Vec<(String, usize)> = t.chosen.form.iter().map(|(l, c)| (l.code().to_string(), *c)).collect();
                if &got != g { err += 1; }
            }
        }
        println!("{f}: token error {:.2}% ({err}/{tot})", 100.0 * err as f64 / tot.max(1) as f64);
    }
}
