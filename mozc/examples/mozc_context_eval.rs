//! 일본어 문절 단위 변환 비교: (A) 문절만, (B) 앞 문절 1개 읽기를 붙여 변환 후 지금 문절 부분만, (C) 문장 통째.
//! 지표: 기호를 뺀 글자 오류율(CER). 학습(SUBMIT)은 하지 않는다.
//! 사용: cargo run --release -p mozc --example mozc_context_eval -- <mozc_server.exe> <bunsetsu.jsonl> [문장 수]
use std::io::BufRead;

fn clean(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric() || ('\u{3040}'..='\u{30ff}').contains(c) || ('\u{4e00}'..='\u{9fff}').contains(c)).collect()
}

fn edit(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn romaji(piece: &str) -> String {
    piece.chars().filter(|c| c.is_ascii_alphabetic() || *c == '-').map(|c| c.to_ascii_lowercase()).collect()
}

fn conv(s: &mut mozc::Session, keys: &str) -> mozc::Conversion {
    s.revert().unwrap();
    s.type_keys(keys).unwrap();
    let c = s.convert_current().unwrap();
    s.cancel_conversion().unwrap();
    c
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut s = mozc::Session::new(mozc::Client::connect_or_launch(std::path::Path::new(&a[1])).unwrap()).unwrap();
    let n: usize = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(200);
    let ctx_n: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(1); // 붙일 앞 문절 수
    let f = std::io::BufReader::new(std::fs::File::open(&a[2]).unwrap());
    let (mut ea, mut eb, mut ec, mut tot, mut aligned, mut pieces_n, mut diff) = (0, 0, 0, 0, 0, 0, 0);
    let mut shown = 0;
    for line in f.lines().take(n) {
        let v: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let text = clean(v["text"].as_str().unwrap());
        let pieces: Vec<String> = v["keys"].as_str().unwrap().split('|').map(romaji).filter(|p| !p.is_empty()).collect();
        let (mut out_a, mut out_b) = (String::new(), String::new());
        let mut hist: Vec<(String, String)> = Vec::new(); // 앞 문절들 (romaji, 읽기)
        for p in &pieces {
            let alone = conv(&mut s, p);
            out_a += &alone.text();
            let mut got = alone.text();
            let k0 = hist.len().saturating_sub(ctx_n);
            let prev: Option<(String, String)> = if hist.is_empty() { None } else {
                Some((hist[k0..].iter().map(|x| x.0.clone()).collect(), hist[k0..].iter().map(|x| x.1.clone()).collect()))
            };
            if let Some((pk, pr)) = &prev {
                let c = conv(&mut s, &format!("{pk}{p}"));
                // 앞 문절 읽기와 정확히 맞는 경계가 있으면 그 뒤만 쓴다
                let mut acc = String::new();
                for (i, r) in c.readings.iter().enumerate() {
                    acc += r;
                    if acc == *pr {
                        got = c.segments[i + 1..].concat();
                        aligned += 1;
                        break;
                    }
                    if acc.len() >= pr.len() { break; }
                }
            }
            pieces_n += 1;
            if got != alone.text() {
                diff += 1;
                if shown < 0 { shown += 1; println!("  {} : {} -> {}", prev.as_ref().map(|x| x.0.as_str()).unwrap_or(""), alone.text(), got); }
            }
            out_b += &got;
            hist.push((p.clone(), alone.readings.concat()));
        }
        let whole = conv(&mut s, &pieces.concat()).text();
        let (ca, cb, cc) = (clean(&out_a), clean(&out_b), clean(&whole));
        ea += edit(&ca, &text);
        eb += edit(&cb, &text);
        ec += edit(&cc, &text);
        tot += text.len();
    }
    s.revert().unwrap();
    println!("chars {tot}: CER alone {:.2}%, with prev bunsetsu {:.2}%, whole sentence {:.2}% | aligned {aligned}/{pieces_n}, changed {diff}",
        100.0 * ea as f64 / tot as f64, 100.0 * eb as f64 / tot as f64, 100.0 * ec as f64 / tot as f64);
}
