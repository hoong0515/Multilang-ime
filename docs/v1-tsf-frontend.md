# v1 TSF 프런트엔드 설계 (초안)

작성 2026-11-23. 대상: v1 한·영·일 IME (Windows 10/11, x64 우선).
참고 구현: Mozc Windows TIP (`third_party/mozc/src/win32/tip`, BSD), Microsoft Windows-classic-samples의 SampleIME.

## 1. 목표와 범위

- 전환 키 없이 한국어·영어·일본어를 섞어 입력한다. 언어는 v0 판정 엔진이 고른다.
- 조합 중 텍스트는 항상 그 시점의 최선 결과로 보여준다 (라이브 표시).
- 왼쪽 Shift 탭 = 변환(다음 후보), Space·Enter = 확정. Space는 변환에 쓰지 않는다 (기획서 "키 설계안").
- 일본어 한자 변환은 Mozc 서버에 맡긴다 (기획서 "Mozc 연동 방식", 개념 검증 완료).
- v1에서 하지 않는 것: 중국어, 재변환(확정된 글 다시 변환), 터치 키보드 최적화, macOS·Linux.

## 2. 프로세스 구성

```
 앱 프로세스 (메모장, 브라우저, 게임 ...)            사용자 세션에 하나
 ┌────────────────────────────┐   이름 있는 파이프   ┌──────────────────────────────┐
 │ TIP DLL (TSF 텍스트 서비스)  │ ─────────────────▶ │ 백엔드 프로세스                │
 │ - 키 이벤트 받기             │ ◀───────────────── │ - 언어 판정 (v0 엔진 이식)      │
 │ - 조합·확정 텍스트 쓰기       │   요청 1개 = 왕복 1회 │ - 두벌식 오토마타, 로마자 변환   │
 │ - 표시 속성(밑줄)            │                     │ - 선택 학습·사용자 사전         │
 │ - UI-less 후보 정보 제공      │                     │ - 후보창 그리기 (자체 창)       │
 └────────────────────────────┘                     │ - Mozc 서버 관리·통신           │
                                                    └──────────────┬───────────────┘
                                                                   │ Mozc IPC (파이프)
                                                    ┌──────────────▼───────────────┐
                                                    │ mozc_server (자체 빌드, 별도   │
                                                    │ 파이프 이름·프로필 폴더)        │
                                                    └──────────────────────────────┘
```

- **TIP DLL은 얇게 둔다.** 모든 앱 프로세스에 로드되므로, 사전·모델·학습 데이터는 백엔드에만 둔다. DLL이 하는 일은 키 전달, 문서 편집, 표시 속성, 위치 보고뿐이다. 백엔드가 죽어도 앱은 죽지 않고, TIP는 키를 먹지 않는 상태(직접 입력)로 물러난다.
- **후보창은 백엔드가 그린다.** Mozc도 후보창을 별도 렌더러 프로세스가 그린다 (`win32/tip/tip_ui_handler_conventional.cc`의 `Win32RendererClient`). 우리는 프로세스 수를 줄이려고 백엔드가 직접 그리는 쪽으로 시작한다. TIP는 조합 위치(`ITfContextView::GetTextExt`)만 보낸다.
- **mozc_server는 백엔드의 자식 프로세스다.** 개념 검증과 같이 일시 정지 + 가장 토큰 방식으로 띄운다. 사용자가 설치한 Mozc·Google 日本語入力과 섞이지 않도록 파이프 접두사와 프로필 폴더를 바꾼 자체 빌드를 쓴다.

## 3. TIP DLL이 구현할 TSF 인터페이스

Mozc의 `TipTextServiceImpl`(`win32/tip/tip_text_service.cc`)을 기준으로, v1에 필요한 것만 남긴다.

| 인터페이스 | 용도 | v1 |
| --- | --- | --- |
| ITfTextInputProcessorEx | 활성화·비활성화 | 필수 |
| ITfKeyEventSink | OnTestKeyDown/Up, OnKeyDown/Up, OnPreservedKey | 필수 |
| ITfThreadMgrEventSink | 포커스 문서 변경 감지 | 필수 |
| ITfCompositionSink | 앱이 조합을 강제로 끝낼 때 정리 | 필수 |
| ITfDisplayAttributeProvider | 구간별 밑줄 모양 | 필수 |
| ITfTextLayoutSink | 조합 위치가 바뀔 때 후보창 이동 | 필수 |
| ITfCompartmentEventSink | 키보드 열림·비활성 상태 감지 | 필수 |
| ITfUIElementMgr 연동 (ITfCandidateListUIElement) | 게임 등 UI-less 앱에 후보 정보 제공 | 필수 |
| ITfThreadFocusSink | 포커스 잃을 때 후보창 숨김 | 필수 |
| ITfTextEditSink | 앱이 조합 영역을 바꿨는지 감지 | 권장 |
| ITfFnReconversion, ITfFnConfigure | 재변환, 설정 열기 | v2 이후 |

등록 카테고리 (Mozc `win32/base/tsf_registrar.cc`와 같게): `GUID_TFCAT_TIP_KEYBOARD`, `GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER`, `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`(UI-less), `GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT`(스토어 앱), `GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT`, `GUID_TFCAT_TIPCAP_COMLESS`, `GUID_TFCAT_TIPCAP_INPUTMODECOMPARTMENT`.

## 4. 키 입력 흐름

TSF의 키 이벤트 처리는 동기식이다 (`tip_keyevent_handler.cc`의 주석). Mozc도 `OnTestKeyDown` 안에서 서버에 동기 호출로 "이 키를 먹을지"를 묻는다. 우리도 같은 구조로 간다.

1. `OnTestKeyDown`: 키 정보를 백엔드에 보내 `consumed` 여부만 받는다. 백엔드 응답 예산은 **5 ms 이내** (판정 0.2~1 ms, Mozc 키 1 ms 측정치 기준).
2. `OnKeyDown`: 같은 키를 실제로 처리하고 출력(조합·확정·후보)을 받아, 동기 편집 세션(`TF_ES_SYNC`)으로 문서에 반영한다.
3. 시간 초과·백엔드 없음: 키를 먹지 않는다. 사용자는 IME 없이 입력하는 상태가 되고, 백엔드는 다시 띄운다.

### 키를 어떻게 보낼까

- 판정 모델은 **US QWERTY의 물리 키 위치** 기준이다. TIP는 활성 키보드 배열과 상관없이 가상 키·스캔 코드를 보내고, 백엔드가 QWERTY 글자로 바꾼다.
- 대소문자: Shift·CapsLock 상태를 함께 보낸다. 친 그대로의 대소문자는 백엔드가 기억한다 (v0과 같음).

### 키 표

| 키 | 조합이 없을 때 | 조합 중 |
| --- | --- | --- |
| 글자 키 | 조합 시작 | 조합에 추가, 판정·표시 갱신 |
| 왼쪽 Shift 탭 | 먹지 않음 | 다음 후보 (후보창 열기) |
| 숫자 1~5 | 먹지 않음 | 후보창이 열려 있으면 해당 후보 선택 |
| Space | 먹지 않음 | 확정 + 띄어쓰기 (일본어·중국어 구간은 공백 없이 확정) |
| Enter | 먹지 않음 | 확정 (공백 없음) |
| Esc | 먹지 않음 | 조합 전체를 친 키 그대로 표시 (자동 변환 되돌리기) |
| Backspace | **확정 직후면** 방금 확정한 구간을 친 키 그대로 다시 열고 후보 표시. 그 외엔 먹지 않음 | 후보창이 열려 있으면 닫기, 아니면 마지막 키 하나 지우기 |
| 한/영 (VK_HANGUL) | 기본 언어 순환 | 현재 구간의 언어 바꾸기 |
| 한자 (VK_HANJA) | 먹지 않음 | 한국어 구간의 한자 후보 |
| Shift + ←/→ | 먹지 않음 | 구간 경계 조정 |
| 구두점 | 먹지 않음 | 확정 후 구두점 입력 |

- 한/영·한자 키는 `ITfKeystrokeMgr::PreserveKey`로 등록해 `OnPreservedKey`에서 받는다.
- **Shift 탭 판정**: 왼쪽 Shift의 KeyDown에서 시각을 기록하고, KeyUp까지 다른 키가 없고 200 ms 이내면 탭이다. Shift를 누른 채 글자를 치면 탭이 아니다. 탭 판정은 `OnTestKeyUp`에서 하고, 조합이 없으면 먹지 않는다.

## 5. 조합 상태와 표시

백엔드가 돌려주는 출력:

```
Output {
  consumed: bool
  preedit: [Segment { text, lang(ko|en|ja), ambiguous: bool, focused: bool }]
  cursor: int
  commit: string            // 이번에 확정된 텍스트 (있으면)
  candidates: { items: [string], focused: int, page } | 없음
  candidate_anchor: segment index   // 후보창을 붙일 구간
}
```

표시 속성 (`ITfDisplayAttributeProvider`):

| 상태 | 밑줄 | 근거 |
| --- | --- | --- |
| 일반 조합 | 가는 실선 | Mozc의 입력 속성과 같음 |
| 모호한 구간 | 점선 | 기획서 "깜빡임 대책"의 모호함 표시 |
| 후보를 고르는 중인 구간 | 굵은 실선 | Mozc의 변환 구간 속성과 같음 |

깜빡임 대책(표시 안정화, 앞부분 고정, 짧은 입력 유예, 확정 시점 재판정)은 **백엔드에서** 처리한다. TIP는 받은 preedit을 그대로 그린다.

## 6. 확정 규칙

- 한국어·영어: Space, Enter, 구두점, 포커스 이동에서 확정. 확정할 때 문장 전체 문맥으로 마지막 판정을 한 번 더 한다.
- 일본어: 구간이 일본어로 판정되면, 백엔드는 그 구간의 키를 Mozc 세션에 키 단위(`SEND_KEY`, 약 1 ms)로 넘기고, 예측 후보 1위를 라이브 표시에 쓴다. 판정이 일본어로 새로 바뀌면 그 구간의 가나를 `UPDATE_COMPOSITION`(약 15 ms)으로 한 번에 넣는다. 확정은 Mozc `SUBMIT`이며, 이때 Mozc의 변환 학습이 일어난다.
- **확정 직후 Backspace = 다시 열기** (휴대폰 자동 수정 취소와 같은 방식):
    - 조건: 확정한 뒤 다른 키를 누르지 않았고, 커서가 확정 텍스트 바로 뒤에 있음. 백엔드는 마지막 확정 구간의 키 입력·위치·길이를 기억한다.
    - 동작: TIP가 문서에서 확정 텍스트(+ 자동으로 넣은 공백)를 지우고, 그 자리에 친 키 그대로를 조합으로 다시 연다. 후보창에 한국어·영어·일본어(가나·한자) 후보를 띄운다.
    - 다시 Backspace를 누르면 평소처럼 지운다 (다시 연 조합의 마지막 키부터).
    - 커서가 옮겨졌거나 앱이 텍스트를 바꿨으면(확정 텍스트를 다시 읽어 비교) 다시 열지 않고 평소 Backspace로 넘긴다.
- **조합 중 Backspace**: 후보창이 열려 있으면 닫기만 하고, 아니면 마지막 키 하나를 지운다. 조합 중 오타 수정에 키가 더 들지 않게 하려는 것이다. 조합 전체를 친 키 그대로 보고 싶으면 Esc.
- **되돌린 기록의 학습**: 다시 연 구간에서 다른 후보를 고르면, 새 선택은 올리고 처음 결과는 내린다 (부정 신호). 일본어로 확정됐던 구간이면 Mozc 쪽 학습도 되돌려야 하는데, 방법은 미정 (아래 "열린 문제").
- 확정 직후 한/영: 다시 열지 않고 바로 다른 언어로 바꿔 넣는다 (Backspace 다시 열기 + 언어 지정을 한 번에).
- 앱이 조합을 강제로 끝내면(`OnCompositionTerminated`) 현재 표시대로 확정한 것으로 보고 학습은 하지 않는다.

## 7. 자동 꺼짐

- InputScope가 비밀번호·숫자·전화번호·이메일 주소·URL이면 IME가 키를 먹지 않는다. Mozc도 InputScope로 상태를 바꾼다 (`tip_input_mode_manager.h`).
- 비활성 문맥과 빈 문맥 (Mozc의 `TipStatus::IsDisabledContext`, `IsEmptyContext`에 해당)에서는 동작하지 않는다.
- 앱별 규칙 (터미널, 코드 에디터, 게임): 설정에서 프로세스 이름으로 지정. 기본 목록은 v1 테스트하면서 정한다.
- 자동 꺼짐 구간의 입력은 학습·사전에 남기지 않는다.

## 8. TIP ↔ 백엔드 통신

- 이름 있는 파이프, 메시지 모드. 요청 하나에 응답 하나. 메시지는 protobuf (Mozc와 같은 방식).
- Mozc와 달리 요청마다 연결을 새로 열지 않고, **TIP 인스턴스(스레드)당 연결 하나를 유지**한다. 키마다 연결을 여는 비용과, Mozc 개념 검증에서 본 "파이프 바쁨" 재시도를 피하려는 것이다.
- 보안: 같은 사용자만 접속할 수 있게 파이프 DACL을 건다. 스토어 앱(AppContainer)에서도 TIP가 접속해야 하므로, AppContainer SID 접근을 허용한다. Mozc도 AppContainer 때문에 이벤트·뮤텍스 이름에 `Local\` 접두사를 쓴다 (`base/const.h` 주석).
- 백엔드는 사용자 로그온 때 시작(작업 스케줄러 또는 TIP 첫 활성화 때 실행). 하나만 뜨도록 뮤텍스로 막는다.

## 9. 설치와 등록

- 하나의 TIP CLSID를 **한국어(0x0412)와 일본어(0x0411) 두 언어 프로필**로 등록하는 안을 기본으로 검토한다. 사용자는 Windows 언어 설정의 한국어 또는 일본어 아래에서 이 IME를 고른다. 한 프로필만 쓰면 다른 언어 앱에서 IME 목록에 안 보이는 문제가 있어서다.
- TIP DLL은 32비트·64비트 둘 다 필요하다 (32비트 앱용). Mozc도 `mozc_tip32.dll`, `mozc_tip64.dll`을 따로 둔다. ARM64는 v1 이후.
- 스토어 앱을 위해 설치 폴더와 DLL에 `ALL APPLICATION PACKAGES` 읽기·실행 권한을 준다.
- 코드 서명: 배포 전 필수 (백신 오탐 방지).

## 10. 구현 언어

| 부분 | 후보 | 메모 |
| --- | --- | --- |
| TIP DLL | C++ 또는 Rust (windows 크레이트) | COM 구현이 많다. Mozc·SampleIME를 참고하기 쉬운 건 C++. Rust는 메모리 안전성 이점 |
| 백엔드 | Rust 또는 C++ | v0 Python 엔진을 이식. 판정 성능은 Python으로도 충분했지만 배포·상주를 생각하면 네이티브 |
| 후보창 | 백엔드 안의 Win32 창 | DirectWrite로 그리기 |

결정은 미정. 추천은 TIP·백엔드 모두 Rust이고, 막히면 TIP만 C++로 바꾼다.

## 11. 마일스톤

1. **M1 뼈대 TIP**: 등록, 키를 그대로 조합에 넣고 Space로 확정. 메모장·브라우저·Word·스토어 앱에서 동작 확인.
2. **M2 백엔드 + 통신 + 한·영 판정**: v0 판정 엔진 이식, 두벌식 조합, 구간별 표시 속성.
3. **M3 후보창과 학습**: 왼쪽 Shift 탭, 숫자 선택, 되돌리기(Esc, 확정 직후 Backspace 다시 열기), 선택 학습 저장.
4. **M4 일본어**: Mozc 자체 빌드 번들, 키 단위 전달, 라이브 표시, 확정 시 Mozc 학습.
5. **M5 안정화**: 깜빡임 대책, 자동 꺼짐, UI-less(게임), 설정 화면 최소판.
6. **M6 실사용**: 1~2주 직접 사용, 100단어당 수정 횟수 기록 (v1 완료 기준).

## 12. 열린 문제

- 판정이 틀려 일본어로 잘못 확정된 구간의 Mozc 학습을 되돌리는 방법 (Mozc에 확정 취소 명령이 있는지 확인 필요). 확정 직후 Backspace로 다시 여는 경우에 특히 필요하다.
- 확정 직후 Backspace로 다시 열 때, 앱이 확정 텍스트를 이미 바꿨는지 확인하는 방법 (주변 텍스트 읽기가 안 되는 앱에서의 기본 동작).
- 라이브 표시에서 Mozc 예측 후보 1위를 쓸지, 키마다 변환을 다시 부를지.
- Shift 탭이 게임·다른 IME 습관과 충돌하는 경우의 기본값.
- 언어 프로필을 두 개로 등록했을 때 Windows의 언어 전환 동작 (입력 언어가 바뀌면 앱이 맞춤법 검사 언어를 바꾸는 등).
- 백엔드가 응답하지 않을 때의 사용자 알림 방식.
