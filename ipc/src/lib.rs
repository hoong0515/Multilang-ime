//! TIP(앱 안의 입력 모듈) ↔ 백엔드 사이의 메시지.
//!
//! 이름 있는 파이프(메시지 모드)로 요청 하나에 응답 하나. 본문은 JSON.
//! TIP 인스턴스마다 연결 하나를 유지하고, 백엔드는 연결마다 조합 상태를 따로 가진다.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
pub mod win;

/// 사용자별 파이프 이름 (`user`는 사용자 SID, `win::user_pipe_name`). 같은 사용자의 TIP와 백엔드만 만난다.
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

/// 미국 배열 기준 숫자·기호 키 → 글자 (글자 키 제외).
pub fn symbol_char(k: &Key) -> Option<char> {
    if k.ctrl || k.alt {
        return None;
    }
    let (lo, hi) = match k.vk {
        0x30..=0x39 => {
            let d = (k.vk - 0x30) as usize;
            ((b'0' + d as u8) as char, b")!@#$%^&*("[d] as char)
        }
        0xBA => (';', ':'),
        0xBB => ('=', '+'),
        0xBC => (',', '<'),
        0xBD => ('-', '_'),
        0xBE => ('.', '>'),
        0xBF => ('/', '?'),
        0xC0 => ('`', '~'),
        0xDB => ('[', '{'),
        0xDC => ('\\', '|'),
        0xDD => (']', '}'),
        0xDE => ('\'', '"'),
        _ => return None,
    };
    Some(if k.shift { hi } else { lo })
}

/// 글자 키(Shift·CapsLock 반영) 또는 숫자·기호 키 → 글자.
pub fn key_char(k: &Key) -> Option<char> {
    if (0x41..=0x5A).contains(&k.vk) && !k.ctrl && !k.alt {
        let c = k.vk as u8 as char;
        return Some(if k.shift ^ k.caps { c } else { c.to_ascii_lowercase() });
    }
    symbol_char(k)
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
    /// 왼쪽 Shift를 짧게 눌렀다 뗌: 변환(다음 후보). 조합 중일 때만 보낸다.
    ShiftTap,
    /// 연결 직후 TIP가 알리는 프로그램 실행 파일 이름 (앱별 자동 꺼짐).
    Hello { app: String },
    /// 설정 화면: 학습 기록(판정·한자·Mozc)을 모두 지운다. 관리 명령: 샌드박스(AppContainer)나
    /// 낮은 무결성 프로세스가 보내면 거부한다 (`error`가 채워진 응답).
    ClearLearning,
    /// 상태만 묻는다 (입력 표시기 갱신용, 포커스를 다시 얻었을 때).
    Status,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PreeditSegment {
    pub text: String,
    /// "ko" | "en" | "ja" | "other" | "raw"(Esc로 친 키 그대로 보기)
    pub lang: String,
    pub ambiguous: bool,
    /// 후보를 고르는 중인 구간 (굵은 밑줄).
    #[serde(default)]
    pub focused: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CandidateList {
    /// 표시할 후보 (변환 결과 텍스트).
    pub items: Vec<String>,
    /// 각 후보의 언어 구성 설명 (예: "ko", "en+ko").
    pub labels: Vec<String>,
    pub selected: usize,
    /// 여러 쪽일 때 지금 쪽(1부터)과 전체 쪽 수. items는 지금 쪽의 후보만 담는다. 한 쪽이면 0.
    #[serde(default)]
    pub page: usize,
    #[serde(default)]
    pub pages: usize,
}

/// 작업 표시줄 입력 표시기: 지금 언어("ko" | "en" | "ja")와 고정 여부.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Indicator {
    pub lang: String,
    pub locked: bool,
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
    /// 확정 텍스트의 언어별 구간 (TIP가 언어 속성을 붙여 앱이 맞는 글꼴을 쓰게 한다).
    #[serde(default)]
    pub commit_segments: Vec<PreeditSegment>,
    /// 후보창 (열려 있을 때만).
    #[serde(default)]
    pub candidates: Option<CandidateList>,
    /// 입력 표시기 (백엔드가 응답마다 채운다).
    #[serde(default)]
    pub indicator: Option<Indicator>,
    /// Status: 입력칸에 들어갈 때 고정 상태 알림을 보일지 (설정 `focus_notice`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub focus_notice: bool,
    /// 요청을 거부했을 때의 이유 (예: 샌드박스 앱이 보낸 관리 명령). 보통은 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn preedit_text(&self) -> String {
        self.preedit.iter().map(|s| s.text.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_field_is_optional() {
        // 보통 응답에는 error가 없고, 옛 응답(필드 없음)도 읽힌다
        let json = serde_json::to_string(&Response::default()).unwrap();
        assert!(!json.contains("error"));
        let r: Response = serde_json::from_str(r#"{"eaten":false,"flush_first":false,"preedit":[],"commit":null}"#).unwrap();
        assert_eq!(r.error, None);
        let denied = Response { error: Some("denied".into()), ..Default::default() };
        let back: Response = serde_json::from_str(&serde_json::to_string(&denied).unwrap()).unwrap();
        assert_eq!(back.error.as_deref(), Some("denied"));
    }
}
