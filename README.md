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
