# 배포 (릴리스) 절차

## 만들기

```powershell
powershell -ExecutionPolicy Bypass -File installer\build-release.ps1 -Version 0.1.0
```

- x64(`tip`, `backend`)와 x86(`tip`)을 빌드해 `dist\MultilangIME-<버전>\`에 모으고 zip과 SHA-256을 만든다.
- 넣는 것: TIP DLL(x64 루트, 32비트는 `x86\`), `backend.exe`, `engine_model.json`, `hanja.txt`, `mozc\`(공식 MSI에서 꺼낸 mozc_server와 VC 런타임, 학습 DB는 빼고), `LICENSE.txt`(GPL-3.0), `THIRD-PARTY-NOTICES.md`, `licenses\`, 사용자용 `README.md`(`installer\README-install.md`), 설치·제거 스크립트.
- 재료: `data\engine_model.json`(`python -m ime_v0.export_model`), `deploy\x64\mozc\`.

## 설치 스크립트 (`installer\install.ps1`)

관리자 권한(UAC)으로 `C:\Program Files\MultilangIME`에:
1. 이전 설치의 백엔드·Mozc 종료, 파일 복사 (앱이 물고 있는 DLL은 이름을 바꿔 두고 다음 설치 때 지움).
2. 샌드박스 앱용 읽기 권한: ALL APPLICATION PACKAGES(S-1-15-2-1), ALL RESTRICTED APPLICATION PACKAGES(S-1-15-2-2).
3. `regsvr32`로 64비트(System32)·32비트(SysWOW64) 등록. 등록 카테고리에 UI-less(`GUID_TFCAT_TIPCAP_UIELEMENTENABLED`) 포함.
4. 작업 스케줄러 "MultilangIME Backend": Users 그룹, 로그온 때, 그 사용자 권한(제한)으로 실행.
5. 설치한 사용자의 한국어 입력 방법 목록에 추가 (`-NoLanguageList`로 끔).

`-DryRun`은 단계만 보여 준다. 제거는 설치 폴더의 `uninstall.ps1` (`-RemoveUserData`로 설정·학습까지).

32비트 앱의 TIP는 `x86\`에 있으므로 백엔드를 직접 띄울 때 한 단계 위 폴더의 `backend.exe`를 찾는다.

## 개발용 배포(`deploy\x64`)와의 관계

설치판과 개발판은 같은 CLSID·작업 이름을 쓴다. 설치하면 등록과 작업이 Program Files 쪽으로 바뀐다.
개발판으로 돌아가려면 관리자 권한으로 `regsvr32 deploy\x64\multilang_tip.dll`, `tools\backend-task.ps1 install`.

## 아직 남은 것

- **코드 서명**: 서명 안 된 DLL·exe는 SmartScreen 경고, 일부 보안 제품 차단. 인증서가 필요하다 (무료 오픈소스는 SignPath Foundation 등 검토).
- **검색창 후보창**: UIAccess 후보창 프로세스. UIAccess는 서명 + 보안 위치(Program Files) 설치가 조건이라 서명과 함께 한다.
- **설치 프로그램 형식**: 지금은 zip + PowerShell 스크립트. Inno Setup(.exe) 또는 MSI로 감쌀지 결정 (시작 메뉴·프로그램 추가/제거 등록).
- **Mozc**: 공식 빌드 그대로 배포 중. 자체 빌드 도우미로 바꿀지는 next-tasks C-10.
- 다른 사용자 계정의 입력 방법 목록에는 자동으로 추가되지 않는다 (각자 설정에서 추가, 프로필은 기본 사용으로 등록됨).
