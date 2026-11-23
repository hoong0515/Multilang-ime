# multilang-ime (v0: 한·영·일 언어 판정 엔진 프로토타입)

전환 키 없이 한국어·영어(이후 일본어·중국어)를 섞어 쓰는 IME의 판정 엔진 실험.
일본어 IME의 변환 방식(조합 → 1위 후보 자동 선택 → 후보 순환 → 확정 시 학습)을 다국어로 확장하고,
그 위에 언어 자동 판정을 얹는 구조다. 기획서: https://claude.ai/artifact/KC3BDnzMf6JEacSz6gG9yK

## 구성

| 파일 | 내용 |
| --- | --- |
| `ime_v0/hangul.py` | 두벌식 오토마타 (키 입력 ↔ 한글) |
| `ime_v0/romaji.py` | IME식 로마자 ↔ 히라가나 |
| `ime_v0/ngram.py` | 키 입력 문자 4-gram (보간 절대 할인) |
| `ime_v0/models.py` | wordfreq로 영어·한국어·일본어(fugashi 읽기) 모델과 빈도 사전 생성, `data/models.pkl`에 캐시 |
| `ime_v0/detector.py` | 토큰 후보(언어별 단일 + 토큰 안 언어 래티스) 점수, 문장 단위 비터비, 모호 판정, 사용자 선택 학습 |
| `ime_v0/cli.py` | 대화형 데모 |
| `eval/extract_tweets.py` | X 아카이브에서 한·영, 일본어 평가 데이터 추출 |
| `eval/evaluate.py` | 오판률 측정, 파라미터 탐색, 학습 시뮬레이션 |

## 실행

```powershell
python -m pip install wordfreq fugashi unidic-lite pytest
python -m pytest -q
python -m ime_v0.cli --params data/private/best_params_koja.json --langs en,ko,ja
python -I eval/extract_tweets.py "<아카이브>\data\tweets.js" data/private
python eval/evaluate.py data/private/tweets_koen.jsonl --langs en,ko --params data/private/best_params.json
python eval/evaluate.py data/private/tweets_koen.jsonl data/private/tweets_ja.jsonl --langs en,ko,ja --tune 120 --save data/private/best_params_koja.json
```

`data/private/`는 개인 데이터라 git에서 제외한다.

## v0 결과 (2026-11-22, 본인 트윗 20,952개 / 토큰 116,348개 test)

| 설정 | 토큰 오판률 | 오류 있는 트윗 |
| --- | --- | --- |
| 첫 버전 | 3.40% | 16.6% |
| 튜닝 후, 학습 없음 | 1.47% | 7.4% |
| 튜닝 후 + 선택 학습 | 0.73% | 3.6% |

- 학습 없음 기준 오류 1,711개 중 1,433개(84%)는 정답이 2위 후보, 134개(8%)는 3위 후보.
- 판정 비용은 토큰당 약 0.1 ms (Python).
- 남은 오류: 짧은 낱자모(`ㅎㅎ`=gg, `ㄳ`=rt, `ㅁㄴㅇㄹ`=asdf), 영어 뒤 조사 분할, 한 토큰 안의 3단 혼용(`ㅋwㅋ`).

## 일본어 추가 (한·영·일, 2026-11-22)

| 토큰 종류 | 수 | 한·영만 | 한·영·일 | 한·영·일 + 학습 |
| --- | --- | --- | --- | --- |
| 한국어 | 111,844 | 1.32% | 1.24% | 0.52% |
| 일본어 | 7,134 | - | 4.11% | 3.34% |
| 영어 | 3,828 | 1.83% | 4.83% | 3.61% |
| 영어+일본어 | 529 | - | 34.4% | 34.0% |
| 일본어+영어+일본어 | 285 | - | 33.0% | 32.6% |
| 전체 | 124,881 | - | 1.96% | 1.22% |

- 일본어는 띄어쓰기가 없어 한 토큰에 여러 언어가 섞이므로 토큰 안 언어 래티스(동적 계획법)로 구간을 나눈다.
  토큰 중간의 영어는 대문자로 시작할 때만 허용해 토큰당 0.19 ms.
- 일본어를 켜면 영어 오판이 약 2.6배. 일본어 문장 속 영어가 가장 어렵다.
- v0는 가나까지만 만든다. 한자 변환은 Mozc 연동 단계.

## 공개 데이터 재평가 (2026-11-22)

`python eval/fetch_public.py && python eval/build_public.py && python eval/report_public.py --tune 80`
(공개 dev로 튜닝, 출처별 test. 결과는 `data/public/report.json`)

| 평가셋 | 토큰 | 전체 오판 | 문장 속 영어 | 토큰 안 혼용 | 학습 후 |
| --- | --- | --- | --- | --- | --- |
| 한국어 뉴스·위키·웹 (Leipzig) | 15.2만 | 0.53~0.56% | 3.8~11.5% | 5.8~6.4% | 0.28~0.34% |
| 한국어 영화 리뷰 (NSMC) | 25,829 | 1.34% | 2.3% | 21.8% | 0.86% |
| 한국어 회화 (Tatoeba) | 13,911 | 1.79% | - | - | 0.50% |
| 영어 뉴스 (Leipzig) | 63,811 | 0.16% | - | - | 0.15% |
| 일본어 뉴스·위키, 문장 단위 | 28,108 | 5.7~6.1% | 5.5~11.9% | 26~33% | 4.8~4.9% |
| 일본어 뉴스·위키, 문절 단위 | 93,171 | 6.4~7.0% | 7.6~12.8% | 9.5~12.1% | 2.7~3.0% |
| 일본어 회화 (Tatoeba), 문절 단위 | 17,406 | 10.8% | - | - | 4.5% |

- 트윗으로 튜닝한 파라미터와 공개 데이터로 튜닝한 파라미터의 차이가 작다 (과적합 아님).
- 한국어 문장 속 영어 오류는 대부분 대문자 약어(ESG, NFT, UAM)와 1~3글자.
- 문절 단위 입력은 혼용이 쉬워지는 대신 짧은 일본어(`ni`, `go`, `kai`)가 어려워지고, 선택 학습 효과가 가장 크다.

## 약점 개선 (v3, 2026-11-23)

대문자 약어 우선, 대소문자 구분 학습, 짧은 토큰의 언어 전환 벌점, 일본어 단어+조사 사전, 언어별 토큰 중간 영어 허용 규칙.
공개 dev로 재튜닝 (`data/public/best_params_public_v3.json`).

| 평가셋 | 해당 언어 오판, 전 → 후 | 토큰 안 혼용, 전 → 후 |
| --- | --- | --- |
| 한국어 뉴스·위키·웹 | 0.40~0.46% → 0.13~0.26% | 5.8~6.4% → 2.4~4.4% |
| 한국어 영화 리뷰 / 회화 | 1.24% / 1.79% → 1.00% / 0.67% | 21.8% → 18.5% |
| 영어 뉴스 | 0.16% → 0.33% | - |
| 일본어 문장 단위 | 3.6~4.3% → 0.5~1.2% | 26~33% → 21~31% |
| 일본어 문절 단위 | 6.2~10.8% → 0.4~0.6% | 9.5~12% → 12~24% |

남은 과제: 한 토큰 안의 일본어+영어 혼용, 한국어 구어체의 영어+한국어 혼용.

## 혼용 토큰 개선 (v4, 2026-11-23)

토큰 안 래티스가 위치·언어마다 상위 3개 경로를 남기고(후보 다양화), 소문자 영어 + 일본어 조사(`ill|と`),
소문자 영어 + 긴 한국어 어미(`vga|카드로는`)를 허용, 약어 뒤 소문자 꼬리 벌점, 영어 뒤 조사로 시작하는 일본어 보너스.
공개 dev로 재튜닝 (`data/public/best_params_public_v4.json`).

| 평가셋 | 해당 언어 오판 v3 → v4 | 토큰 안 혼용 v3 → v4 |
| --- | --- | --- |
| 한국어 뉴스·위키·웹 | 0.13~0.26% → 0.02~0.04% | 2.4~4.4% → 4.7~9.9% (악화) |
| 한국어 영화 리뷰 | 1.00% → 0.47% | 18.5% → 22.7% (악화) |
| 일본어 문장 단위 | 0.3~1.2% → 0.4~1.4% | 15~30% → 15~30% |
| 일본어 문절 단위 | 0.4~0.6% → 0.9~1.0% | 12~24% → 8% |
| 본인 트윗 한·영 / 일본어 | 1.13% / 1.00% → 0.46% / 0.86% | 12.0% / 29.7% → 14.8% / 24.9% |

한국어 단독과 일본어 문절 단위 혼용은 좋아졌고, 한국어 쪽 혼용 토큰은 나빠졌다.
다음 후보: 언어 쌍별 분할 벌점 분리, 혼용 토큰 비중을 높인 튜닝 목표.

## M1 뼈대 TIP (Rust, `tip/`)

`cargo build --release` (64비트), `--target i686-pc-windows-msvc` (32비트).
알파벳 키를 그대로 조합에 넣고 Backspace/Space/Enter/Esc 처리. 언어 판정은 M2에서 연결.
`python tip/tests/com_smoke.py`: 등록 없이 DLL을 불러 클래스 팩토리·인터페이스 확인.
등록: 관리자 명령 프롬프트에서 `regsvr32 target
elease\multilang_tip.dll` (해제는 `/u`).

## M2 백엔드 + 한·영 판정 연결 (2026-11-23)

Rust 워크스페이스: `engine/`(판정 엔진), `backend/`(파이프 서버), `ipc/`(메시지), `tip/`(TSF 텍스트 서비스).

```powershell
python -m ime_v0.export_model            # data/engine_model.json (Rust 엔진용 모델·파라미터)
cargo test -p engine -p backend
python eval/dump_parity.py; cargo run --release -p engine --example parity   # Python과 결과 비교
cargo build --release                     # target/release: multilang_tip.dll, backend.exe
```

- 엔진: 두벌식 오토마타, n-gram, 판정기(한·영)를 Rust로 이식. 공개 데이터 1,200문장에서 Python v0과 분할·변환·상위 3후보가 모두 일치. 토큰당 0.023 ms, 모델 로딩 59 ms.
- 백엔드: 사용자당 하나, 연결(TIP 인스턴스)마다 조합 상태. JSON 메시지(test / key / flush / reset). 키 왕복 중앙값 0.14 ms.
- TIP: 키를 백엔드에 보내 조합·확정을 반영. 백엔드가 없으면 DLL 옆 backend.exe를 띄우고, 200 ms 안에 응답이 없으면 키를 먹지 않는다.
- 배포: `deployd`에 `multilang_tip.dll`, `backend.exe`, `engine_model.json`. 이미 DLL을 불러온 프로세스는 이전 버전을 계속 쓰므로, 시험하는 앱은 다시 시작해야 한다.
- 메모장 실측: 전환 키 없이 `dkssudgktpdy hello APIfmf.` → `안녕하세요 hello API를.`, `gksrmf test` → `한글 test`.
  백엔드를 끈 상태에서도 TIP가 자동으로 띄움.
- 백엔드 시작: `powershell -ExecutionPolicy Bypass -File toolsackend-task.ps1 install` (사용자 로그온 작업, 관리자 권한 불필요, 해제는 `uninstall`).
  패키지 앱(메모장 등)이 띄운 백엔드는 그 앱의 패키지 문맥을 물려받기 때문에(가상 폴더, 앱과 수명 공유) 작업 스케줄러로 띄운다. TIP의 직접 실행은 비상용.
- 구간별 밑줄: 표시 속성 제공자(일반 실선 / 모호 점선 / 후보 선택 중 굵은 실선). 메모장에서 조합 중 실선 표시와 확정 뒤 밑줄 제거를 확인.
  모호 구간 점선은 백엔드가 모호 표시(`go`, `so`, `ah` 등)를 보내는 것까지 확인했고, 화면 해상도 문제로 점선 모양 자체는 눈으로 확인하지 못했다.

## M3 후보창 (2026-11-23)

- 직전 단어 언어 가중치: 같은 언어로 이어 확정한 단어 수만큼 전환 벌점을 키운다 (`MOMENTUM = 0.3`).
  IME처럼 왼쪽 문맥만 쓰면 문장 전체 판정보다 오판이 1.2~3배 많았고(`eval/online_context.py`), 가중치로 한국어 오판을 약 10% 줄임.
- 후보창: 왼쪽 Shift 탭으로 열기(2위 선택)·다음 후보, 숫자 1~5 선택, Space·Enter 확정, Backspace·Esc로 닫기, 열린 채 글자를 치면 선택 후보로 확정 후 새 조합.
  조합 바로 아래에 포커스를 가져가지 않는 팝업으로 그린다 (`tip/src/candwin.rs`). 후보마다 언어 구성(en, ko, en+ko)을 함께 표시.
- 학습: 확정한 형태를 모든 앱이 함께 쓰는 기록에 쌓고 `%LOCALAPPDATA%\multilang-ime\history.json`에 저장.
- 메모장 실측: `go` → Shift 탭 → `1 go / 2 해 / 3 gㅐ` 후보창, Shift로 이동, Backspace로 닫기, 숫자 선택 후 Space 확정, 구두점 앞 확정 모두 확인.
- 진단: `deployd`에 `MULTILANG_TIP_DEBUG` 파일을 두면 TIP가 키 이벤트를 `tip-debug.log`에 남긴다.

## Mozc 연동 개념 검증 (2026-11-23)

공식 GitHub Actions의 Windows 빌드(`Mozc64_x64.msi`, 3.35.6354.100)를 설치하지 않고 풀어서 mozc_server만 띄우고,
`poc/mozc/mozc_client.py`가 이름 있는 파이프로 직접 대화한다.

```powershell
python -m pip install protobuf grpcio-tools
python -m grpc_tools.protoc -I third_party/mozc/src --python_out=poc/mozc/gen third_party/mozc/src/protocol/commands.proto third_party/mozc/src/protocol/config.proto third_party/mozc/src/protocol/candidate_window.proto third_party/mozc/src/protocol/engine_builder.proto third_party/mozc/src/ipc/ipc.proto
python poc/mozc/run_poc.py
```

- 가나 통째로 넣기(`UPDATE_COMPOSITION`), 로마자 키 입력, 구간 이동·경계 조정, 선택 학습, 변환 전 예측 후보 모두 동작.
- 지연(중앙값): 키 1개 0.8 ms, 변환 1.1 ms, 조합 통째 교체 14.9 ms.
- 서버는 스레드 토큰이 없으면 실행을 거부하므로, 일시 정지 상태로 띄워 가장 토큰을 붙인 뒤 재개한다.
- 학습 데이터는 `%USERPROFILE%\AppData\LocalLow\Mozc`에 쌓인다 (검증 중 확정한 결과도 학습됨).

## M4 일본어 (2026-11-23)

- 엔진: 일본어 판정(가나 읽기 모델, 문절 끝 규칙)을 Rust로 이식. Python v0와 한·영·일 1,500문장 모두 일치 (`cargo run --release -p engine --example parity`).
- 영어 단어 쪼개짐 수정: 흔한 영어 단어(zipf ≥ 3.0, `EN_WORD_KEEP`)는 토큰 안에서 다른 언어로 쪼개지 않는다.
  영어 뉴스에서 다른 언어 끼어듦 1,000단어당 6.1 → 3.4.
- Mozc Rust 클라이언트 (`mozc/`, prost + 동봉 protoc; Mozc proto는 BSD-3, `mozc/proto/LICENSE-mozc`). 변환 중앙값 14.5 ms.
- 백엔드:
  - 일본어 구간은 가나가 완성되면 Mozc 변환 결과를 라이브 표시. 가나별 캐시, Mozc가 없거나 실패하면 가나로 표시하고 30초 뒤 재시도.
  - Shift 탭: 일본어 토큰이면 Mozc 후보(첫 문절) 먼저, 그 뒤에 다른 언어 후보, 영어 후보가 있으면 끝에 전각(`ｇｏ`).
  - 확정: 일본어 구간은 Mozc에서 변환·선택·`SUBMIT` (Mozc 학습). 일본어로 끝나면 Space에도 공백을 넣지 않는다.
  - 일본어 문맥 기호: `. , [ ] / ? !` → `。、「」・？！`, 글자 뒤 `-` → `ー`.
- 배포: 공식 빌드의 `mozc_server.exe`와 VC 런타임을 `deployd\mozc\`에 둔다 (백엔드가 자동 실행).
- 메모장 실측: `kyouhaiitennkidesu.` → `今日はいい天気です。`, `ko-hi-kisha` → `コーヒー記者`, Shift 탭으로 `珈琲記者`, `☕記者`, `ｋｏ－ｈｉ－ｋｉｓｈａ` 등 후보.
- 주의: Claude 데스크톱 앱 같은 패키지 앱에서 띄운 mozc_server는 그 앱의 가상 폴더를 써서, 작업 스케줄러 백엔드가 접속하지 못한다. 시험 중 띄운 mozc_server는 끄고 백엔드가 직접 띄우게 한다.
- 남은 일: 키당 15 ms 줄이기, 긴 토큰의 구간 단위 후보, 여러 문절 중 다른 문절 고르기.

## M5 안정화 1: 일본어 지연·글자 깨짐 (2026-11-23)

- 지연 원인: Mozc 호출 단계별로 재 보면 (`cargo run --release -p mozc --example mozc_timing`) 연결 0.05 ms, 변환(Space) 0.9 ms인데
  가나를 통째로 넣는 `UPDATE_COMPOSITION`이 14 ms였다.
- 수정: Mozc 조합을 우리 로마자 키와 맞춰 두고 **새로 친 키만** `SEND_KEY`로 보낸 뒤 변환 → Esc로 조합 상태 복귀
  (키 0.5 + 변환 0.5 + 취소 0.5 ms). 앞부분이 바뀌면 지우고 다시 보내고, Mozc의 가나와 우리 가나가 다르면 통째로 넣는다(느린 길, 로그에 남김).
  Mozc는 끝의 n을 전각 `ｎ`으로 둔다는 점을 처리.
- 결과 (일본어 16단어 145키, 파이프 왕복 포함): 키당 중앙값 0.45 ms, p95 2.2 ms, 최대 3.0 ms (이전 약 15 ms).
- 글자 깨짐: 메모장에서 `気`, `ー`가 네모로 보였다. 들어간 글자는 정상(U+6C17, U+30FC)이다.
  - MS 일본어 IME로 친 같은 문장은 일본어 글꼴로 제대로 보인다. 메모장(RichEdit)은 **글을 넣을 때 켜져 있는 입력기 프로필의 언어**로 글꼴을 고른다.
  - 우리 TIP는 한국어 프로필이라 일본어도 한국어 글꼴(맑은 고딕)로 그려지고, 이 글꼴에 없는 일본 전용 글자가 깨진다.
  - 우리 TIP의 일본어 프로필로 시험하면 반대가 된다: 일본어는 정상, 한국어는 **조합 중에만** 네모로 보이고 확정하면 정상.
  - TIP가 구간마다 붙인 언어 속성(`GUID_PROP_LANGID`)은 설정에 성공하지만 메모장은 무시한다.
  - 해결 후보: 일본어 구간을 넣을 때 TIP가 자기 일본어 프로필로 바꾸기(같은 TIP 안 프로필 전환), 또는 기본 프로필을 일본어로 두기.
- 앞 문맥 (2026-11-23):
  - Mozc 요청의 `context.preceding_text`는 이 Mozc 서버에서 변환 결과를 바꾸지 않았다. 같은 세션에서 앞 단어를 SUBMIT한 이력도 마찬가지.
  - 앞 문절 읽기를 붙여 한 번에 변환하고 지금 문절 부분만 떼어 쓰는 방식 (`mozc/examples/mozc_context_eval.rs`, 문절 200문장, 기호 뺀 글자 오류율):

    | 데이터 | 문절만 | 앞 문절 1~4개 붙임 | 문장 통째 |
    | --- | --- | --- | --- |
    | 뉴스 | 17.53% | 17.72~17.75% | 16.47% |
    | Tatoeba | 9.95% | 9.52~9.55% | 7.37% |

    앞 문절을 붙이는 것은 효과가 거의 없다 (Mozc가 문절 경계를 다르게 잡아 떼어 낼 수 있는 경우가 약 65%). 효과가 나는 것은 문장을 통째로 변환할 때다. 적용하지 않았다.
  - `kinou`→機能, `amegafurusoudesu`→雨が古そうです는 문장 통째로 넣어도 같다. 공개 Mozc 자체의 판단이다.
  - 시험으로 확정한 문장이 Mozc에 학습돼 결과를 오염시켜서 Mozc 사용자 학습을 지웠다 (`Session::clear_learning`). 시험 확정은 학습을 남긴다는 점에 주의.

## M5 안정화 2: 방향키·영단어+조사·한자 (2026-11-23)

- 후보창이 열려 있으면 방향키로 후보 이동 (↓→ 다음, ↑← 이전).
- Mozc 서버는 백엔드 시작 때 백그라운드로 미리 띄운다 (키 처리 중 실행 금지, 시작 직후에도 키당 최대 4 ms). 50 ms 넘는 요청은 `slow request`로 로그.
- 소문자 영단어 + 일본어 조사 (`mailwo`, `zoomdekaiginisankashita`): 흔한 영단어 뒤가 조사로 시작해 가나로 풀리면 영어+일본어로 나눈다. 패턴 세트 26→36/43, 공개 데이터 변화 없음.
- 영단어 뒤 일본어 구간은 Mozc에 `これ`를 붙여 변환하고 떼어 낸다 (`Youtubeに動画を上げた`, 이전엔 `二動画`).
- 한국어 한 단어의 후보창에 한자 후보 (`학교`→學校). 사전: libhangul `hanja.txt` (BSD-3-Clause). 고른 한자는 `%LOCALAPPDATA%\multilang-ime\hanja_learn.json`에 학습.
- 배포 주의: TIP는 백엔드가 없으면 바로 다시 띄우므로, `backend.exe`는 실행 중 파일 이름을 바꾸고 새 파일을 넣는 식으로 교체한다. 시험용 예제로 띄운 mozc_server(Claude 앱 문맥)가 남아 있으면 작업 스케줄러 백엔드가 Mozc에 붙지 못한다.

## M5 안정화 3: 언어 묶음 후보·Tab·기호 변환 (2026-11-23)

- 후보창은 문맥과 상관없이 언어마다 묶음을 만든다 (1위 언어 묶음 먼저): 영어 = 그대로 + 전각, 한국어 = 한글 + 한자, 일본어 = Mozc 후보. 그 뒤 섞인 형태(최대 3), 깨진 한글(최대 2). 최대 16개.
  - 예: `go` → go, ｇｏ | 해, 海, 害… | 五, 御, ご, 後, 語, 碁. 이전에는 한국어·영어 문맥에서 일본어 변환 후보가 안 나왔다.
  - Shift 탭으로 처음 열면: 1위가 일본어면 다음 변환 후보, 그 밖에는 다음 언어 묶음의 첫 후보.
- Tab / Shift+Tab: 다음/이전 언어 묶음의 첫 후보로.
- 기호·숫자도 조합에 넣는다 (일본어 IME처럼). Shift 탭으로 변형 후보: `.`→。．…・, `?`→？⁇⁉, `->`→→⇒, `3`→３③三, `[`→「『【 등. 일본어 문맥이면 。、「」 등이 기본.
  - 글자를 치면 기호를 확정하고 새 조합, Space는 확정+공백(일본어 문맥이면 공백 없음), Enter는 확정만.
  - 주의: 숫자도 조합되므로 표 계산 앱에서 숫자 뒤 Enter는 확정만 하고 셀을 옮기지 않는다 (일본어 IME와 같은 동작). 필요하면 자동 꺼짐 앱 목록으로.
- 백엔드를 다시 띄운 직후 첫 Shift 탭이 한 번 무시된 일이 있었다 (TIP 재연결 직후). "시간 초과 때 글자 유지"(A-2)와 함께 볼 것.

## M5 안정화 4: 백엔드 응답 지연·재시작 때 글자 유지 (2026-11-23)

- 문제: TIP는 백엔드 응답을 200 ms만 기다리고, 실패하면 키를 앱에 그냥 넘겼다. 백엔드는 연결마다 조합 상태를 새로 만들어서, 재연결(시간 초과 뒤, 백엔드 재시작 뒤)하면 조합 중이던 키를 잃었다. 재시작 직후 첫 Shift 탭이 무시된 것도 같은 원인.
- 수정 (`tip/src/lib.rs`, `tip/src/client.rs`):
  - TIP가 지금 조합을 만든 키를 기록한다 (후보창 안의 이동·선택은 빼고).
  - 연결 세대가 바뀌면(새 연결) 먼저 Reset + 기록한 키를 다시 보내 백엔드 조합을 되살린다.
  - 연결이 바로 끊긴 경우(백엔드 재시작)는 한 번 다시 연결해 재시도한다.
  - 응답이 없으면 글자·기호는 친 그대로 조합에 붙여 두고 기록한다. 다음 응답 때 되살린다. Space는 보이는 그대로 확정, 그 밖의 키는 보이는 그대로 확정하고 앱에 넘긴다.
  - 실패 직후 0.5초는 백엔드를 부르지 않는다 (키마다 200 ms 기다려 앱이 버벅이지 않게). Space·Enter·Esc·Backspace는 예외.
  - 시험용: `deployd\BACKEND_DELAY_MS` 파일(숫자)이 있으면 백엔드가 글자 키 응답을 그만큼 늦춘다.
- 메모장 실측:
  - 조합 중 백엔드 재시작: `dks`(안) → 재시작 → `sud` → `안녕`. 바로 뒤 Shift 탭 정상.
  - 400 ms 지연: `gksrnr`이 로마자로 조합에 남고, 지연을 끈 뒤 Space에서 `한국`으로 확정.
  - 0.5초 쉬기는 앱에서 확인하지 않았다 (메모장 재시작 필요).

## M5 안정화 5: 부분 변환 (2026-11-23)

- 여러 부분으로 나뉘는 입력에서 Shift 탭을 누르면 부분 변환이 시작된다. 언어가 바뀌는 곳에서 나누고, 일본어는 Mozc 문절마다 더 나눈다.
  - `kyouhaiitennkidesu` → [今日は] | いい天気です
  - `Youtubenidougawoageta` → [Youtube] | に | 動画を | 上げた
  - `APIfmf` → [API] | 를
- 키: ←/→ 부분 이동, ↓/↑·Shift 탭 그 부분의 후보, Tab 언어 묶음, 숫자 고르기, Space/Enter 전체 확정, Esc 부분 변환 닫기, Backspace 친 키 그대로.
- 부분마다 후보는 그 부분의 키만으로 다시 판정한 언어 묶음이다 (Youtube 부분에 ヨウツベ, ようつべ 등). 지금 보이던 결과가 첫 후보.
- 일본어 부분은 확정할 때 부분마다 Mozc로 학습시킨다. 한 부분짜리 입력은 이전처럼 후보창 하나 (←/→는 후보 이동).
- 로마자를 문절 읽기에 맞춰 나누는 `split_by_readings` (nn·っ를 앞 문절이 다 먹도록 가장 긴 앞부분).
- 주의: 파이프 시험에서 일본어를 확정하면 실제 Mozc 학습에 남는다. 지울 때는 `mozc/examples/mozc_clear.rs`를 작업 스케줄러로 한 번 실행한다 (Claude 셸에서는 실제 Mozc 서버에 닿지 않음).

## 전각 공백·글꼴 (2026-11-23)

- Shift+Space: 전각 공백(U+3000). 조합 중이면 확정하고 붙인다. 보통 Space는 그대로 (한·영은 확정+공백, 일본어 문맥은 공백 없이 확정, 조합이 없으면 앱에 넘김).
- 메모장 일본어 글자 깨짐: 한국어 프로필 TIP로 넣은 글은 메모장이 한국어 글꼴로 그린다. 메모장 글꼴을 한·일 글자가 모두 있는 Noto Sans KR로 바꾸면 잘 표시된다 (사용자 확인). 프로필 전환 방식은 보류.

## 자동 꺼짐 (2026-11-23)

- 조합 중이 아닐 때 키가 오면 입력칸을 확인하고, IME를 쓰지 않을 칸이면 키를 먹지 않고 앱에 넘긴다 (`tip/src/lib.rs` `input_off`).
  - 앱이 키보드를 꺼 둔 문맥·빈 문맥 (`GUID_COMPARTMENT_KEYBOARD_DISABLED`, `GUID_COMPARTMENT_EMPTYCONTEXT`).
  - 입력칸 용도 (`GetAppProperty(GUID_PROP_INPUTSCOPE)`의 첫 용도): URL, 이메일·로그인 이름, 숫자·전화·금액, 비밀번호·PIN, 반각 영숫자.
  - 옛 Win32 입력칸(Edit, WinForms TextBox): 창 스타일 ES_NUMBER / ES_PASSWORD.
  - 브라우저는 한 문맥 안에서 칸이 바뀌므로 저장하지 않고 매번 확인한다 (읽기 전용 동기 편집 세션, 조합 시작 키에서만).
- 시험 창 `tools/ImeScopeTest.cs` (일반 / 비밀번호 / 숫자 / 이메일 칸): `Add-Type -TypeDefinition (Get-Content tools\ImeScopeTest.cs -Raw) -ReferencedAssemblies System.Windows.Forms -OutputAssembly target\imetest\ImeScopeTest.exe -OutputType WindowsApplication`
- 실측: 이메일 칸 용도 [5] → 꺼짐, 숫자 전용 칸 → 꺼짐, 비밀번호 칸은 Windows가 처음부터 IME에 키를 보내지 않음. 메모장 평소 입력 정상.
- 처음에 `GetProperty`로 읽어 늘 실패했다. 입력칸 용도는 앱 속성이라 `GetAppProperty`로 읽어야 한다.
- **새로 발견한 문제**: 옛 Win32 입력칸(CUAS 경유, 예: WinForms TextBox)에서는 키마다 조합이 끊겨 백엔드가 매번 처음부터 시작한다 (미리보기가 d → k → s 한 글자씩). 메모장(TSF 지원 RichEdit)은 정상. 다음 작업으로 조사 필요.

## 설정 파일과 일본어 Space (2026-11-23)

- 설정: `%LOCALAPPDATA%\multilang-ime\settings.json` (백엔드가 처음 시작할 때 기본값으로 만든다, 고치면 1초 안에 반영, BOM 있어도 됨). 설정 화면이 생기기 전까지는 직접 고친다.

  ```json
  { "ja_space": "commit" }
  ```

- `ja_space` — 일본어 문맥에서 조합 중의 Space:
  - `"commit"` (기본): 확정 + 전각 공백(U+3000). 예: `watashiha` Space → `私は　`.
  - `"continue"`: 확정하지 않고 조합을 이어 간다 (문장을 한 번에 변환, 부분 변환으로 문절별 수정). 확정은 구두점·Enter·Shift+Space. Space 뒤 새 단어가 일본어가 아니면(로마자로 가나가 안 되는 글자 2개 이상 + 판정이 일본어 아님) 앞의 일본어를 먼저 확정하고 새 단어만 조합한다 (`watashiha` Space `dkssud` → `私は` 확정, `안녕` 조합).
- 조합이 없을 때 Space는 문맥과 상관없이 보통 공백(앱에 넘김). 전각 공백은 언제나 Shift+Space.
- 한국어·영어 문맥의 Space는 그대로 확정 + 공백.

## 옛 Win32 입력칸: 조합 창 (2026-11-23)

- 문제: 옛 Win32 입력칸(문자표의 Edit, WinForms TextBox 등, Windows 호환 계층 CUAS 경유)에서 키마다 조합이 끊겨 백엔드가 매번 처음부터 시작했다 (d → k → s 한 글자씩 그대로 입력).
- 조사 (디버그 로그):
  - 조합은 키 처리가 끝난 직후 앱(CUAS) 쪽에서 끝난다 (OnCompositionTerminated). 우리 편집 단계(밑줄·언어 속성·위치 조회)와는 무관.
  - IME 켜짐 표시(KEYBOARD_OPENCLOSE)·입력 모드를 켜도 그대로 (이 설정은 남겨 둠).
  - 조합 전체를 interim(조합 중 글자) 선택으로 두면 **한 글자일 때만** 유지되고, 두 글자부터는 선택이 거부(E_INVALIDARG)되며 끊긴다 → 한국어 프로필의 CUAS는 조합 글자를 한 글자만 허용한다.
- 해결: CUAS 문맥(TS_SS_TRANSITORY)에서는 조합을 입력칸에 넣지 않고 커서 아래 **조합 창**(후보창 맨 윗줄)에 보이고, 확정할 때만 입력칸에 넣는다. 부분 변환 중이면 고치는 부분을 `[ ]`로 표시. 다른 입력칸으로 옮기면 남은 조합은 버린다. TSF 앱(메모장 등)은 지금처럼 입력칸 안 조합.
- 함께 고친 것: 옛 입력칸에서는 Shift에 대해 OnKeyDown 없이 OnTestKeyDown만 불려 Shift 탭이 인식되지 않았다 → OnTestKeyDown에서도 누른 시각을 기록.
- 실측 (문자표): `dkssud` → 조합 창 `안녕`, Space로 `안녕 ` 확정, `hello`, `go` + Shift 탭 → 후보(go, ｇｏ, 해, 海…, ご) + Tab → 五, `kyouhaiitenki` + Shift 탭 + → → `今日は[いい天気]`. 메모장은 그대로 입력칸 안 조합.

## 샌드박스(AppContainer) 앱 지원: 작업 표시줄 검색창 (2026-11-23)

- 문제: 작업 표시줄 검색창(SearchHost, AppContainer)에서 우리 IME가 불러와지지 않았다.
- 원인 두 가지:
  - `deployd` 폴더에 샌드박스 앱 읽기 권한이 없어 TIP DLL을 읽지 못한다.
  - 백엔드 파이프가 기본 보안으로 만들어져 샌드박스 앱(Low 무결성)이 접속하지 못한다.
- 수정:
  - 폴더: `icacls deployd /grant "*S-1-15-2-1:(OI)(CI)(RX)" /grant "*S-1-15-2-2:(OI)(CI)(RX)" /T` (ALL APPLICATION PACKAGES, ALL RESTRICTED APPLICATION PACKAGES). 설치 프로그램에도 넣어야 한다.
  - 파이프 (`backend/src/pipe_security.rs`): DACL = SYSTEM·이 사용자 전체, AC·LPAC 읽기/쓰기, SACL = 무결성 Low. 샌드박스 앱은 사용자 확인과 샌드박스 확인을 둘 다 통과해야 해 다른 사용자의 앱은 접속할 수 없다. 실제 적용 확인: `D:(A;;FA;;;SY)(A;;FA;;;<user>)(A;;0x12019f;;;AC)(A;;0x12019f;;;S-1-15-2-2)S:AI(ML;;NW;;;LW)`.
- 일반 앱 접속은 그대로 정상. 검색창 입력은 사용자가 확인함 (Claude의 화면 조작 권한으로는 셸 검색창에 입력할 수 없음).
- 후보창이 검색창 뒤에 가려지던 문제: 검색창은 몰입형(immersive) 화면이라 일반 창보다 위 층에 그려진다. 후보창(조합 창 포함)을 띄울 때 소유자를 포커스 창의 최상위 창으로 맞춰(`candwin::follow_owner`, GWLP_HWNDPARENT) 같은 층 위에 뜨게 했다. 메모장 후보창은 그대로 정상.
- 그래도 검색창에서는 후보창이 안 보인다. 진단 로그(샌드박스 앱도 쓸 수 있는 `deploy\x64\logs`, `MULTILANG_TIP_DEBUG`가 있을 때): SearchHost 안에서 위치 계산 성공, 창 생성 성공, 소유자 `Windows.UI.Core.CoreWindow`, `SetWindowPos` 성공, `IsWindowVisible`=true. 즉 창은 떠 있지만 셸 검색 화면이 일반 창보다 높은 창 층(band)에 있어 그 뒤에 가려진다. 일반 `CreateWindowEx` 창은 topmost·소유자 지정으로도 그 층 위에 갈 수 없다. 해결 후보: (1) 후보창을 별도 프로세스로 띄우고 UIAccess(서명 + Program Files 설치 필요)로 높은 층 사용, (2) TSF UI 요소(ITfCandidateListUIElement)로 후보 목록을 시스템에 넘기기(셸 검색창이 그려 주는지 불확실).
- UI 요소 방식 시험 (2026-11-23): 후보창을 띄울 때 `ITfUIElementMgr::BeginUIElement`로 `ITfCandidateListUIElement`(`tip/src/uielement.rs`)를 알리고, 앱이 그리겠다고(bShow=false) 하면 우리 창을 숨기게 했다. 검색창(SearchHost)과 메모장 모두 bShow=true(IME가 그려라)로 답해 검색창 문제는 해결되지 않았다. 게임 등 UI-less 앱용으로 코드는 유지 (UI-less 지원 등록 카테고리 `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`는 아직 등록하지 않음, 관리자 재등록 필요). 남은 해결책: 별도 후보창 프로세스 + UIAccess (서명·Program Files 설치와 함께, 배포 준비 단계).

## 알려진 한계

- 한국어 모델은 wordfreq의 형태소 단위 목록이라 어절 패턴이 약하다.
- 평가 데이터가 한 사람(트위터)의 문체에 치우쳐 있다. 공개 벤치마크 데이터가 필요하다.
- wordfreq 데이터는 CC BY-SA 4.0이라 배포 시 라이선스 검토가 필요하다.
