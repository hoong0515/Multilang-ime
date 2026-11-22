//! Mozc 호출 단계별 지연 측정.
//! 사용: cargo run --release -p mozc --example mozc_timing -- <mozc_server.exe>
use std::path::PathBuf;
use std::time::Instant;

fn med(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let exe = PathBuf::from(std::env::args().nth(1).expect("server path"));
    let client = mozc::Client::connect_or_launch(&exe).expect("connect");
    let mut s = mozc::Session::new(client).expect("session");
    let kana = "かいぎしつをよやくした";
    let (mut rv, mut sc, mut sp, mut all, mut noop) = (vec![], vec![], vec![], vec![], vec![]);
    for _ in 0..30 {
        let t = Instant::now();
        s.revert().unwrap();
        rv.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        s.set_composition(kana).unwrap();
        sc.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        s.convert_current().unwrap();
        sp.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        s.convert(kana).unwrap();
        all.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        s.noop().unwrap();
        noop.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("revert {:.2} ms, set_composition {:.2} ms, space-convert {:.2} ms, convert() {:.2} ms, no-op {:.2} ms",
        med(rv), med(sc), med(sp), med(all), med(noop));
    // 가나를 하나씩 늘려 가며 (라이브 입력 흉내)
    let chars: Vec<char> = kana.chars().collect();
    let mut per = vec![];
    for n in 1..=chars.len() {
        let k: String = chars[..n].iter().collect();
        let t = Instant::now();
        s.convert(&k).unwrap();
        per.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("incremental convert per kana: {:?}", per.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>());

    // 키 단위: 로마자 하나 보내고 변환 → 변환 취소 (라이브 입력)
    let romaji = "kaigisitsuwoyoyakusita";
    s.revert().unwrap();
    let (mut tk, mut cv, mut cc) = (vec![], vec![], vec![]);
    let mut last = mozc::Conversion::default();
    for c in romaji.chars() {
        let t = Instant::now();
        s.type_keys(&c.to_string()).unwrap();
        tk.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        last = s.convert_current().unwrap();
        cv.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        s.cancel_conversion().unwrap();
        cc.push(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("per key: send {:.2} ms, convert {:.2} ms, cancel {:.2} ms; result {:?}", med(tk), med(cv), med(cc), last.segments);
    let comp = s.type_keys("").unwrap_or_default();
    println!("composition after cancel: {comp:?}");
    s.revert().unwrap();
}
