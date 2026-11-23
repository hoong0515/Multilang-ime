# Codex 인계: 배포 준비 계획 (2026-11-23)

작성: Claude Code. 이어서 작업할 Codex용. 이 저장소(multilang-ime)의 첫 공개 배포(v0.1.0)까지 남은 일과 순서, 지켜야 할 것을 정리한다.
현재 기준 커밋: `31caa63` (master). 개요는 `README.md`, 절차는 `docs/release.md`, 할 일 기록은 `docs/next-tasks.md`.

## 1. 지금 상태 (끝난 것)

| 영역 | 내용 | 커밋 |
| --- | --- | --- |
| 라이선스 | GPL-3.0-or-later `LICENSE`, `THIRD-PARTY-NOTICES.md` (Mozc BSD-3, libhangul 한자 BSD-3, wordfreq CC BY-SA 4.0, Rust 크레이트) | 1be1eef |
| 패키지 | `installer/build-release.ps1` → `dist/MultilangIME-<ver>-win.zip` + sha256 | 1be1eef, 5e58349 |
| 설치·제거 | `installer/install.ps1`·`uninstall.ps1`·`files.ps1`: Program Files 복사, x64·x86 regsvr32, 앱 컨테이너 읽기 권한, Users 그룹 로그온 작업, 시작 메뉴, 앱 및 기능, `-Language ko|ja|en` | 1be1eef, 7c931a9, 3613fbd |
| 삭제 안전 | 전용 폴더만 설치, `install-manifest.txt`에 적힌 파일과 `<이름>.old<숫자>`만 삭제, 쓰는 중인 파일은 재부팅 때 삭제(MoveFileEx), 시험 `tests/installer-files.ps1` | 31caa63 |
| Mozc | 직접 빌드한 표준 입출력 도우미 `mozc_stdio.exe` (`mozc-helper/`, `docs/mozc-helper.md`) | 0d8ad4b |
| 검색창 후보창 | UIAccess 후보창 프로세스 `MultilangIME-CandWin.exe` (`candhost/`). 개발용 자체 서명으로 동작 확인. 정식 서명이 있어야 배포판에서 동작 | e55537f |
| 설정 화면 | `settings/` WinForms: 사용할 언어, 자동 꺼짐 앱, 학습 지우기, 일본어 Space, 사용자 사전 탭 | c724160, a6f3736 |
| 표시 | 버전 정보·아이콘, 입력 표시기 아이콘(지금 언어 + 자동/고정), 한/영 키 언어 고정 + 커서 아래 알림 | 7c931a9, 0f31381, 6f3fdaa |
| CI | `.github/workflows/build.yml` (core / mozc / package). 아직 GitHub에서 한 번도 돌지 않음 | 5e58349 |
| Codex 리뷰 반영 | 제거는 등록된 설치 폴더에서만(다른 곳의 사본은 아무것도 안 바꿈), Windows 아래·정션 폴더 금지와 정션 경유 쓰기·삭제 차단(`Test-SafeTarget`), 다른 관리자 계정으로 승격 시 사용자별 단계 생략, `-Version` 검증. 정션 시험의 PowerShell 5.1 멈춤 수정 | 7ba925a, 이후 커밋 |

이 PC의 상태: 설치판이 `C:\Program Files\MultilangIME`에 깔려 있다 (한국어 등록, 목록 파일 있음). 남은 DLL 사본 2개는 재부팅 때 지워지게 예약됨. 개발용 자체 서명 인증서("CN=Multilang IME Dev Signing")가 아직 신뢰 목록에 있다 (지금은 쓰이지 않음, 사용자에게 `tools\uiaccess-dev-setup.ps1 -Remove`를 권했음).

## 2. 남은 일 (순서대로)

각 항목: 할 일 → 끝난 기준 → 확인 방법. **[사용자]** 표시는 사용자의 결정·승인이 먼저 필요한 것.

0. **보안 검토 반영 (출시 전 먼저)** — Codex 정적 검토 `docs/security-review-2026-11-23.md` (실측 안 함, 코드로 먼저 확인할 것)
   - R1 높음: 백엔드 파이프 요청 크기·동시 연결 무제한 (`backend/src/main.rs` `read_message`, `PIPE_UNLIMITED_INSTANCES`) → 최대 요청 크기, 연결 수 상한, 문자열 필드 길이 제한 + 시험. **[완료]** `backend/src/pipe.rs`: 요청 16 KiB 상한(넘으면 연결 끊음), 동시 연결 128개(넘으면 받자마자 끊음, 인스턴스 상한 129), `Hello.app` 260자·제어 문자·경로 구분자 거부, 잘못된 요청 로그는 1분에 한 번. 시험: 실제 파이프로 상한·상한+1·4 MiB·연속 요청, 연결 수 세기.
   - R2 중~높음: 입력 용도(비밀번호 등) 확인이 실패하면 IME가 켜진 채로 (fail-open), 여러 용도 중 첫 항목만 봄 (`tip/src/lib.rs` `input_off`) → 하나라도 보호 용도면 끄기, 실패 시 정책 결정 **[사용자]**, 앱별 비밀번호 칸 확인.
   - R3 중간: 앱 컨테이너도 열 수 있는 파이프로 `ClearLearning`(학습 전부 삭제) 호출 가능 → TIP용 키 처리와 관리 명령 분리(관리 명령은 앱 컨테이너 거부 또는 별도 파이프). **[완료]** 연결할 때 `GetNamedPipeClientProcessId` → 프로세스 토큰으로 샌드박스 여부·무결성을 보고, 샌드박스이거나 Medium 미만(또는 확인 실패)이면 `ClearLearning`을 거부하고 `error:"denied"` 응답 (키 처리는 그대로). 설정 앱은 거부 응답이면 파일도 지우지 않고 알림. 시험: 이 프로세스는 허용, Low 무결성 자식 프로세스는 거부(실제 파이프). 남은 확인: 설치판 갱신 뒤 설정 화면의 "학습 기록 지우기" 실사용. 관리자 권한으로 띄운 설정 화면은 토큰을 못 열어 거부됨(의도).
   - R4 중간: UIAccess 후보창이 `WM_COPYDATA` 발신자를 확인하지 않음 (`candhost/src/main.rs`) → 발신 창/프로세스 확인, 후보 수·문자열 길이 상한. **[완료]** 창 메시지는 발신자를 믿을 수 없어서, `show`는 전경 창이 셸 프로세스(SearchHost 등, TIP의 `SHELL_HOSTS`와 같은 목록)일 때만 따르고 아니면 숨김. 본문 64 KiB, 후보 32개, 글자 1024자 상한. 시험: `CANDHOST_NO_UIACCESS=1 cargo test -p candhost` (UIAccess 매니페스트가 있으면 시험 exe가 실행되지 않음). 남은 확인: 서명된 설치판(5단계)에서 검색창 후보창이 그대로 뜨는지.
   - R5 낮음: 파이프 생성에 원격 클라이언트 거부 플래그(`PIPE_REJECT_REMOTE_CLIENTS`) 없음 → 추가. **[완료]** `pipe::create`에 추가 (로컬 접속 시험 통과, 원격 거부는 실측 안 함).
   - 기준: 항목마다 고침 + 시험, 문서의 상태 갱신.

1. **실사용 확인표 만들기**
   - 할 일: 설치판으로 앱별 동작을 표로 기록한다 (`docs/compat-matrix.md` 새로). 앱: 메모장, 워드패드/문자표(옛 입력칸), 카카오톡(32비트, 조합 창), Chrome·Edge·Whale, VS Code, Office(Word), 작업 표시줄 검색창, 관리자 권한 앱, 게임 1개. 항목: 한·영·일 자동 전환, 후보창 위치, Shift 탭, 한/영 고정과 알림, 자동 꺼짐(비밀번호 칸).
   - 기준: 모든 칸에 결과(정상/문제+증상). 문제는 `docs/next-tasks.md`에 항목으로.
   - 확인: 사용자가 직접 치는 것이 필요하다 (자동 입력 도구는 IME를 거치지 않는다). 진단 로그는 DLL 옆 `MULTILANG_TIP_DEBUG` 파일 + `logs\` (Program Files는 관리자 필요).

2. **설치·업데이트·제거 끝까지 시험** (관리자 UAC는 사용자가 승인)
   - 할 일: 현재 설치판에서 (a) 같은 판 재설치, (b) 제거(`uninstall.cmd`), (c) 재부팅 뒤 폴더·예약 정리 확인, (d) 새로 설치, (e) `-Language en`/`ja` 설치 후 작업 표시줄·메모장 글꼴 확인 뒤 `ko`로 되돌리기.
   - 기준: 제거 뒤 `C:\Program Files\MultilangIME`, 등록(HKLM CLSID 64·32, CTF\TIP — `regsvr32 /u`의 DllUnregisterServer가 TSF API로 지운다), 작업, 시작 메뉴, 앱 및 기능, HKCU 프로필 설정이 모두 없고, 재부팅 뒤 `PendingFileRenameOperations`에 우리 경로가 없음. 사용자 데이터(`%LOCALAPPDATA%\multilang-ime`)는 `-RemoveUserData`일 때만 지워짐.
   - 확인: `tests/installer-files.ps1` (관리자·일반 둘 다), 레지스트리 조회. **[사용자]** 언어 목록이 바뀌는 `-Language en` 시험은 사용자 승인 후.

3. **설치 프로그램 형식 결정과 포장** **[사용자]**
   - 선택지: (a) 지금처럼 zip + `install.cmd`, (b) Inno Setup .exe가 같은 스크립트를 부름(언어 고르기 화면, 관리자 요청, 제거는 기존 `uninstall.ps1`), (c) MSI(WiX).
   - 권장: (b). 스크립트 로직(`files.ps1`의 안전 규칙)을 그대로 쓰고 감싸기만 한다. Inno Setup 설치(다운로드)는 사용자 승인 필요.
   - 기준: .exe 하나로 설치·제거, 언어 선택, 업그레이드 시 기존 설치 감지. CI의 package 작업에 .exe 빌드 추가.

4. **공개 저장소와 CI 첫 실행** **[사용자]**
   - 할 일: GitHub 공개 저장소 생성·푸시는 사용자가 결정한다. 커밋 작성자 이메일이 공개되니 바꿀지 먼저 묻는다. `data/private`(사용자 트윗)는 절대 올리지 않는다 (`.gitignore`로 제외돼 있음, 푸시 전 `git ls-files`로 확인).
   - CI: 수동 실행(workflow_dispatch)이면 core·mozc·package 세 작업이 모두 돈다 (push·PR은 core만, `v*` 태그는 셋 다). 돌려 보며 고친다. mozc 작업은 런너에서 오래 걸릴 수 있다(타임아웃 150분, Bazel 디스크 캐시).
   - 기준: `release` 아티팩트(zip + sha256)가 나온다.

5. **코드 서명** **[사용자]**
   - 할 일: SignPath Foundation 신청 (OSI 라이선스, 공개 저장소, CI 빌드, 2단계 인증). 승인 후 CI에 서명 단계: DLL(x64·x86), `backend.exe`, `MultilangIME-CandWin.exe`, `MultilangIME-Settings.exe`, `mozc_stdio.exe`, 설치 .exe. 릴리스 문구 "Free code signing provided by SignPath.io, certificate by SignPath Foundation".
   - 기준: 서명된 설치판에서 검색창 후보창이 보인다 (UIAccess는 서명 + Program Files 조건).
   - 참고: Azure Artifact Signing은 국내 개인 불가, Store(MSIX)는 TSF IME에 맞지 않음 (`docs/release.md`).

6. **배포 마무리**
   - 버전 0.1.0 확정, `CHANGELOG.md`(또는 릴리스 노트), README 스크린샷, `docs/release.md`의 "아직 남은 것" 갱신, 기획서와 `docs/next-tasks.md` 상태 갱신.

## 3. 보류·결정 필요 (구현 전에 사용자와 정할 것)

- 한/영 키가 없는 키보드(미국·일본 배열)의 언어 고정 키 (예: 오른쪽 Alt 탭, Ctrl+Shift+숫자, 아이콘 메뉴).
- 설정 화면에서 등록 언어 바꾸기(지금은 설치 때만).
- 다른 사용자 계정: 설치한 계정의 입력 방법 목록에만 추가된다. 다른 계정은 각자 추가해야 한다(안내만 할지).

## 4. 반드시 지킬 것

- **사용자와 상관없는 파일·설정을 건드리지 않는다.** 삭제는 `installer/files.ps1`의 규칙(목록 파일 기준)만 쓴다. 폴더를 훑어서 지우는 코드를 새로 만들지 않는다. 바꾸면 `tests/installer-files.ps1`에 시험을 더한다.
- **사용자 데이터는 로컬에만.** `data/private`(트윗 평가 데이터)는 커밋·전송 금지.
- **관리자 작업은 사용자 승인으로.** UAC 창은 사용자가 허용한다. 신뢰 인증서 추가·삭제 같은 보안 설정은 스크립트를 만들어 사용자가 실행하게 한다.
- **Mozc 학습 오염 주의**: 파이프로 시험할 때 일본어를 확정하면 실제 학습에 남는다. Esc/Reset으로 끝낸다.
- 커밋 메시지 끝: `Co-Authored-By:` 줄은 각 도구의 규칙을 따른다 (Claude 커밋은 Claude 줄).

## 5. 작업 요령 (이 PC)

- 설치판 갱신: 쓰는 중인 DLL·exe는 덮어쓸 수 없다. 이름을 바꾸고 새 파일을 넣는다 (`install.ps1`이 처리). 이미 열린 앱은 옛 DLL을 계속 쓰므로 앱을 다시 열어야 새 입력기가 적용된다.
- 백엔드 재시작: `Start-ScheduledTask "MultilangIME Backend"`. 바로 다시 띄우면 이전 파이프가 안 닫혀 한 번 실패할 수 있다(작업이 1분 뒤 재시도).
- 개발판(`deploy\x64`)과 설치판은 같은 CLSID·작업 이름을 쓴다. 지금 등록은 설치판 쪽.
- 32비트 프로세스의 모듈 확인은 32비트 PowerShell의 `Get-Process … .Modules`로 (64비트 tasklist는 못 볼 수 있다).
- 빌드·시험: `cargo test -p engine -p ipc -p backend`, `powershell -File tests\installer-files.ps1`, `powershell -File installer\build-release.ps1 [-NoBuild]`. Mozc 도우미는 `K:\mozc-build\mozc` + `mozc-helper\build.ps1` (VS 2026 Community의 ATL 필요).
