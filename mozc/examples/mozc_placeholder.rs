//! 조사로 시작하는 일본어 구간(영단어 뒤)을 Mozc로 변환할 때, 앞에 자리표시 명사를 붙이는 효과.
use std::path::Path;
fn conv(s: &mut mozc::Session, keys: &str) -> mozc::Conversion {
    s.revert().unwrap();
    s.type_keys(keys).unwrap();
    let c = s.convert_current().unwrap();
    s.cancel_conversion().unwrap();
    c
}
/// 자리표시 읽기(ph_kana)를 첫 문절에서 떼어 낸다. 실패하면 None.
fn strip(c: &mozc::Conversion, ph_kana: &str) -> Option<String> {
    let first = c.segments.first()?;
    let r0 = c.readings.first()?;
    if !r0.starts_with(ph_kana) || !first.starts_with(ph_kana) { return None; }
    Some(format!("{}{}", &first[ph_kana.len()..], c.segments[1..].concat()))
}
fn main() {
    let exe = std::env::args().nth(1).unwrap();
    let mut s = mozc::Session::new(mozc::Client::connect_or_launch(Path::new(&exe)).unwrap()).unwrap();
    let cases = [("Youtube", "nidouganoageta"), ("Youtube", "nidougawoageta"), ("API", "wotsukatte"), ("PC", "gakowareta"), ("Git", "dekanrisuru"),
        ("mail", "wookutta"), ("chrome", "gaosoi"), ("python", "dekaita"), ("server", "niakusesudekinai"), ("stream", "nohaishin"),
        ("update", "woshitekudasai"), ("zoom", "dekaiginisankashita"), ("music", "wokiiteiru"), ("file", "wohozonshita"), ("password", "wowasureta"),
        ("Discord", "dehanashita"), ("game", "gatanoshii"), ("app", "wokaihatsushiteimasu"), ("CPU", "noshiyouritsugatakai"), ("Twitter", "nikakikonda")];
    for (en, ja) in cases {
        let alone = conv(&mut s, ja).text();
        let mut row = vec![format!("{en}|{ja}: alone={alone}")];
        for (phk, phr) in [("kore", "これ"), ("sore", "それ"), ("nanika", "なにか"), ("mono", "もの")] {
            let c = conv(&mut s, &format!("{phk}{ja}"));
            row.push(format!("{phr}={}", strip(&c, phr).unwrap_or_else(|| format!("(fail:{})", c.text()))));
        }
        println!("{}", row.join("  "));
    }
    s.revert().unwrap();
}
