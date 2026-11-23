# Mozc 표준 입출력 도우미 (`mozc_stdio.exe`)

공식 `mozc_server.exe` 대신 Mozc 엔진을 직접 빌드해 백엔드의 자식 프로세스로 띄운다 (next-tasks C-10).

## 왜

- 공식 서버는 샌드박스 런처의 가장 토큰이 없으면 실행을 거부해, 일시 정지 → 토큰 복제 → 재개로 띄워야 했다. 패키지 앱 문맥에서 띄우면 다른 프로세스가 붙지 못하는 문제도 있었다.
- 서버의 `session.ipc`·파이프·사용자 프로필(LocalLow\Mozc)을 거치지 않는다. 백엔드가 끝나면(표준 입력이 닫히면) 도우미도 끝난다.
- 학습 데이터는 `%LOCALAPPDATA%\multilang-ime\mozc`에 따로 둔다 (공식 Mozc 설치와 섞이지 않음, 이전 학습은 이어지지 않음).

## 프로토콜

요청·응답마다 4바이트 길이(little endian) + protobuf. 요청은 `mozc.commands.Input`, 응답은 `mozc.commands.Output`.
Mozc 서버의 `SessionServer::Process`와 같은 처리(`SessionHandler::EvalCommand`)라 클라이언트 코드는 전송만 다르다 (`mozc/src/lib.rs`의 `Client::Stdio`).
백엔드는 `mozc\mozc_stdio.exe`가 있으면 그것을, 없으면 `mozc\mozc_server.exe`를 쓴다.

## 빌드

준비 (한 번, `K:\mozc-build`): Python 3.12, Bazelisk (`winget`), `git clone --recursive https://github.com/google/mozc`, `python build_tools/update_deps.py` (LLVM, MSYS2, ninja).
Visual Studio는 ATL(`atlbase.h`)이 있는 것이어야 한다 (이 PC: VS 2026 Community, 2022 Build Tools에는 없음). 스크립트가 찾아 `BAZEL_VC`로 쓴다.

```powershell
$env:PATH = "$env:LOCALAPPDATA\Programs\Python\Python312;$env:LOCALAPPDATA\Microsoft\WinGet\Links;$env:PATH"
powershell -ExecutionPolicy Bypass -File mozc-helper\build.ps1   # -> target\mozc-helper\mozc_stdio.exe
```

스크립트는 `mozc-helper\`를 Mozc의 `src\multilang\`로 복사하고, `//session:session_handler`와 `//engine:engine_factory`의 visibility에 `//multilang:__pkg__`를 더한 뒤 `bazelisk build //multilang:mozc_stdio --config oss_windows --config release_build`. 이 PC에서 처음 빌드는 10~20분 정도 (다시 빌드는 캐시로 빠름).

## 측정 (2026-11-23)

- 변환 결과: 공식 서버와 같다 (きしゃ 30, こうせい 104, かんじ 44 후보 등, `mozc_count` 예제).
- 지연: 키 보내기 0.48 ms, 변환 0.53 ms, 취소 0.50 ms, 빈 호출 0.03 ms (`mozc_timing` 예제). 띄우고 준비까지 약 340 ms (백엔드 시작 때 백그라운드).
- 크기 22 MB (사전 포함), VC 런타임 DLL 필요 (`mozc\`에 함께 둔다).

## 라이선스

Mozc는 BSD-3-Clause. 도우미 코드(`mozc-helper/`)는 GPL-3.0-or-later. 배포물에는 Mozc의 LICENSE와 credits를 함께 넣는다.
