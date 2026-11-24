# Multilang IME

전환 키 없이 한국어·영어·일본어를 섞어 치는 Windows 입력기. 치는 동안 언어를 자동으로 판정하고, 일본어는 Mozc로 변환한다.
A Windows IME that detects Korean, English and Japanese as you type, with no switch key. Free software (GPL-3.0-or-later).

- `dkssudgktpdy hello APIfmf` → `안녕하세요 hello API를`
- `watashihagakuseidesu` → `私は学生です`
- `Youtubenidougawoageta` → `Youtubeに動画を上げた` (한 단어 안에서도 구간마다 언어를 나눠 변환)

## 기능

- **자동 전환**: 키 입력 4-gram 언어 모델과 단어 빈도 사전으로 토큰·구간마다 언어를 정한다. 직전 단어의 언어, 사용자의 선택을 학습한다.
- **일본어 변환**: Mozc 엔진(직접 빌드한 도우미)으로 가나가 완성되는 대로 변환해 보인다. 키당 최대 약 3 ms.
- **후보창**: 언어별 묶음(영어 / 한국어·한자 / 일본어), 여러 쪽, 일본어 묶음 맨 앞에 히라가나·가타카나·반각 가타카나.
- **부분 변환**: 긴 입력을 언어 구간·문절로 나눠 ←/→로 옮겨 가며 고른다.
- **한자**: 한국어 발음 → 한자 후보 (libhangul 사전), 고른 것을 학습.
- **사용자 사전**: 치는 키 → 변환 (예: `yrsk` → よろしくお願いします).
- **자동 꺼짐**: 비밀번호·숫자·이메일·URL 입력칸, 지정한 프로그램. 입력칸 용도는 앱이 알려 주는 것(TSF 입력 범위)과 표준 Win32·WinForms 입력칸 스타일로 판단하므로, 용도를 알려 주지 않는 앱의 비밀번호 칸에서는 꺼지지 않을 수 있다 (그때는 한/영 키로 English 고정).
- **앱 호환**: 64·32비트 앱, 샌드박스 앱(작업 표시줄 검색창 등), 조합을 받지 못하는 옛 입력칸·카카오톡은 커서 자리에 겹치는 조합 창.

## 설치

릴리스의 `MultilangIME-<버전>-setup.exe`를 실행한다 (관리자 권한, 입력 언어 선택). 또는 zip을 풀고 `install.cmd`. 자세한 사용법·키·설정은 [installer/README-install.md](installer/README-install.md).

| 키 | 동작 |
| --- | --- |
| 그냥 치기 | 자동 판정으로 조합 |
| Space / Enter | 확정 |
| 왼쪽 Shift 탭 | 후보창 / 다음 후보 |
| 숫자 1~9, ↑↓, PgUp/PgDn | 후보 고르기, 쪽 넘기기 |
| Tab / Shift+Tab | 후보 언어 묶음 이동 |
| ← / → | 부분 이동 |
| 한/영 (없으면 오른쪽 Alt 탭, 일본어 배열은 半角/全角) | 언어 고정 순환 (자동 → 한국어 → English → 日本語). 연달아 누르면 순환, 고정해 쓰다가 누르면 먼저 자동 |

## 구조

| 폴더 | 내용 |
| --- | --- |
| `tip/` | TSF 텍스트 서비스 DLL (Rust, x64·x86). 앱 안에서 키를 받아 조합·후보창을 그린다 |
| `backend/` | 사용자별 백엔드 (이름 있는 파이프). 판정·후보·학습·Mozc 연결 |
| `engine/` | 언어 판정 엔진 (두벌식 오토마타, 로마자, n-gram, 격자·비터비) |
| `mozc/`, `mozc-helper/` | Mozc 클라이언트(protobuf), Mozc 엔진을 감싼 표준 입출력 도우미(C++, Bazel) |
| `candhost/` | 작업 표시줄 검색창용 후보창 프로세스 (UIAccess) |
| `settings/` | 설정 화면 (WinForms) |
| `installer/` | 설치·제거 스크립트, 릴리스 빌드 |
| `ime_v0/`, `eval/` | 모델 생성(Python)과 평가 |

## 빌드

```powershell
python -m pip install -r requirements-model.txt          # Python 3.10
python -m ime_v0.export_model                              # data/engine_model.json
powershell -File tools\fetch-third-party.ps1               # libhangul hanja.txt
cargo test -p engine -p ipc -p backend -p tip
cargo build --release -p tip -p backend -p candhost
cargo build --release -p tip --target i686-pc-windows-msvc
powershell -File settings\build.ps1
powershell -File mozc-helper\build.ps1                     # Mozc 체크아웃 필요: docs/mozc-helper.md
powershell -File installer\build-release.ps1 -NoBuild      # dist\MultilangIME-<버전>-win.zip
```

GitHub Actions(`.github/workflows/build.yml`)가 같은 순서로 빌드한다. 절차와 코드 서명은 [docs/release.md](docs/release.md).

## 문서

- [docs/v1-tsf-frontend.md](docs/v1-tsf-frontend.md): 설계
- [docs/next-tasks.md](docs/next-tasks.md): 할 일과 해결 기록
- [docs/development-log.md](docs/development-log.md): 측정 결과와 개발 경위
- [docs/mozc-helper.md](docs/mozc-helper.md): Mozc 도우미

## Code signing policy

현재 릴리스는 서명되지 않는다. 서명 대상, 팀 역할, 개인정보 정책은 [docs/code-signing-policy.md](docs/code-signing-policy.md).

개인정보: 이 프로그램은 사용자나 설치·운영하는 사람이 직접 요청하지 않는 한 어떤 정보도 다른 네트워크 시스템으로 보내지 않는다 (입력 내용·학습 기록·사용자 사전은 PC 밖으로 나가지 않는다).

## 라이선스

GPL-3.0-or-later ([LICENSE](LICENSE)). 제3자 구성 요소(Mozc, libhangul 한자 사전, wordfreq 데이터, Rust 크레이트)는 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
