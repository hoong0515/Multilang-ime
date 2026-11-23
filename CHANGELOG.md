# Changelog

## v0.1.0 (2026-11-24)

첫 공개 판. First public release.

### 기능 / Features

- **자동 언어 판정**: 한국어(두벌식)·영어·일본어(로마자)를 전환 키 없이 섞어 친다. 키 입력 4-gram 모델과 단어 빈도 사전으로 단어·구간마다 언어를 정하고, 고른 결과를 학습한다.
  Korean (2-beolsik), English and Japanese romaji without a switch key; decided word by word and learned from your choices.
- **일본어 변환**: 직접 빌드한 Mozc 엔진. 히라가나·가타카나·반각 후보, 여러 쪽 후보창, 발음대로 친 장음의 ー 후보 (`koohii` → コーヒー), 글자 뒤 `-`는 장음 ー.
- **한자**: 한국어 발음 → 한자 후보 (libhangul 사전), 선택 학습.
- **후보창**: 왼쪽 Shift 탭으로 열기, 언어별 묶음(Tab), 숫자·방향키·PgUp/PgDn, 부분 변환(←/→).
- **사용자 사전**: 치는 키 → 변환 (예: `yrsk` → よろしくお願いします).
- **한/영 키**: 언어 고정 순환 (자동 → 한국어 → English(IME 끔) → 日本語), 커서 아래 알림, 작업 표시줄 입력 표시기.
- **앱 호환**: 64·32비트 앱, 샌드박스 앱(작업 표시줄 검색창), 조합을 받지 못하는 옛 입력칸·카카오톡·Synthesizer V(JUCE)는 커서 자리의 조합 창.
- **자동 꺼짐**: 비밀번호·숫자·이메일·URL 입력칸(앱이 입력칸 용도를 알려 줄 때), 지정한 프로그램.
- **설치·제거**: `install.cmd -Language ko|ja|en`, 설치한 파일만 지우는 제거, 앱 및 기능 등록.
- **개인정보**: 입력 내용·학습 기록·사용자 사전은 PC 밖으로 나가지 않는다. 네트워크 기능 없음.

### 알려진 제한 / Known limitations

- 이 판은 서명되지 않았다. Windows SmartScreen 경고가 뜰 수 있고, 작업 표시줄 검색창의 후보창은 가려진다(입력은 됨). 서명은 SignPath Foundation에 신청 예정 ([Code signing policy](docs/code-signing-policy.md)).
  This release is unsigned: SmartScreen may warn, and the candidate window in the taskbar search box stays hidden (typing works).
- 앱이 입력칸 용도를 알려 주지 않는 비밀번호 칸에서는 자동으로 꺼지지 않을 수 있다 (한/영 키로 English 고정).
- `to-do`, `so-so`처럼 양쪽이 모두 로마자로 읽히는 영어 하이픈 단어는 일본어가 1순위가 될 수 있다 (영어는 후보에 있음).
- 한/영 키가 없는 키보드의 언어 고정 키, 설정 화면에서 등록 언어 바꾸기는 아직 없다.

### 보안 / Security

- 두 차례의 정적 보안 검토와 대응: [2026-11-23](docs/security-review-2026-11-23.md), [2026-11-24](docs/security-review-2026-11-24.md) (파이프 요청·연결 상한, 원격 거부, 샌드박스 앱의 관리 명령·학습 차단, 후보창 메시지 검증, 비밀번호·PIN 입력 범위).

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/) (once approved; see [docs/code-signing-policy.md](docs/code-signing-policy.md)).
