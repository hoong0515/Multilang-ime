//! 변환 후보 수 확인: 첫 문절의 후보를 몇 개 받는가 (all_candidate_words).
//! 사용: cargo run -p mozc --example mozc_count -- <mozc_server.exe>
fn main() {
    let exe = std::path::PathBuf::from(std::env::args().nth(1).expect("mozc_server path"));
    let mut s = mozc::Session::new(mozc::Client::connect_or_launch(&exe).unwrap()).unwrap();
    for keys in ["kisya", "kousei", "kanji", "hashi", "seikou"] {
        s.revert().unwrap();
        s.type_keys(keys).unwrap();
        let c = s.convert_current().unwrap();
        s.cancel_conversion().unwrap();
        let shown: Vec<&str> = c.candidates.iter().take(12).map(|(_, v)| v.as_str()).collect();
        println!("{keys}: {} candidates {:?}", c.candidates.len(), shown);
    }
    s.revert().unwrap();
}
