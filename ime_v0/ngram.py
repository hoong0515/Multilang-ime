"""키 입력 문자 n-gram 모델 (보간 절대 할인)."""
from __future__ import annotations

import math
from collections import defaultdict

BOS, EOS = "^", "$"


class CharNgram:
    def __init__(self, order: int = 4, discount: float = 0.75):
        self.n = order
        self.d = discount
        # counts[k][ctx][ch]: 길이 k 문맥 뒤에 ch가 나온 횟수
        self.counts: list[dict[str, dict[str, float]]] = [defaultdict(lambda: defaultdict(float)) for _ in range(order)]
        self.vocab: set[str] = set()
        self._cache: dict[tuple[str, str], float] = {}

    def add(self, word: str, weight: float = 1.0):
        s = BOS * (self.n - 1) + word + EOS
        for i in range(self.n - 1, len(s)):
            ch = s[i]
            self.vocab.add(ch)
            for k in range(self.n):
                self.counts[k][s[i - k:i]][ch] += weight

    def finalize(self):
        """dict 집계를 (합계, 종류 수) 요약과 함께 굳힌다."""
        self.totals = [{c: (sum(v.values()), len(v)) for c, v in lvl.items()} for lvl in self.counts]
        self.counts = [{c: dict(v) for c, v in lvl.items()} for lvl in self.counts]
        self.V = len(self.vocab) + 1

    def prob(self, ctx: str, ch: str) -> float:
        key = (ctx, ch)
        p = self._cache.get(key)
        if p is not None:
            return p
        p = 1.0 / self.V
        for k in range(0, self.n):
            c = ctx[len(ctx) - k:] if k else ""
            tot = self.totals[k].get(c)
            if not tot:
                continue
            total, types = tot
            cnt = self.counts[k][c].get(ch, 0.0)
            p = max(cnt - self.d, 0.0) / total + (self.d * types / total) * p
        self._cache[key] = p
        return p

    def logprob(self, word: str) -> float:
        s = BOS * (self.n - 1) + word + EOS
        return sum(math.log(self.prob(s[i - self.n + 1:i], s[i])) for i in range(self.n - 1, len(s)))
