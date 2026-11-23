# 다음 작업 목록 (2026-11-23 기준)

현재 상태: v1 M1~M4 완료, M5 진행 중, 배포 준비 일부 완료 (`docs/release.md`). 사용자 평가로는 기본 동작(한·영·일 자동 전환, 후보창, 일본어 Mozc 변환)은 원하는 대로 된다. 남은 것은 아래 순서로 진행한다.

## A0. 새로 발견 (2026-11-23, 우선)
- **(해결, 조합 창) 옛 Win32 입력칸(CUAS 경유)에서 조합이 키마다 끊김** — README "옛 Win32 입력칸: 조합 창".: WinForms TextBox 등에서 미리보기가 d → k → s처럼 한 글자씩만 나오고 그대로 들어간다. 조합이 끝났다는 알림(OnCompositionTerminated) → reset으로 백엔드가 매번 처음부터 시작하는 것으로 추정. 메모장은 정상. 많은 앱이 옛 입력칸을 쓰므로 배포 전에 꼭 고쳐야 한다. 시험 창: `tools/ImeScopeTest.cs`.

## A. 바로 할 것 (작고 효과 큼)

1. **(완료 51fe375) 방향키로 후보 고르기** (사용자 요청)
   - 후보창이 열려 있을 때 ↓/→ 다음, ↑/← 이전.
   - 후보창이 없을 때의 방향키는 지금처럼 먼저 확정하고 앱에 넘긴다.
   - 손댈 곳: `backend/src/session.rs` `test`/`key` (VK_LEFT 0x25 ~ VK_DOWN 0x28).
2. **(완료) TIP 시간 초과 때 글자 유지** — README "M5 안정화 4". 키 기록 + 새 연결 때 되살리기 + 응답 없을 때 로마자로 이어 붙이기.
   - 백엔드 응답이 200 ms를 넘으면 지금은 키를 놓쳐 조합이 어긋난다.
   - 시간 초과 시 친 키를 그대로 조합에 넣고, 다음 응답 때 백엔드와 다시 맞춘다 (`tip/src/client.rs`, `tip/src/lib.rs`).
   - Mozc 미리 띄우기(d859d97)로 주된 원인은 없앴으니 보험 성격.
3. **(완료) 한국어 발음 → 한자 변환 (단어 단위)** — 후보 순서: 한 언어 후보 → 한자(漢) → 섞인 언어 후보. 한자 순서는 사전 순(글자 빈도 정렬은 더 나빴음) + 고른 횟수 학습(`hanja_learn.json`). 예: 한국은 寒國이 韓國보다 앞이라 한 번 고르면 고쳐진다.
   - 한국어 토큰의 Shift 탭 후보창에 한자 후보를 덧붙인다 (학교 → 學校). 자동 변환 없음.
   - 데이터: libhangul `hanja.txt` (파일 머리말 기준 BSD-3-Clause, `deployd\hanja.txt`).
   - 고른 한자는 학습 기록에 남긴다.
4. **실사용 확인**
   - IME 켠 직후의 Shift 탭이 실제 키보드에서 괜찮은지 확인.
   - 백엔드 로그의 `slow request`를 모아 남은 지연 원인 확인 (작업 스케줄러 백엔드 로그는 Claude 셸에서 안 보이므로 사용자가 `%LOCALAPPDATA%\multilang-ime\backend.log`를 확인).

## A+. 2026-11-23 추가 요청 (완료)
- 문맥과 상관없이 모든 언어의 후보 (언어 묶음), Tab/Shift+Tab으로 묶음 이동, 기호·숫자 Shift 탭 변환. README "M5 안정화 3" 참고.
- 영단어+일본어 조사의 남은 실패(`mailwookutta` 등)는 실사용에서 쓸 일이 없다 (일본어 입력 중엔 メール로 친다) — 사용자 판단, 우선순위 낮음.
- (해결) 백엔드 재시작 직후 첫 Shift 탭 무시: 끊긴 연결을 다시 연결해 재시도.

## A++. 후보창 여러 쪽 (2026-11-23, 완료)
- 일본어·한자에서 원하는 변환이 후보에 없던 문제: Mozc 첫 쪽(candidate_window, 9개)만 받던 것을 모든 후보(all_candidate_words)로 바꿈 (예: こうせい 104개, かんじ 44개). 상한: Mozc 100, 한자 40, 전체 160.
- 후보창은 한 쪽에 9개, 숫자 1~9는 지금 쪽 안에서 고름, Page Down/Up으로 쪽 이동 (↑↓·Shift 탭은 쪽을 넘어 이어짐), 맨 아래 "◀ 2 / 5 ▶" 표시. 백엔드가 지금 쪽만 보낸다 (`CandidateList.page/pages`).
- 숫자 키가 1~5에서 1~9로 늘었다 (쪽 크기 상수 `PAGE`).
- 첫 Shift 탭은 지금처럼 다음 언어 묶음으로 가므로, 한자가 많은 한국어 단어는 영어 후보가 뒤쪽에 있다 (그 쪽이 열림).

## A+++. 사용자 사전 (2026-11-23, 1차)
- `%LOCALAPPDATA%\multilang-ime\user_dict.json` (`[{"keys","text"}]`), 1초 안에 다시 읽음 (`backend/src/userdict.rs`).
- 키 전체가 등록된 것과 같으면(대소문자 무시) 후보창 맨 앞 "사전" 묶음, 첫 Shift 탭이 고름. 자동 표시는 바꾸지 않음. 판정 학습에는 넣지 않음. 일본어 글자가 있는 변환은 일본어 문맥으로 친다.
- 설정 화면 "사용자 사전" 탭 (친 키 칸은 IME 끔), `MultilangIME-Settings.exe --dict`로 바로 열기.
- 다음: 후보창에서 바로 등록(지금 조합을 사전에 넣기), 자동 등록(자주 고친 변환 제안), 부분 일치·접두어, 가져오기/내보내기, Mozc 사용자 사전 연동.

## A++++. 일본어 가나 후보 (2026-11-23)
- 후보창 일본어 묶음 맨 앞에 ひらがな / カタカナ / 半角カナ (입력 전체 읽기). 자동 변환(조합 중 표시)은 Mozc 1위 그대로.
- 첫 Shift 탭: 일본어 묶음이 1위이면 맨 앞(가나)부터, 지금 보이는 것과 같은 후보는 건너뜀 (橋 → はし, はし → ハシ).
- 한 문절이고 Mozc 후보에 같은 글자가 있으면 그 id로 확정해 Mozc가 학습, 아니면 Mozc를 거치지 않고 그대로 확정.

## A5. 작업 표시줄 검색창 후보창: UIAccess 대행 프로세스 (2026-11-23, 서명 시험 대기)
- `candhost/` → `MultilangIME-CandWin.exe` (매니페스트 uiAccess=true). `tip/src/candwin.rs`를 그대로 컴파일해 같은 모양.
- TIP는 셸 프로세스(SearchHost, SearchApp, StartMenuExperienceHost, ShellExperienceHost)에서 대행 창이 있으면 WM_COPYDATA(JSON show/hide)로 맡기고, 없거나 실패하면 직접 그린다 (`tip/src/host.rs`). 대행 창은 ChangeWindowMessageFilterEx로 낮은 무결성(AppContainer)의 메시지를 받는다.
- 백엔드가 ShellExecute로 띄운다 (Program Files 설치본 먼저, 30초마다 확인, 실패 시 간격 늘림). 서명·보안 위치가 아니면 거부(5)되고 로그만 남는다.
- 확인함: UIAccess 없는 시험 빌드(`CANDHOST_NO_UIACCESS=1`)로 메시지·그리기·쪽 표시·숨기기·한 개만 실행.
- 남은 것: `tools\uiaccess-dev-setup.ps1`(사용자가 관리자로 실행: 자체 서명 인증서 신뢰 + 서명 + Program Files 복사) 뒤 검색창에서 실제로 위에 뜨는지 확인. 배포판은 정식 서명 필요.

## B. 품질 (중간 크기)

0. **영단어 + 일본어 조사 패턴** (사용자 제안) — **적용함**
   - 판정 (51fe375): 흔한 영단어(zipf≥3) + 조사로 시작해 가나로 남김없이 풀리는 나머지면 영어+일본어 분할 허용. 패턴 세트 26→36/43 (`cargo run --release -p engine --example en_particle`), 공개 데이터 5종 토큰 오류율 변화 없음. 가산점을 더 주면 38/43까지 늘지만 일본어 문절 오류율이 1.72→1.75~2.03%로 나빠져 넣지 않음. 남은 실패: `mailwookutta`, `filega`, `loginga`, `downloadshita`.
   - 변환 (9b794e0): 영단어 뒤 일본어 구간은 これ를 붙여 Mozc 변환 후 떼어 냄. `Youtube|二動画を上げた`→`に動画を上げた`, `server|二アクセス`→`にアクセスできない`. 20개 사례에서 나빠진 것 없음 (それ도 같음, なにか/もの는 떼어 내기 실패가 많음). 메모장에서 확인.
   - 아래는 적용 전 기록.
   - 잘 되는 것 (짧은 형태): `APIwo`→API|を, `PCga`→PC|が, `gitde`, `zoomde`, `pythonno`, `Discordni`, `updatewo`, `serverga`, `mailwo`, `chromede` 모두 영어+일본어로 나뉜다. `APIwotsukatte`→API|を使って, `PCgakowareta`→PC|が壊れた, `Gitdekanrisuru`→Git|で管理する도 정상.
   - **판정 실패 (조사 뒤에 일본어가 길게 이어질 때, 소문자 영단어)**: `pythondekaita`, `serverniakusesudekinai`, `mailwookutta`, `chromegaosoi`, `updatewoshitekudasai`는 토큰 전체가 영어로, `zoomdekaiginisankashita`는 전체가 일본어(ぞおmで…)로 판정된다.
     - 대책: 토큰 앞부분이 흔한 영단어(사전 zipf 기준)이고 바로 뒤가 일본어 조사(を が で に の は と も へ から まで より)로 시작하며 나머지가 가나로 남김없이 풀리면, 영어+일본어 분할에 가산점. 한국어의 "영단어+조사(API를)" 처리와 같은 방식.
     - 평가: 공개 데이터에 이 패턴이 적으니, IT·게임 문장으로 작은 평가 세트를 만들어 전후 비교.
   - **변환 품질 (조사로 시작하는 일본어 구간)**: Mozc가 `にどうがをあげた`를 조사 없이 혼자 변환해 `Youtube|二動画の上げた`가 됐다.
     - 대책: 영어 구간 뒤의 일본어 구간은 앞에 자리표시 명사 읽기(예: これ)를 붙여 Mozc에 넣고, 결과 첫 문절에서 그 부분을 떼어 쓴다 (これに|動画を|上げた → に動画を上げた). 확정(학습) 때도 같은 방식.
     - 확인: 자리표시 단어 후보 몇 개(これ, それ, なにか)를 바꿔 가며 오변환률 비교.

5. **메모장 등에서 일본어 글자 깨짐** — 2026-11-23 진행 기록:
   - 프로필 전환(`ITfInputProcessorProfileMgr::ActivateProfile`, flags 0)은 E_INVALIDARG로 실패. 입력기 표시가 바뀌는 부작용도 있어 사용자 요청으로 보류하고 코드는 되돌림 (다시 하면 TF_IPPMF_FORPROCESS/FORSESSION 플래그부터 시험).
   - 우회: 글꼴. 설치 글꼴 확인 결과 (한 気 ー あ 学): 맑은 고딕 O x x O x, Noto Sans KR O O O O O, Yu Gothic·Meiryo·BIZ UD·Noto Sans JP는 한글 없음. 메모장 글꼴을 Noto Sans KR(또는 Noto Sans CJK KR, 본고딕)로 하면 한국어 프로필 그대로 일본어도 그려질 것으로 예상 — 메모장 실측: 사용자가 메모장 글꼴을 Noto Sans KR로 바꾸니 잘 표시됨 (2026-11-23). → 설치 안내에 "한·일 글자가 모두 있는 글꼴(Noto Sans KR, 본고딕 등)" 권장을 넣는다.
   - 가짜 입력 언어 알림 실험 (2026-11-23): 일본어를 넣기 직전 RichEdit 창(RichEditD2DPT)에 WM_INPUTLANGCHANGE(SHIFTJIS_CHARSET, HKL 0x04110411)를 보냈지만 효과 없음 — 메모장은 알림이 아니라 실제 키보드 언어를 본다. 코드 되돌림. 남은 우회는 API 가로채기(권하지 않음)뿐이라 글꼴 안내로 확정.
   - 원인: 앱이 입력 중인 프로필의 언어로 글꼴을 고른다. 우리 TIP는 한국어 프로필이라 気·ー 등이 맑은 고딕으로 그려져 깨진다. `GUID_PROP_LANGID`는 무시된다.
   - (시도했으나 실패, 위 기록) 일본어 구간에서 같은 TIP의 일본어 프로필로 전환.
   - (채택 안 함) 기본 프로필을 일본어로 (한국어는 조합 중에만 네모, 확정 후 정상).
6. **(완료, 설정 옵션) 일본어 긴 단위 변환** — `settings.json`의 `"ja_space": "continue"`. 기본값은 기존 방식(확정, 이제 + 전각 공백). README "설정 파일과 일본어 Space".
   - 문장 통째 변환이 문절별보다 낫다 (뉴스 글자 오류율 17.5→16.5%, Tatoeba 9.9→7.4%). 앞 문맥 넘기기는 효과 없음.
   - 일본어 구간을 구두점·Enter까지 이어 가는 방식은 Space=확정 원칙과 충돌 → 사용자 결정으로 기본은 확정, `continue`는 선택 옵션.
7. **(완료, 부분 변환) 긴 토큰 후보 품질**: 2위 이하 후보가 토큰 전체 단위라 질이 낮다. 구간(문절) 단위 후보 고르기.
8. **(완료, 부분 변환 ←/→) 여러 문절 일본어 토큰**: 첫 문절 말고 다른 문절의 후보 고르기 (←→로 문절 이동 등, 1번 방향키와 키 배정 조율).

## C. 배포 준비

9. **(완료) 라이선스**: GPL-3.0-or-later. `LICENSE`(원문), `THIRD-PARTY-NOTICES.md`(Mozc BSD-3, VC 런타임, libhangul 한자 사전 BSD-3, wordfreq 데이터 CC BY-SA 4.0, Rust 크레이트 MIT/Apache). 기획서의 "라이선스 미정"은 아직 갱신 안 함.
10. **(완료, `docs/mozc-helper.md`) Mozc 배포 방식**: 직접 빌드한 표준 입출력 도우미 `mozc_stdio.exe`를 백엔드 자식으로 띄움 (공식 서버와 같은 변환·지연, 시작 약 340 ms, 학습은 `%LOCALAPPDATA%\multilang-ime\mozc`에 따로). 릴리스에는 도우미만 넣는다. 실사용 확인 남음. 이전 메모: 지금은 공식 MSI에서 꺼낸 mozc_server를 쓴다. Meltype처럼 Mozc를 직접 빌드한 도우미(stdin/stdout)로 바꾸면 실행 문제(가장 토큰, 패키지 앱 문맥)와 브랜딩이 풀린다. Bazel 빌드 30~40분.
11. **(부분 완료) 설치 프로그램** — 절차는 `docs/release.md`.
    - 완료: zip 패키지(`installeruild-release.ps1`), `install.ps1`/`uninstall.ps1` — Program Files 복사, 64·32비트 등록, 앱 컨테이너 읽기 권한, UI-less 카테고리, Users 그룹 로그온 작업, 한국어 입력 목록 추가. 시험 실행(-DryRun)만 했고 실제 설치는 아직.
    - 남은 것: 실제 설치 시험, 코드 서명, 작업 표시줄 검색창용 UIAccess 후보창 프로세스(서명·Program Files 설치가 조건), .exe(Inno Setup)/MSI 포장 여부 결정(시작 메뉴·프로그램 추가/제거 등록).
12. **(완료) 설정 화면 최소판**: `settings\Settings.cs` (WinForms, .NET Framework 4.x 기본 csc, `settingsuild.ps1`) → `MultilangIME-Settings.exe`, 설치 때 시작 메뉴 바로 가기.
    - 사용할 언어(`languages.ko/ja`, 영어는 항상): 백엔드가 요청마다 판정기 언어 목록에 반영 (조합 중에는 안 바꿈).
    - 자동 꺼짐 프로그램(`disabled_apps`): TIP가 새 연결마다 `Hello{app}`으로 실행 파일 이름을 알리고, 백엔드가 Test/Key를 먹지 않음.
    - 학습 기록 지우기: 파이프 `ClearLearning` → history.json, hanja_learn.json, Mozc 사용자 학습. 백엔드가 없으면 파일만 지움.
    - 일본어 Space(`ja_space`).
    - 실제 앱에서 확인할 것: 자동 꺼짐 목록, 언어 끄기, 학습 지우기 (개발판 deployd에 배포함, 앱은 다시 열어야 새 TIP).
13. **(완료, 입력칸 부분) 자동 꺼짐**: 비밀번호·숫자·이메일·URL 칸, 꺼진 문맥 (README "자동 꺼짐"). 남은 것: 관리자 앱, 게임, 앱별 목록(설정 화면과 함께).

## D. 나중에

14. **Meltype 참조 브랜치**: Meltype의 영어 판정 자원·Mozc 도우미·Zenzai를 참조한 브랜치를 만들어 판정 정확도·변환 품질·지연을 비교.
15. **Zenzai(로컬 모델) 선택 기능**: 품질 차이가 클 때만, 기본 끔.
16. **중국어 (v3)**.

## 참고 문서
- 설계: `docs/v1-tsf-frontend.md`
- 실사용 문제·Meltype·GPL 검토: `docs/meltype-review.md`
- 측정 결과: `README.md`
