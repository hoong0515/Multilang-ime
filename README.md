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

## 알려진 한계

- 한국어 모델은 wordfreq의 형태소 단위 목록이라 어절 패턴이 약하다.
- 평가 데이터가 한 사람(트위터)의 문체에 치우쳐 있다. 공개 벤치마크 데이터가 필요하다.
- wordfreq 데이터는 CC BY-SA 4.0이라 배포 시 라이선스 검토가 필요하다.
