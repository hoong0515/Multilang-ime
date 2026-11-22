"""공개 데이터(wordfreq)로 언어별 키 입력 n-gram과 빈도 사전을 만든다.

- en: 영어 단어 그대로
- ko: 한국어 단어를 두벌식 키 입력으로 바꿔서
- ja: 일본어 단어의 읽기(fugashi + unidic-lite)를 IME식 로마자로 바꿔서
"""
from __future__ import annotations

import math
import pickle
import re
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

from .hangul import to_keys
from .ngram import CharNgram
from .romaji import kata_to_hira, to_romaji

CACHE = Path(__file__).resolve().parent.parent / "data" / "models.pkl"
EN_WORD = re.compile(r"^[a-z]+$")
KO_WORD = re.compile(r"^[가-힣ㄱ-ㅣ]+$")
JA_WORD = re.compile(r"^[ぁ-ゟ゠-ヿ一-鿿々ー]+$")
KANA_ONLY = re.compile(r"^[ぁ-ゟー]+$")

EXPRESSIVE = {"ㅋ": 3000.0, "ㅎ": 1500.0, "ㅠ": 1500.0, "ㅜ": 800.0}


@dataclass
class LangModels:
    en: CharNgram
    ko: CharNgram
    en_zipf: dict[str, float]   # 소문자 영어 단어 -> zipf (log10 빈도 + 9)
    ko_zipf: dict[str, float]   # 한글 단어 -> zipf
    ja: CharNgram | None = None
    ja_zipf: dict[str, float] = field(default_factory=dict)  # 히라가나 읽기 -> zipf


def _zipf(freq: float) -> float:
    return math.log10(freq) + 9


def japanese_reading(tagger, text: str) -> str | None:
    """일본어 텍스트 -> 히라가나 읽기 (입력 기준: は는 は, を는 を)."""
    out = []
    for w in tagger(text):
        k = w.feature.kana
        if not k or k == "*":
            if KANA_ONLY.match(kata_to_hira(w.surface)):
                k = w.surface
            else:
                return None
        out.append(kata_to_hira(k))
    return "".join(out)


def _build_ja(n_ja: int):
    import fugashi
    import wordfreq

    tagger = fugashi.Tagger()
    items = [(w, f) for w, f in wordfreq.get_frequency_dict("ja").items() if JA_WORD.match(w)]
    items.sort(key=lambda x: -x[1])
    items = items[:n_ja]
    ja = CharNgram(4)
    zipf_sum: dict[str, float] = defaultdict(float)
    fmin = items[-1][1]
    for w, f in items:
        r = japanese_reading(tagger, w)
        if not r:
            continue
        ja.add(to_romaji(r), math.sqrt(f / fmin))
        zipf_sum[r] += f
    ja.finalize()
    return ja, {r: _zipf(f) for r, f in zipf_sum.items()}


def build(order: int = 4, n_en: int = 60000, n_ja: int = 80000) -> LangModels:
    import wordfreq

    en_freq = {w: f for w, f in wordfreq.get_frequency_dict("en").items() if EN_WORD.match(w)}
    ko_freq = {w: f for w, f in wordfreq.get_frequency_dict("ko").items() if KO_WORD.match(w)}
    en_top = sorted(en_freq.items(), key=lambda x: -x[1])[:n_en]

    en, ko = CharNgram(order), CharNgram(order)
    # 빈도의 제곱근으로 가중: 최상위 기능어가 모델을 독점하지 않게
    fmin = min(f for _, f in en_top)
    for w, f in en_top:
        en.add(w, math.sqrt(f / fmin))
    fmin_ko = min(ko_freq.values())
    for w, f in ko_freq.items():
        ko.add(to_keys(w).lower(), math.sqrt(f / fmin_ko))
    # wordfreq(형태소 단위)에 거의 없는 인터넷 표현용 낱자모 패턴을 보강
    for base, w in EXPRESSIVE.items():
        for n in range(2, 9):
            ko.add(to_keys(base * n).lower(), w / n)
        ko.add(to_keys(base).lower(), w / 4)
    for expr in ("ㄷㄷ", "ㅇㅇ", "ㄴㄴ", "ㄱㄱ", "ㅇㅋ", "ㅊㅋ", "ㅅㅂ", "ㅈㅅ", "ㄳ", "ㅎㄷㄷ", "ㅡㅡ", "ㅇㅅㅇ", "ㅁㄴㅇㄹ"):
        ko.add(to_keys(expr).lower(), 200.0)
    en.finalize()
    ko.finalize()
    ja, ja_zipf = _build_ja(n_ja)
    return LangModels(
        en=en, ko=ko,
        en_zipf={w: _zipf(f) for w, f in en_top},
        ko_zipf={w: _zipf(f) for w, f in ko_freq.items()},
        ja=ja, ja_zipf=ja_zipf,
    )


def load(rebuild: bool = False) -> LangModels:
    if CACHE.exists() and not rebuild:
        with CACHE.open("rb") as fh:
            m = pickle.load(fh)
        if getattr(m, "ja", None) is not None:
            return m
    m = build()
    CACHE.parent.mkdir(parents=True, exist_ok=True)
    for lm in (m.en, m.ko, m.ja):
        lm._cache.clear()
    with CACHE.open("wb") as fh:
        pickle.dump(m, fh)
    return m
