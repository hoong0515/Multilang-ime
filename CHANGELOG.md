# Changelog

## 다음 판 (미출시) / Unreleased

- 한/영 고정: 1.5초 안에 연달아 누르면 순환, 고정해 쓰다가 누르면 먼저 자동으로 (`00319e0`)
- 한/영 키가 없는 키보드: 오른쪽 Alt 단독 탭, 일본어 배열은 半角/全角 키로 언어 고정 (`d677a1e`, 실측 전)
- 앱 호환: Cubase 플러그인 검색창에서 두 번째부터 조합·후보창이 안 뜨던 문제 (`3ca30ca`),
  원신 등 커서 위치를 알려 주지 않는 앱은 창 안 클릭 자리나 창 아래쪽에 (`102b7cc`),
  한워드에서 조합 창이 커서보다 위 줄에 붙던 문제 일부 (`1253317`)
- 설치·제거: 제거 후 재부팅 전에 다시 설치하면 재부팅 때 새 DLL이 지워지던 문제 (`f90530f`),
  제거 뒤 TSF 등록 키가 남던 문제, 재부팅 전 재설치 거부, 재부팅 삭제 중복 예약 (`ea7a1d1`)
- 설치 프로그램 .exe (Inno Setup, 입력 언어 선택) (`548f785`)
- 후보창 모양: Windows 11 모양(둥근 모서리, 선택 줄 강조 막대, 흐린 번호·언어 칩), 다크 모드·고대비 테마,
  가나는 Yu Gothic UI로, 깜빡임 방지(이중 버퍼), 한/영 알림에 언어 배지

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
- `to-do`, `so-so`처럼 양쪽이 모두 로마자로 읽히는 영어 하이픈 단어는 처음에 일본어가 1순위가 될 수 있다. 후보에서 영어를 한 번 고르면 학습되어 다음부터 영어로 나온다.
- 한/영 키가 없는 키보드의 언어 고정 키, 설정 화면에서 등록 언어 바꾸기는 아직 없다.

### 보안 / Security

- 두 차례의 정적 보안 검토와 대응: [2026-11-23](docs/security-review-2026-11-23.md), [2026-11-24](docs/security-review-2026-11-24.md) (파이프 요청·연결 상한, 원격 거부, 샌드박스 앱의 관리 명령·학습 차단, 후보창 메시지 검증, 비밀번호·PIN 입력 범위).

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/) (once approved; see [docs/code-signing-policy.md](docs/code-signing-policy.md)).
