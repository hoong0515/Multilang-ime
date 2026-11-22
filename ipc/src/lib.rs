//! TIP(앱 안의 입력 모듈) ↔ 백엔드 사이의 메시지.
//!
//! 이름 있는 파이프(메시지 모드)로 요청 하나에 응답 하나. 본문은 JSON.
//! TIP 인스턴스마다 연결 하나를 유지하고, 백엔드는 연결마다 조합 상태를 따로 가진다.

use serde::{Deserialize, Serialize};

/// 사용자별 파이프 이름. 같은 사용자 세션의 TIP와 백엔드만 만난다.
pub fn pipe_name(user: &str) -> String {
    let safe: String = user.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    format!(r"\\.\pipe\multilang-ime.{safe}")
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub vk: u16,
    pub shift: bool,
    pub caps: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// 키를 먹을지만 묻는다 (상태 변화 없음). OnTestKeyDown.
    Test { key: Key },
    /// 키를 처리한다. OnKeyDown.
    Key { key: Key },
    /// 현재 조합을 확정한다 (조합 중 다른 키가 왔을 때 등).
    Flush,
    /// 조합을 버린다 (앱이 조합을 강제로 끝냈을 때, 포커스 이동).
    Reset,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PreeditSegment {
    pub text: String,
    /// "ko" | "en" | "ja" | "other" | "raw"(Esc로 친 키 그대로 보기)
    pub lang: String,
    pub ambiguous: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Response {
    /// Test: IME가 이 키를 처리하는가.
    pub eaten: bool,
    /// Test: 키를 앱에 넘기기 전에 조합을 먼저 확정해야 하는가 (Flush를 보낼 것).
    pub flush_first: bool,
    /// 현재 조합 (비어 있으면 조합 끝).
    pub preedit: Vec<PreeditSegment>,
    /// 이번에 확정된 텍스트.
    pub commit: Option<String>,
}

impl Response {
    pub fn preedit_text(&self) -> String {
        self.preedit.iter().map(|s| s.text.as_str()).collect()
    }
}
