//! 앞 단어 읽기를 붙여 한 번에 변환하고, 지금 단어 부분만 떼어 쓰는 방식 확인.
use std::time::Instant;
fn main() {
    let exe = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let mut s = mozc::Session::new(mozc::Client::connect_or_launch(&exe).unwrap()).unwrap();
    if std::env::args().any(|a| a == "--clear") {
        s.clear_learning().unwrap();
        println!("cleared user learning");
    }
    let cases = [("watashiha", "kinou"), ("amega", "furusoudesu"), ("", "kinou"), ("tegamiwo", "kaita"), ("ewo", "kaita"),
                 ("hashiwo", "watatta"), ("ohashide", "taberu"), ("kaigiwo", "kaishisuru"), ("kisyaga", "kisyade"), ("kisyaga", "kisyadekisyasita")];
    for (prev, cur) in cases {
        s.revert().unwrap();
        s.type_keys(&format!("{prev}{cur}")).unwrap();
        let t = Instant::now();
        let c = s.convert_current().unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        s.cancel_conversion().unwrap();
        println!("{prev}|{cur} -> {:?} {:?} ({ms:.1} ms)", c.segments, c.readings);
    }
    s.revert().unwrap();
}
