# Multilang IME (한·영·일 자동 전환 입력기)

전환 키 없이 한국어·영어·일본어를 섞어 칠 수 있는 Windows 입력기입니다. 일본어 변환은 Mozc를 씁니다.
GPL-3.0-or-later 자유 소프트웨어입니다 (`LICENSE.txt`, 제3자 구성 요소는 `THIRD-PARTY-NOTICES.md`).

## 설치

설치 프로그램 `MultilangIME-<버전>-setup.exe`를 실행하면 입력 언어(한국어·日本語·English)를 고른 뒤 아래 과정을 대신 해 줍니다. 조용히 설치: `MultilangIME-<버전>-setup.exe /VERYSILENT /INPUTLANG=ko`.

zip으로 설치할 때:

1. 압축을 풉니다.
2. `install.cmd`를 실행하고 관리자 권한 요청(UAC)을 허용합니다.
   - `C:\Program Files\MultilangIME`에 복사하고, 64비트·32비트 입력기를 등록합니다.
   - 로그온 때 백엔드를 띄우는 작업 스케줄러 작업("MultilangIME Backend")을 만듭니다.
   - 지금 사용자의 한국어 입력 방법 목록에 "Multilang IME"를 추가합니다.
3. 이미 열려 있던 앱은 다시 열어야 입력기가 적용됩니다.

어느 언어 밑에 보일지 고를 수 있습니다 (기본 한국어). 어느 쪽이든 한·영·일을 모두 입력합니다. 작업 표시줄의 언어 표시와, 앱이 글꼴을 고르는 기준이 이 언어를 따릅니다:

```
install.cmd -Language ko    (한국어, 기본)
install.cmd -Language ja    (日本語)
install.cmd -Language en    (English)
```

입력 방법은 작업 표시줄의 언어 표시(또는 Win+Space)에서 고릅니다. 작업 표시줄 아이콘은 지금 언어(가 / A / あ)를 보이고, 점 세 개는 자동 전환, 자물쇠는 고정입니다. 아이콘을 누르면 설정 화면이 열립니다.

## 기본 키

| 키 | 동작 |
| --- | --- |
| 그냥 치기 | 언어를 자동으로 판정해 조합 (일본어는 로마자 입력 → 변환 결과 표시) |
| Space / Enter | 확정 |
| 왼쪽 Shift 탭 | 변환 후보창 열기 / 다음 후보 |
| 숫자 1~9, ↑↓ | 후보 고르기 (숫자는 지금 쪽 안에서) |
| Page Down / Page Up | 후보창 다음 / 이전 쪽 |
| Tab / Shift+Tab | 후보 묶음 이동 (영어 / 한국어·한자 / 일본어 / 맨 뒤에 전각·기호). 영어 묶음에는 언제나 친 그대로(로마자)가 있다 |
| Ctrl+Enter (후보창에서) | 고른 후보를 사용자 사전 맨 앞에 넣고 확정 (다음부터 후보창 맨 앞에) |
| Ctrl+Delete (후보창에서) | 고른 후보를 이 키의 후보창에서 숨김 (사전 후보면 사전에서 뺌). 설정 > 숨긴 후보에서 되돌림 |
| ← / → | 긴 입력의 부분(문절) 이동 |
| Backspace | 후보창이 열려 있으면 친 키 그대로, 아니면 키 하나 지우기 |
| Esc | 한 번: 입력 전체를 친 그대로(로마자)로 보임, 두 번: 조합 취소 |
| 한/영 (한/영 키가 없으면 오른쪽 Alt를 혼자 짧게, 일본어 배열은 半角/全角) | 입력 언어 고정: 자동 → 한국어 → English → 日本語 → 자동 (모든 앱 공통, English 고정은 IME를 거치지 않음). 1.5초 안에 연달아 누르면 순서대로, 고정해 쓰다가 누르면 먼저 자동으로 |
| Shift+Space | 전각 공백 |

## 설정

시작 메뉴의 **Multilang IME Settings**에서 바꿉니다. 저장하면 바로 적용됩니다 (앱을 다시 열 필요 없음).

- **사용할 언어**: 한국어·일본어를 자동 판정과 후보에서 뺄 수 있습니다 (영어는 항상 사용).
- **일본어 문맥에서 Space**: 확정 + 전각 공백(기본) / 확정하지 않고 이어서 변환.
- **자동 꺼짐 프로그램**: 실행 파일 이름(예: `game.exe`)을 넣으면 그 프로그램에서는 IME가 키를 받지 않습니다.
- **사용자 사전** 탭: 치는 키와 변환을 등록합니다 (예: `yrsk` → よろしくお願いします). 후보창 맨 앞 "사전" 묶음에 나오고 첫 Shift 탭이 고릅니다.
- **알림**: 언어를 고정해 쓸 때 입력칸에 들어가면 고정 상태를 커서 옆에 잠깐 보입니다 (기본 켬).
- **학습 기록 지우기**: 언어 판정·한자 선택·일본어 변환(Mozc) 학습을 모두 지웁니다.

설정 파일은 `%LOCALAPPDATA%\multilang-ime\settings.json`입니다:

```json
{ "ja_space": "commit", "languages": { "ko": true, "ja": true }, "disabled_apps": ["game.exe"], "focus_notice": true }
```

## 알아 둘 점

- 메모장 등에서 일본어 글자(気, ー 등)가 네모로 보이면 한·일 글자가 모두 있는 글꼴(Noto Sans KR, 본고딕 등)을 쓰세요.
- 비밀번호·숫자·이메일·URL 입력칸에서는 자동으로 꺼집니다. 단, 앱이 입력칸 용도를 알려 주지 않으면 꺼지지 않을 수 있습니다. 그런 비밀번호 칸에서는 한/영 키로 English 고정(IME 끔)을 써 주세요.
- 작업 표시줄 검색창의 후보창은 서명된 배포판에서만 보입니다 (후보창 대행 프로그램 `MultilangIME-CandWin.exe`가 UIAccess로 실행되어야 함). 서명 없는 판에서는 검색창에서 입력은 되지만 후보창이 가려집니다.

## 검색창 후보창: 이 PC 전용 서명 (선택, 보안 위험 있음)

서명 없는 판에서도 검색창에 후보창을 띄우고 싶다면, 이 PC에서만 믿는 인증서를 만들어 `MultilangIME-CandWin.exe`에 서명할 수 있습니다. **이 PC의 보안 설정(신뢰할 수 있는 루트 인증서)을 바꾸는 일**이라 설치 프로그램과 설정 화면에는 없고, 직접 관리자 PowerShell에서 실행하고 확인 문구를 쳐야 켜집니다.

위험:
- 이 인증서의 키로 서명된 파일은 이 PC에서 신뢰됩니다. 키는 이 PC에서 새로 만들고, 내보낼 수 없으며, 서명 직후 지웁니다. 다만 실행하는 몇 초 동안은 존재합니다.
- 서명된 후보창 프로그램은 UIAccess 권한으로 실행되어, 관리자 창을 포함한 다른 창 위에 그리고 입력을 보낼 수 있습니다. 이 프로그램에 버그가 있으면 이 PC의 다른 프로그램이 악용할 여지가 있습니다.
- 백신이나 회사 정책이 새 루트 인증서를 경고하거나 막을 수 있습니다. 회사·학교 PC에서는 쓰지 마세요.

제한: 인증서는 코드 서명 전용이고 인증 기관(CA)이 아니어서, 키가 있어도 웹사이트(HTTPS) 인증서를 만들 수 없습니다. 설치 폴더의 그 파일 하나만 서명합니다. 업데이트·제거 때 인증서를 지우므로, 업데이트 후에는 다시 켜야 합니다.

관리자 PowerShell에서:

```
powershell -ExecutionPolicy Bypass -File "C:\Program Files\MultilangIME\local-signing.ps1" -Status
powershell -ExecutionPolicy Bypass -File "C:\Program Files\MultilangIME\local-signing.ps1" -Enable
powershell -ExecutionPolicy Bypass -File "C:\Program Files\MultilangIME\local-signing.ps1" -Disable
```

`-Enable`은 안내를 보여 주고 `I accept the risk on <컴퓨터 이름>`을 그대로 쳐야 진행합니다. `-Disable`로 언제든 되돌립니다.

## 제거

설치 폴더(`C:\Program Files\MultilangIME`)의 `uninstall.cmd`를 실행합니다. 설정·학습 기록까지 지우려면:

```
uninstall.cmd -RemoveUserData
```

등록을 푼 뒤 쓰던 앱을 닫거나 재부팅하고 설치 폴더를 지웁니다.
