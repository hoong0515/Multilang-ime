use std::time::Instant;
fn main() {
    let exe = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let mut s = mozc::Session::new(mozc::Client::connect_or_launch(&exe).unwrap()).unwrap();
    for word in ["konpyu-ta", "kantan", "nihon", "kaiteimasu"] {
        s.revert().unwrap();
        let mut row = vec![];
        for c in word.chars() {
            let t = Instant::now();
            let comp = s.type_keys(&c.to_string()).unwrap();
            let t1 = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            let cv = s.convert_current().unwrap();
            let t2 = t.elapsed().as_secs_f64() * 1e3;
            s.cancel_conversion().unwrap();
            row.push(format!("{c}:{comp}/{}({t1:.1}+{t2:.1})", cv.text()));
        }
        println!("{}", row.join(" "));
    }
    s.revert().unwrap();
}
