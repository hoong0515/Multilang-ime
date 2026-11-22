//! 실행 중인 Mozc 서버의 사용자 학습(변환 이력·예측)을 지운다. 시험 확정으로 학습이 오염됐을 때.
//! 사용: mozc_clear [결과 파일]  — 서버를 띄우지 않고, 떠 있는 서버에만 붙는다.
fn main() {
    let out = std::env::args().nth(1);
    let msg = match mozc::Client::connect().and_then(mozc::Session::new).and_then(|mut s| s.clear_learning()) {
        Ok(()) => "cleared".to_string(),
        Err(e) => format!("failed: {e:?}"),
    };
    println!("{msg}");
    if let Some(p) = out {
        let _ = std::fs::write(p, msg);
    }
}
