# 서드파티 IME 요구사항·호환성 검토

검토일: 2026-11-23  
대상: `C:\Users\jaehw\Documents\multilang-ime`의 TSF 프런트엔드, 백엔드 통신, 등록 및 배포 폴더  
범위: Microsoft의 IME 기본 요구사항과 Windows 앱 호환성에 대한 정적 검토. 파일 수정이나 호환성 실행 시험은 하지 않았다.

## 요약

프로젝트는 Rust COM 텍스트 서비스와 TSF 키 이벤트 sink를 구현하고 있어, `IMM32` 대신 TSF를 쓰라는 Microsoft의 기본 방향과 맞는다. 다만 현재 저장소 상태만으로는 서명, AppContainer 통신, 32비트 앱 지원, UI-less 앱의 후보 UI 등 배포 호환성 요건을 충족한다고 보기 어렵다.

가장 먼저 확인할 위험은 키 이벤트 안에서 백엔드 연결·응답을 동기 대기하는 구조다. 백엔드가 없을 때 연결 시도가 최대 1.5초 지속될 수 있고, 파이프 응답도 최대 200ms 대기한다. 이는 입력 콜백이 실행되는 동안 앱 입력을 지연시킬 가능성이 있다.

## 항목별 검토

| 우선순위 | 항목 | 관찰 및 판단 |
|---|---|---|
| 높음 | 키 처리 중 동기 대기 | `tip/src/client.rs`는 요청 응답을 최대 200ms 기다린다. `connect()`는 백엔드 실행 후 파이프가 열릴 때까지 최대 1.5초 재시도하며, 키 이벤트 경로의 `OnKeyDown`에서 호출된다. 지연 시 앱의 입력 처리가 멈춘 것처럼 보일 수 있다. 설계 문서의 5ms 목표와 큰 차이가 있다. |
| 높음 | AppContainer와 백엔드 파이프 | `backend/src/main.rs`의 `CreateNamedPipeW`는 보안 속성 인자를 `None`으로 전달한다. AppContainer SID를 포함한 명시적 ACL은 코드에서 확인되지 않았다. 사용자별 파이프 이름만으로 앱 컨테이너에서의 접근이 보장되지는 않으므로, 패키지 앱에서 연결되는지와 파이프 ACL을 확인해야 한다. |
| 높음 | 코드 서명 및 설치 | 저장소에서 서명 설정·배포용 설치 패키지를 확인하지 못했다. Microsoft는 서드파티 IME에 디지털 서명을 요구한다. COM 클래스는 `HKLM` 아래에 등록하므로 현재 등록 방식은 관리자 권한이 필요한 시스템 전역 설치다. 서명된 최종 바이너리 및 설치·제거 절차를 별도로 검증해야 한다. |
| 높음 | x86 앱 지원 | 현재 `deploy` 산출물은 x64 중심이며 x86 TIP DLL 및 양쪽 비트수 설치·등록 구성이 확인되지 않았다. 64비트 Windows에서 x86 앱도 지원하려면 x86·x64 DLL을 각각 빌드하고 함께 설치해야 한다. README에 빌드 명령이 있다는 것만으로 실제 배포 호환성이 입증되지는 않는다. |
| 중간 | UI-less 앱과 후보 UI | 후보창은 TIP 프로세스 안에서 Win32 팝업 창으로 직접 그린다. 설계 문서에 계획된 `ITfCandidateListUIElement` 연동과 UI-less 앱 지원 등록은 구현에서 확인되지 않았다. 따라서 일반 편집 컨트롤에서의 동작을 게임 등 UI-less 앱 지원으로 간주할 수 없다. |
| 중간 | 터치 키보드와 후보창 겹침 | 후보창 위치는 조합 영역과 모니터 작업 영역 기준으로 계산한다. 터치 키보드 위치를 확인해 후보 UI를 재배치하는 처리는 확인되지 않았다. 키보드가 후보창을 가릴 수 있으므로 터치 키보드를 띄운 상태의 검증이 필요하다. |
| 중간 | 트레이 브랜딩 아이콘 | 시스템 트레이 지원 카테고리는 등록하지만, 브랜딩 아이콘은 DLL/EXE 리소스에서 제공해야 한다. 리소스 스크립트나 아이콘 리소스가 저장소에서 확인되지 않았다. 실제 리소스가 빠져 있으면 입력 표시기에 IME 아이콘 대신 언어 약어가 나타날 수 있다. |
| 낮음/기능 선택 | 터치 최적화 레이아웃 | `ITfFnGetPreferredTouchKeyboardLayout` 및 `ITfFunctionProvider` 구현은 보이지 않는다. 이는 터치 최적화 레이아웃을 요청하는 경우 필요한 기능이며, 구현하지 않아도 기본 클래식 터치 키보드 레이아웃을 사용할 수 있다. 독립 터치 키보드를 제공하는 방식은 사용자 지정 IME에서 허용되지 않는다. |

## 확인된 기반

- COM 텍스트 서비스가 `ITfTextInputProcessorEx`, `ITfKeyEventSink`, `ITfCompositionSink`, `ITfDisplayAttributeProvider`를 구현한다.
- 조합 및 확정 텍스트를 TSF 편집 세션에서 반영한다.
- 한국어와 일본어 프로필을 등록한다.
- TIP 구현에서 IMM32 기반 입력 처리는 확인되지 않았다.

## 권장 검증 순서

1. 키 입력 중 백엔드 미실행·시작 지연·응답 시간 초과 상황에서 앱이 멈추거나 글자를 잃지 않는지 측정한다.
2. 패키지 앱(AppContainer)에서 파이프 연결 및 모델·사전 접근을 확인하고, 필요한 파이프 ACL을 검토한다.
3. x86 앱과 x64 앱에서 각각 프로필 등록, 입력, 후보 선택, 제거 후 재설치를 확인한다.
4. UI-less 앱에서 후보 목록 제공 동작을 확인하고, 미지원이면 지원 범위를 명시한다.
5. 터치 키보드 표시 중 후보창 가림 여부와 입력 표시기 아이콘을 확인한다.
6. 배포 산출물에 코드 서명과 설치·제거 절차가 포함되는지 확인한다.

## Microsoft 문서

- [Input Method Editors (IME)](https://learn.microsoft.com/en-us/windows/apps/develop/input/input-method-editors) — 서드파티 IME의 서명, TSF 및 일반 UI 요구사항.
- [Custom Input Method Editor (IME) requirements](https://learn.microsoft.com/en-us/windows/apps/develop/input/input-method-editor-requirements) — TSF 호환성, 앱 컨테이너 제약, 트레이 아이콘, 터치 키보드 가이드.
- [64-Bit Considerations](https://learn.microsoft.com/en-us/windows/win32/tsf/64-bit-platform-considerations) — 32·64비트 텍스트 서비스 DLL 구성.
- [Text Services Framework](https://learn.microsoft.com/en-us/windows/win32/tsf/text-services-framework) — TSF 구조 및 개발자 참고 문서.

## 검토 한계

코드와 현재 배포 폴더를 읽은 정적 검토다. 서명 상태, 설치 권한, Windows 버전별 동작, AppContainer ACL, 실제 x86 빌드, 터치 키보드 및 UI-less 앱 동작은 실행 환경에서 검증하지 않았다. 저장소에 구현이 보이지 않는다는 판단은 별도 외부 설치 도구나 배포 파이프라인에도 해당 기능이 없다는 뜻은 아니다.
