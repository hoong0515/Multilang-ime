# Claude Code 인계 (2026-11-24)

이전 세션(2026-11-22~24)에서 이어받는 새 Claude Code 세션용. 먼저 이 문서, 다음에 `docs/codex-release-plan.md`(남은 일·순서·기준), `docs/security-review-2026-11-23.md`(Codex 보안 검토)를 읽는다.
자세한 경위: `docs/session-summary-2026-11-23.md`(기획·엔진·구조), `docs/next-tasks.md`(할 일·해결 기록), `docs/release.md`(배포 절차), `docs/development-log.md`(측정).

## 1. 이 프로젝트

한·영·일 자동 전환 Windows IME (TSF, Rust). 공개·무료 배포 예정(GPL-3.0-or-later). 사용자 결정의 요지:
- 일본어 IME 방식(조합 → 1위 자동 → 후보 → 확정 시 학습)을 다국어로. Space = 확정만, 왼쪽 Shift 탭 = 변환/다음 후보, 숫자 1~9·PgUp/PgDn, Tab = 언어 묶음.
- 한/영 키 = 언어 고정 순환(자동 → 한국어 → English → 日本語), 모든 앱 공통, 커서 아래 1.2초 알림, 작업 표시줄 아이콘(지금 언어 + 점 세 개=자동 / 자물쇠=고정).
- 등록 언어는 설치 때 고른다(`install.cmd -Language ko|ja|en`, 기본 ko).

## 2. 이전 세션에서 끝낸 것 (최근 순)

| 커밋 | 내용 |
| --- | --- |
| (이 문서 커밋) | 정션 시험 PS 5.1 멈춤 수정, 계획에 보안 항목 R1~R5 추가, 이 인계 문서 |
| 7ba925a | Codex 리뷰 반영(사용자가 커밋): 제거는 등록된 설치 폴더에서만, 정션·Windows 하위 금지, 다른 관리자 계정이면 사용자별 단계 생략, `-Version` 검증 + Codex 보안 검토 문서 |
| 31caa63 | 설치·제거 안전: 전용 폴더만, `install-manifest.txt` 기준 삭제, 쓰는 중 파일은 재부팅 때 삭제(MoveFileEx, IntPtr NULL), `tests/installer-files.ps1` |
| 6f3fdaa | 한/영 키 알림(커서 아래) |
| 3613fbd | 한·일·영 프로필 등록, `-Language` |
| 0f31381 | 언어 고정(한/영) + 언어별 입력 표시기 아이콘 |
| a865871 | 입력 표시기 단추(GUID_LBI_INPUTMODE) |
| 7c931a9 | 버전 정보·아이콘, 앱 및 기능 등록, 공개용 README |
| f26a3cc | 조합 창(옛 입력칸·카카오톡): 조합 글자를 커서 자리에 겹쳐 표시 |
| a0226df | 카카오톡: 조합을 키마다 끝내는 입력칸은 조합 창(지정 + 자동 학습) |
| e55537f | 검색창 후보창: UIAccess 후보창 프로세스(`candhost/`) |
| 5e58349 | GitHub Actions CI(core/mozc/package), git 밖 입력 재생성 |
| f6c9c54, 8f2772a | 일본어 가나 후보 맨 앞, 후보창 여러 쪽(Mozc 전체 후보) |
| 0d8ad4b | Mozc 직접 빌드 도우미 `mozc_stdio.exe` |
| a6f3736, c724160 | 사용자 사전, 설정 화면 |
| 1be1eef | LICENSE·고지·설치 스크립트·릴리스 zip |

사용자 확인(실사용): 메모장 후보창·일본어·사용자 사전·가나 후보, 카카오톡 입력과 겹침 조합 창, 검색창 후보창(개발용 서명), 설치판 동작, 한/영 고정 4종.

## 3. 다음에 할 일 (권장 순서)

1. **보안 R1~R5** (`docs/codex-release-plan.md` 2-0, 근거 `docs/security-review-2026-11-23.md`). 코드에서 R1·R2·R3·R5를 확인함: `backend/src/main.rs:209` `PIPE_UNLIMITED_INSTANCES`(원격 거부 플래그 없음), `read_message`가 무제한 누적, `main.rs:127` `ClearLearning` 무조건 실행, `tip/src/lib.rs:537` 첫 입력 용도만 검사. R2의 실패 시 정책(fail-closed 여부)은 사용자 결정.
2. **설치·업데이트·제거 끝까지 시험** (계획 2-2). 관리자 UAC는 사용자가 허용.
3. **앱별 실사용 확인표** (계획 2-1). 사용자가 직접 쳐야 한다.
4. 설치 프로그램 형식(.exe), 공개 저장소·CI, SignPath 서명, 배포 마무리 (계획 2-3~2-6, 모두 사용자 결정 필요).

## 4. 이 PC의 지금 상태

- 설치판: `C:\Program Files\MultilangIME` (한국어 등록, 목록 파일 있음, 백엔드는 작업 "MultilangIME Backend"). 등록(CLSID)은 설치판을 가리킨다. 개발판 `deploy\x64`는 등록 안 됨(파일만 있음, `deploy\x64\old`는 일부러 보관).
- 재부팅 때 지워지도록 예약: 설치 폴더의 `multilang_tip.dll.old…` 2개.
- 개발용 자체 서명 인증서 "CN=Multilang IME Dev Signing"이 신뢰 목록에 남아 있음(지금은 안 쓰임). 사용자에게 `tools\uiaccess-dev-setup.ps1 -Remove`를 권했다. 그래서 설치판의 검색창 후보창은 지금 가려진다(서명 없음).
- 언어 고정 상태 파일: `%LOCALAPPDATA%\multilang-ime\lock.txt` (없으면 자동). 시험하다 바꿨으면 자동으로 되돌려 둘 것.
- Mozc 빌드 환경: `K:\mozc-build\mozc` (Python 3.12, Bazelisk, VS 2026 Community의 ATL).

## 5. 작업 요령과 함정

- **설치판 갱신**: 앱이 물고 있는 DLL·exe는 덮어쓸 수 없다. 새 패키지로 `install.ps1`을 다시 돌리면(관리자) 이름 바꾸기·재부팅 삭제까지 처리한다. 이미 열린 앱은 옛 DLL을 계속 쓰므로 그 앱을 다시 열어야 한다(아이콘이 안 보인다는 보고의 원인이었음). 32비트 앱(카카오톡)은 `x86\multilang_tip.dll`.
- **백엔드 재시작**: `Start-ScheduledTask "MultilangIME Backend"`. 이전 파이프가 덜 닫혀 첫 시도가 -1로 끝날 수 있다 → 몇 초 뒤 다시.
- **Claude 셸 제약**: 이 셸은 패키지 앱 문맥이라 백엔드 파이프에 직접 붙지 못하고(`\\.\pipe\multilang-ime.<user>` 경로 없음), 실제 `%LOCALAPPDATA%`도 가려진다. 로그 읽기·파일 지우기는 1회성 예약 작업(`cmd /c copy …`)으로 했다. 백엔드 로그: `%LOCALAPPDATA%\multilang-ime\backend.log`.
- **입력 시험**: 자동 입력 도구의 `type`은 IME를 거치지 않는다. 한/영 키는 `keybd_event(0x15, 0x72, …)`로 시험 창(`tools\ImeScopeTest.cs`, x64/x86 빌드)에 보낼 수 있었다. 작업 표시줄 확인은 전체 해상도 `CopyFromScreen` 캡처가 정확하다.
- **32비트 프로세스 모듈 확인**: 64비트 `tasklist /m`은 못 본다 → 32비트 PowerShell `Get-Process … .Modules`.
- **PowerShell 5.1**: 정션에 `Remove-Item`은 묻는 창을 띄워 멈춘다 → `cmd /c rmdir`. `.ps1`은 ASCII로(한글 주석은 BOM 없으면 깨질 수 있음). 파이썬으로 파일을 고칠 때 `\r`·`\x`·`\\` 이스케이프가 여러 번 망가졌다 → 경로·정규식이 든 수정은 Edit 도구나 raw 문자열 파일로.
- **진단 로그**: DLL 옆 `MULTILANG_TIP_DEBUG` + `logs\`(설치판은 관리자 필요). 끝나면 지운다.

## 6. 지켜야 할 것

- 사용자와 상관없는 파일·설정은 건드리지 않는다. 삭제는 `installer/files.ps1` 규칙(목록 파일 기준)만, 바꾸면 `tests/installer-files.ps1`에 시험 추가. 시험은 임시 폴더에서만.
- `data/private`(사용자 트윗 평가 데이터)는 커밋·전송 금지. 사용자 이름·이메일 등 개인 정보를 외부 요청에 넣지 않는다. 공개 전 커밋 이메일 노출 여부를 사용자에게 묻는다.
- 관리자 작업은 UAC로 사용자가 허용. 신뢰 인증서 같은 보안 설정은 스크립트를 만들어 사용자가 직접 실행.
- 파이프로 일본어를 확정하면 Mozc 학습이 오염된다 → Esc/Reset.
- 커밋 메시지 끝에 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Codex(ChatGPT)도 이 저장소에서 일한다. 시작할 때 `git log`·`git status`로 다른 쪽 변경을 확인하고, 계획 문서를 함께 갱신한다.
