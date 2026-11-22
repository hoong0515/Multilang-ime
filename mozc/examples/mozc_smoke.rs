//! Mozc Rust 클라이언트 확인: 서버 실행, 변환, 후보 선택, 확정, 지연 시간.
//! 사용: cargo run --release -p mozc --example mozc_smoke -- <mozc_server.exe 경로>

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let exe = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../third_party/mozc_bin/extract/PFiles/Mozc/mozc_server.exe")
    });
    let t = Instant::now();
    let client = match mozc::Client::connect_or_launch(&exe) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("connect failed: {e:?}");
            return;
        }
    };
    println!("connect: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let mut s = match mozc::Session::new(client) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("session failed: {e:?}");
            return;
        }
    };
    for kana in ["きょうはいいてんきです", "きしゃ", "わたしはがくせいです", "こーひー"] {
        let t = Instant::now();
        let c = s.convert(kana);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match c {
            Ok(c) => println!("{kana} -> {:?} ({ms:.1} ms) candidates: {:?}", c.segments, c.candidates.iter().take(5).map(|x| &x.1).collect::<Vec<_>>()),
            Err(e) => println!("{kana} -> error {e:?}"),
        }
    }
    let _ = s.revert();
    let mut lat = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let _ = s.convert("かいぎしつをよやくした");
        lat.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let _ = s.revert();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    println!("convert latency: median {:.1} ms, max {:.1} ms", lat[lat.len() / 2], lat[lat.len() - 1]);
}
