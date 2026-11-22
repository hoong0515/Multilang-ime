"""다국어(한·영·일) 언어 판정기 (v0).

키 입력 문자열을 받아 알파벳 덩어리(토큰)마다 후보(EN / KO / EN+KO 분할 / KO+EN 분할)를 만들고,
토큰 점수 = 키 입력 n-gram 로그우도 + 사전 보너스 - 깨진 한글 벌점 + 사용자 선택 기록 보너스,
문장 전체는 언어가 바뀔 때 벌점을 주는 비터비로 고른다.

모호한 토큰(1·2위 점수 차가 작음)은 후보창에 다른 언어 후보를 함께 보여주고,
사용자가 고른 후보는 UserHistory에 쌓여 다음부터 위로 올라간다.
"""
from __future__ import annotations

import math
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field

from .hangul import compose
from .romaji import parse
from .models import LangModels

TOKEN = re.compile(r"[A-Za-z]+|[^A-Za-z]+")

# 한국어에서 낱자로 자주 쓰는 자모 (ㅋㅋ, ㅎㅎ, ㅠㅠ, ㄷㄷ, ㅇㅇ, ㄱㄱ, ㅅㅂ ...)
EXPRESSIVE_JAMO = set("ㅋㅎㅠㅜㄷㅇㄱㅅㅂㅈㄴㅊ")
# 두벌식에서 Shift가 의미 있는 키. 그 외 대문자는 한국어 입력 중엔 거의 나오지 않는다
KO_SHIFT_KEYS = set("REQTWOP")
# 영어 단어 뒤에 붙는 흔한 조사·어미 (EN+KO 분할의 근거)
KO_PARTICLES = set("""을 를 이 가 은 는 의 에 에서 으로 로 와 과 도 만 까지 부터 이나 나 이랑 랑 하고 처럼 보다 한테 에게 께
이다 이고 이라 이라서 이라고 라고 인 인데 이면 면 이야 야 이에요 예요 입니다 같은 같이 쪽 용 적 적인 하다 하는 해서 했다 한 할 함 해 됨 된 되는""".split())
JA_PARTICLES = set("""を が に で は の と も へ から まで より って です だ じゃ な っぽい する して した さん くん ちゃん
とか みたい だけ しか でも では には とは のは を する すき がすき""".split())
PARTICLES = {"ko": KO_PARTICLES, "ja": JA_PARTICLES}
# 한 토큰 안에서 바로 이어지지 않는 언어 쌍 (한국어와 일본어를 붙여 치는 경우는 없다고 본다)
NO_SPLIT = {("ko", "ja"), ("ja", "ko")}

Form = tuple  # ((lang, n_keys), ...) 예: (("en", 3), ("ko", 3))


@dataclass
class Params:
    en_dict: float = 1.6        # 영어 사전 보너스 (zipf 당)
    ko_dict: float = 1.0        # 한국어 사전 보너스 (zipf 당)
    zipf_floor: float = 1.8     # 이 zipf 이하는 보너스 없음
    orphan: float = 5.0         # 낱자모 1개당 벌점 (EXPRESSIVE_JAMO 제외)
    orphan_expr: float = 0.5    # ㅋ·ㅎ·ㅠ 같은 표현용 낱자모 1개당 벌점
    shift_noise: float = 4.0    # 한국어 해석에서 의미 없는 대문자 1개당 벌점
    switch: float = 5.0         # 앞 토큰과 언어가 다를 때 벌점
    split: float = 10.0         # 한 토큰 안에서 언어를 나눌 때 벌점
    particle: float = 6.0       # 분할의 한국어 부분이 조사·어미면 보너스
    min_split: int = 1          # 분할 시 각 부분 최소 키 수
    ja_dict: float = 1.0        # 일본어 사전 보너스 (읽기 zipf 당)
    ja_leftover: float = 5.0    # 가나가 되지 못한 로마자 1글자당 벌점
    caps_en: float = 8.0        # 전부 대문자인 구간(ESG, NFT)을 영어로 볼 때 보너스
    short_switch: float = 3.0   # 짧은 토큰(4키 이하, 전부 대문자 제외)에서 언어가 바뀔 때 추가 벌점 (to, no, au)
    switch_cjk: float = 3.0     # 토큰 사이에서 한국어<->일본어로 바뀔 때 추가 벌점
    history: float = 4.0        # 사용자 선택 기록 보너스 (log(1+횟수) 당)
    ambiguous: float = 3.0      # 1·2위 점수 차가 이보다 작으면 모호 토큰


@dataclass
class Segment:
    lang: str        # "en" | "ko" | "other"
    keys: str
    text: str


@dataclass
class Candidate:
    form: Form
    segments: list[Segment]
    score: float

    @property
    def start(self) -> str:
        return self.form[0][0]

    @property
    def end(self) -> str:
        return self.form[-1][0]

    @property
    def text(self) -> str:
        return "".join(s.text for s in self.segments)


@dataclass
class TokenResult:
    keys: str
    ranked: list[Candidate]          # 문맥 반영 점수 순
    chosen: Candidate                # 비터비가 고른 후보 (보통 ranked[0])

    @property
    def margin(self) -> float:
        return self.ranked[0].score - self.ranked[1].score if len(self.ranked) > 1 else math.inf


@dataclass
class Decision:
    segments: list[Segment] = field(default_factory=list)
    tokens: list[TokenResult] = field(default_factory=list)

    @property
    def text(self) -> str:
        return "".join(s.text for s in self.segments)


class UserHistory:
    """키 입력 토큰별로 사용자가 확정한 형태(언어·분할)를 센다. 사용자 사전의 '변환 학습'에 해당."""

    def __init__(self):
        self.exact: dict[str, Counter] = defaultdict(Counter)    # 친 그대로 (대소문자 구분)
        self.folded: dict[str, Counter] = defaultdict(Counter)   # 대소문자 무시

    def record(self, keys: str, form: Form):
        self.exact[keys][form] += 1
        self.folded[keys.lower()][form] += 1

    def count(self, keys: str, form: Form) -> float:
        """같은 대소문자로 친 기록은 그대로, 대소문자만 다른 기록은 절반만 반영."""
        e = self.exact.get(keys)
        f = self.folded.get(keys.lower())
        ne = e[form] if e else 0
        nf = f[form] if f else 0
        return ne + 0.5 * (nf - ne)


class Detector:
    def __init__(self, models: LangModels, params: Params | None = None, history: UserHistory | None = None,
                 langs: tuple[str, ...] = ("en", "ko")):
        self.m = models
        self.langs = langs
        self.p = params or Params()
        self.h = history
        self._memo: dict[tuple[str, str], float] = {}

    # 단일 언어 점수 --------------------------------------------------
    def score_en(self, keys: str) -> float:
        k = ("en", keys)
        if k in self._memo:
            return self._memo[k]
        low = keys.lower()
        s = self.m.en.logprob(low)
        z = self.m.en_zipf.get(low, 0.0)
        s += self.p.en_dict * max(0.0, z - self.p.zipf_floor)
        if keys.isupper() and (len(keys) >= 2 or keys not in KO_SHIFT_KEYS):
            s += self.p.caps_en  # 전부 대문자: 약어일 가능성이 높다
        self._memo[k] = s
        return s

    def score_ko(self, keys: str) -> float:
        k = ("ko", keys)
        if k in self._memo:
            return self._memo[k]
        s = self.m.ko.logprob(keys.lower())
        c = compose(keys)
        expr = sum(1 for j in c.orphan_list if j in EXPRESSIVE_JAMO)
        s -= self.p.orphan * (c.orphans - expr) + self.p.orphan_expr * expr
        s -= self.p.shift_noise * sum(1 for ch in keys if ch.isupper() and ch not in KO_SHIFT_KEYS)
        z = self.m.ko_zipf.get(c.text, 0.0)
        s += self.p.ko_dict * max(0.0, z - self.p.zipf_floor)
        self._memo[k] = s
        return s

    def score_ja(self, keys: str) -> float:
        k = ("ja", keys)
        if k in self._memo:
            return self._memo[k]
        s = self.m.ja.logprob(keys.lower())
        r = parse(keys)
        s -= self.p.ja_leftover * r.leftover
        s -= self.p.shift_noise * sum(1 for ch in keys if ch.isupper())
        z = self.ja_word_zipf(r.kana)
        s += self.p.ja_dict * max(0.0, z - self.p.zipf_floor)
        self._memo[k] = s
        return s

    def ja_word_zipf(self, kana: str) -> float:
        """가나 읽기의 사전 빈도. 문절(단어 + 조사·조동사)도 잡도록 두 조각으로 나뉘는 경우까지 본다."""
        k = ("jaz", kana)
        if k in self._memo:
            return self._memo[k]
        zd = self.m.ja_zipf
        z = zd.get(kana, 0.0)
        if len(kana) <= 12:
            for i in range(1, len(kana)):
                a, b = zd.get(kana[:i]), zd.get(kana[i:])
                if a and b:
                    z = max(z, min(a, b) - 0.5)
        self._memo[k] = z
        return z

    def score(self, lang: str, keys: str) -> float:
        return {"en": self.score_en, "ko": self.score_ko, "ja": self.score_ja}[lang](keys)

    @staticmethod
    def render(lang: str, keys: str) -> str:
        if lang == "ko":
            return compose(keys).text
        if lang == "ja":
            return parse(keys).kana
        return keys

    def _suffix(self, lang: str, keys: str) -> float:
        """분할의 뒷부분 점수. 조사·어미면 보너스."""
        s = self.score(lang, keys)
        if self.render(lang, keys) in PARTICLES.get(lang, ()):
            s += self.p.particle
        return s

    # 토큰 후보 -------------------------------------------------------
    @staticmethod
    def _en_entry(keys: str, j: int, prev: str) -> bool:
        """토큰 중간 j에서 영어가 시작될 수 있나.
        일본어 뒤: 앞 4글자 안에 대문자 (Assistant, iPhone, eSIM, macOS)
        한국어 뒤: 대문자로 시작할 때만 (ㄲ·ㅆ 등 Shift 자모가 섞인 한국어를 영어로 오인하지 않게)"""
        if prev == "ko":
            return keys[j].isupper()
        return any(ch.isupper() for ch in keys[j:j + 4])

    def _lattice(self, keys: str, start: str) -> dict[str, tuple[float, list[tuple[str, int, int]]]]:
        """start 언어로 시작해 토큰을 언어 구간들로 나누는 최선 경로 (끝 언어별)."""
        n = len(keys)
        ms = self.p.min_split
        best: list[dict[str, tuple[float, list]]] = [dict() for _ in range(n + 1)]
        upper = [ch.isupper() for ch in keys]
        entry = {l: [self._en_entry(keys, j, l) for j in range(n)] + [True] for l in ("ko", "ja")}  # 끝은 항상 허용

        def min_len(lang: str) -> int:
            return ms if lang == "en" else max(ms, 2)  # 한국어·일본어 조각은 2키 이상 (WalkinㅎCat 방지)

        def may_end(lang: str, i: int, j: int) -> bool:
            """lang 구간이 j에서 끝날 수 있나 (다음 전환이 가능한 지점인가)."""
            if j == n:
                return True
            if lang != "en":
                return entry[lang][j]    # 한국어·일본어 다음은 영어뿐
            if any(upper[i:j]):
                return True              # 대문자가 있는 영어 다음은 어디서든
            return "ko" in self.langs and n - j <= 8  # 소문자 영어 다음은 짧은 한국어 조사·어미만

        for j in range(min_len(start), n + 1):
            if may_end(start, 0, j):
                best[j][start] = (self.score(start, keys[:j]), [(start, 0, j)])
        for j in range(ms, n - ms + 1):
            for l, (sc, path) in list(best[j].items()):
                for l2 in self.langs:
                    if l2 == l or (l, l2) in NO_SPLIT:
                        continue
                    if l2 == "en" and not entry[l][j]:  # 영어로 바뀌는 지점은 대문자 근처만
                        continue
                    ks = range(j + min_len(l2), n + 1)
                    if l == "en" and not any(upper[path[-1][1]:j]):
                        # 소문자 영어 뒤에는 짧은 한국어 조사·어미로 토큰이 끝나는 경우만 (reactdptj -> react에서)
                        if n - j > 8 or l2 != "ko":
                            continue
                        ks = [n]
                    for k in ks:
                        if not may_end(l2, j, k):
                            continue
                        seg = keys[j:k]
                        v = sc + self._suffix(l2, seg) - self.p.split
                        cur = best[k].get(l2)
                        if cur is None or v > cur[0]:
                            best[k][l2] = (v, path + [(l2, j, k)])
        return best[n]

    def candidates(self, keys: str) -> list[Candidate]:
        n = len(keys)
        out = [Candidate(((l, n),), [Segment(l, keys, self.render(l, keys))], self.score(l, keys))
               for l in self.langs]
        if n >= 2 * self.p.min_split:
            for start in self.langs:
                for end, (sc, path) in self._lattice(keys, start).items():
                    if len(path) < 2:
                        continue
                    out.append(Candidate(
                        tuple((l, j - i) for l, i, j in path),
                        [Segment(l, keys[i:j], self.render(l, keys[i:j])) for l, i, j in path], sc))
        if self.h:
            for c in out:
                cnt = self.h.count(keys, c.form)
                if cnt:
                    c.score += self.p.history * math.log1p(cnt)
        return out

    def _ctx(self, c: Candidate, prev: str | None) -> float:
        if prev is None or prev == c.start:
            return c.score
        pen = self.p.switch
        first = c.segments[0].keys
        if len(first) <= 4 and not first.isupper():
            pen += self.p.short_switch  # 문맥과 다른 짧은 토큰(to, no, au, ro)은 드물다
        if {prev, c.start} == {"ko", "ja"}:
            pen += self.p.switch_cjk    # 한국어와 일본어를 번갈아 쓰는 경우는 드물다
        return c.score - pen

    # 문장 단위 비터비 -------------------------------------------------
    def decide(self, keys: str, prev_lang: str | None = None) -> Decision:
        toks = TOKEN.findall(keys)
        # state: 마지막 언어 -> (누적 점수, 고른 후보 목록)
        states: dict[str | None, tuple[float, list]] = {prev_lang: (0.0, [])}
        cand_cache = []
        for t in toks:
            if not ("a" <= t[0].lower() <= "z"):  # 알파벳이 아닌 덩어리 (한자·기호 등 포함)
                states = {l: (sc, hist + [t]) for l, (sc, hist) in states.items()}
                continue
            cands = self.candidates(t)
            cand_cache.append(cands)
            new: dict[str | None, tuple[float, list]] = {}
            for c in cands:
                for l, (acc, hist) in states.items():
                    tot = acc + self._ctx(c, l)
                    if c.end not in new or tot > new[c.end][0]:
                        new[c.end] = (tot, hist + [c])
            states = new
        best = max(states.values(), key=lambda x: x[0])[1]

        dec = Decision()
        prev, ci = prev_lang, 0
        for item in best:
            if isinstance(item, str):
                dec.segments.append(Segment("other", item, item))
                continue
            cands = cand_cache[ci]
            ci += 1
            ranked = sorted(cands, key=lambda c: -self._ctx(c, prev))
            ranked = [Candidate(c.form, c.segments, self._ctx(c, prev)) for c in ranked]
            dec.tokens.append(TokenResult(item.segments[0].keys if len(item.segments) == 1 else
                                          "".join(s.keys for s in item.segments), ranked, item))
            dec.segments.extend(item.segments)
            prev = item.end
        return dec
