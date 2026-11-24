# 배포 (릴리스) 절차

## 만들기

```powershell
powershell -ExecutionPolicy Bypass -File installer\build-release.ps1 -Version 0.1.0 -Setup
```

- x64(`tip`, `backend`, `candhost`)와 x86(`tip`)을 빌드해 `dist\MultilangIME-<버전>\`에 모으고 zip과 SHA-256을 만든다. `-Setup`이면 설치 프로그램 `MultilangIME-<버전>-setup.exe`(Inno Setup 6, `installer\MultilangIME.iss`)와 그 SHA-256도.
- 설치 .exe는 릴리스 폴더를 임시 폴더에 풀고 고른 입력 언어로 `install.ps1`을 실행하는 껍데기다. 제거는 앱 및 기능의 `uninstall.ps1`. 조용히: `/VERYSILENT /INPUTLANG=ko|ja|en`.
- 넣는 것: TIP DLL(x64 루트, 32비트는 `x86\`), `backend.exe`, `engine_model.json`, `hanja.txt`, `mozc\`(직접 빌드한 `mozc_stdio.exe`와 VC 런타임), `LICENSE.txt`(GPL-3.0), `THIRD-PARTY-NOTICES.md`, `licenses\`, 사용자용 `README.md`(`installer\README-install.md`), 설치·제거 스크립트.
- 설정 화면 `MultilangIME-Settings.exe`도 함께 빌드해 넣는다 (`settingsuild.ps1`).
- 재료: `data\engine_model.json`(`python -m ime_v0.export_model`), `third_party\libhangul\hanja.txt`(`tools\fetch-third-party.ps1`), `target\mozc-helper\mozc_stdio.exe`와 Mozc의 `credits_en.html`(`mozc-helper\build.ps1`), VS redist의 VC 런타임.

## CI (GitHub Actions, `.github/workflows/build.yml`)

| 작업 | 언제 | 내용 |
| --- | --- | --- |
| core | 모든 push·PR | 모델 데이터 생성(`requirements-model.txt`, Python 3.10, `data/model_params.json`), `tools\fetch-third-party.ps1`(libhangul hanja.txt, 커밋 고정 + SHA-256), `cargo test`(engine·ipc·backend·tip, candhost는 `CANDHOST_NO_UIACCESS=1`), x64·x86 빌드, 설정 화면, 설치기 시험 |
| mozc | `v*` 태그, 수동 실행 | Mozc 소스(커밋 고정 `MOZC_COMMIT`) + `update_deps.py` + `mozc-helper\build.ps1`, Bazel 디스크 캐시 |
| package | mozc 뒤 | Inno Setup 준비(없으면 choco) → `installer\build-release.ps1 -NoBuild -Setup` → `release` 아티팩트(zip, 설치 .exe, sha256). 태그 빌드는 태그 버전, 수동 실행은 `0.0.0-dev.<번호>` |

- 액션은 커밋 SHA로 고정 (Mozc 저장소와 같은 방식).
- git에 없는 입력은 모두 CI에서 다시 만든다: 모델(개발용과 같은 바이트 확인), hanja.txt(체크섬), Mozc 도우미(고정 커밋 소스 빌드).
- 코드 서명은 이 `release` 아티팩트(또는 그 안의 exe/dll)를 서명 단계에 넘기는 식으로 붙인다.
- 공개 저장소 https://github.com/hoong0515/Multilang-ime 에서 동작 확인 (2026-11-24). v0.1.0 릴리스 파일은 태그 빌드 45243627966의 결과물.

## 설치 스크립트 (`installer\install.ps1`)

관리자 권한(UAC)으로 `C:\Program Files\MultilangIME`에:
1. 이전 설치의 백엔드·Mozc 종료, 파일 복사 (앱이 물고 있는 DLL은 이름을 바꿔 두고 다음 설치 때 지움).
2. 샌드박스 앱용 읽기 권한: ALL APPLICATION PACKAGES(S-1-15-2-1), ALL RESTRICTED APPLICATION PACKAGES(S-1-15-2-2).
3. `regsvr32`로 64비트(System32)·32비트(SysWOW64) 등록. 등록 카테고리에 UI-less(`GUID_TFCAT_TIPCAP_UIELEMENTENABLED`) 포함.
4. 작업 스케줄러 "MultilangIME Backend": Users 그룹, 로그온 때, 그 사용자 권한(제한)으로 실행.
5. 시작 메뉴 바로 가기 "Multilang IME Settings" (모든 사용자).
6. 설치한 사용자의 한국어 입력 방법 목록에 추가 (`-NoLanguageList`로 끔).

`-DryRun`은 단계만 보여 준다. 제거는 설치 폴더의 `uninstall.ps1` (`-RemoveUserData`로 설정·학습까지).

32비트 앱의 TIP는 `x86\`에 있으므로 백엔드를 직접 띄울 때 한 단계 위 폴더의 `backend.exe`를 찾는다.

## 파일 삭제 안전 규칙 (`installer\files.ps1`, 시험: `tests\installer-files.ps1`)

- 설치 폴더는 전용 폴더만: 새 폴더, 빈 폴더, 이전 설치(목록 파일 `install-manifest.txt`가 있거나, 앱 및 기능에 등록된 위치이면서 우리 파일이 있는 0.1.0 설치). 아니면 설치를 거부한다. Windows·Program Files·사용자 폴더·드라이브 루트 등은 거부.
- 지우는 것은 목록 파일에 적힌 파일과 그 `<이름>.old<숫자>` 사본뿐이다. 폴더를 훑어서 지우지 않는다. 목록의 줄은 폴더 안의 상대 경로여야 한다(`..`, 드라이브, 와일드카드 거부).
- 폴더는 비었을 때만 지운다. 사용자가 넣은 파일(예: 진단용 `logs`)은 남고, 제거 끝에 남은 파일 수를 알려 준다.
- 앱이 물고 있는 파일(TIP DLL)은 이름을 바꿔 두고 재부팅 때 지우도록 예약한다(MoveFileEx, 관리자). 새 판을 설치할 때도 남은 사본을 다시 정리한다.
- 프로세스 종료는 우리 실행 파일 이름 + 설치 폴더 경로(`<폴더>\` 접두사)가 모두 맞을 때만.
- 시험: 임시 폴더에서 관련 없는 파일·비슷한 이름·목록 조작(폴더 밖 경로)·매핑된 DLL·0.1.0 설치를 확인한다. 관리자로 돌리면 재부팅 예약까지 확인한다.

## 개발용 배포(`deploy\x64`)와의 관계

설치판과 개발판은 같은 CLSID·작업 이름을 쓴다. 설치하면 등록과 작업이 Program Files 쪽으로 바뀐다.
개발판으로 돌아가려면 관리자 권한으로 `regsvr32 deploy\x64\multilang_tip.dll`, `tools\backend-task.ps1 install`.
개발판 폴더에는 stub(`multilang_tip.dll`)과 본체(`multilang_tip_impl.dll`)를 나란히 둔다 (가리키는 파일
`multilang_tip.impl`이 없으면 stub은 옆의 본체를 읽는다). 설치판은 본체를 `impl\<해시>\`에 둔다 (`installer\build-release.ps1`).

## 파일 정보와 앱 목록

- 버전 정보와 아이콘: `res/build_res.rs`(tip·backend·candhost의 build.rs가 include, embed-resource로 rc.exe)가 VERSIONINFO(제품 이름, 버전 = Cargo 버전, GPL-3.0)와 `res/multilang.ico`(가·あ, `tools/make-icon.py`로 생성)를 넣는다. TIP의 아이콘 1번 = 입력 표시기 아이콘. 설정 화면은 csc `/win32icon` + 어셈블리 특성.
- 입력기 이름은 "Multilang IME" (개발판 등록에 남은 "(dev)"는 다시 등록할 때 바뀐다).
- 설치 스크립트가 "앱 및 기능"(Uninstall 키)에 등록하고, 제거 스크립트가 지운다.

## 릴리스 순서

1. `CHANGELOG.md`의 "다음 판" 절을 버전 절로 바꾸고 커밋, push.
2. `git tag -a v<버전> -m "Multilang IME <버전>"` 후 태그 push → CI가 태그 버전으로 zip·설치 .exe·sha256을 만든다.
3. 태그 빌드의 `release` 아티팩트로 GitHub 릴리스 초안(설치 방법, 알려진 제한, "Code signing policy" 절, SHA-256) → 확인 후 공개.

## 시험

- `tests\installer-files.ps1`: 설치·제거 파일 처리 (임시 폴더, CI에서도).
- `tests\install-cycle.ps1` (관리자): 이 PC에서 재설치 → 제거 → 설치, 등록·작업·바로 가기·언어 목록·파일 확인. 재부팅 뒤 `-AfterReboot`.
- 앱별 실사용: `docs/compat-matrix.md`.

## 아직 남은 것

`docs/codex-release-plan.md` 참고. 요약: 코드 서명 방법 결정(사용자) → CI 서명 단계와 서명판 릴리스(검색창 후보창은 서명판에서만 보임), 오른쪽 Alt·半角/全角 실측, 확인표의 남은 칸.
