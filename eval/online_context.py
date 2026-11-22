"""문장 전체 비터비(오프라인) 대 IME처럼 왼쪽 문맥만 쓰는 판정(온라인)의 정확도 비교.

온라인: 토큰을 하나씩 확정한다. 각 토큰은 직전에 확정한 토큰의 언어만 문맥으로 받는다
(지금 백엔드와 같은 방식).
사용: python eval/online_context.py
"""
import json
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from evaluate import gold_form, load_params, split_of  # noqa: E402
from ime_v0.detector import TOKEN, Detector  # noqa: E402
from ime_v0.models import load  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
SETS = {"ko_news": "data/public/ko_news.jsonl", "ko_review": "data/public/ko_review.jsonl",
        "ja_news_bunsetsu": "data/public/ja_news_bunsetsu.jsonl", "en_news": "data/public/en_news.jsonl",
        "tweets_koen": "data/private/tweets_koen.jsonl", "tweets_ja": "data/private/tweets_ja.jsonl"}


def main(limit=1500):
    m = load()
    det = Detector(m, load_params(ROOT / "data/public/best_params_public_v4.json"), langs=("en", "ko", "ja"))
    for name, path in SETS.items():
        p = ROOT / path
        if not p.exists():
            continue
        rows = [json.loads(l) for l in open(p, encoding="utf-8")]
        rows = [r for r in rows if split_of(r) == "test"][:limit]
        ok = Counter()
        n = 0
        for r in rows:
            offline = [t.chosen.form for t in det.decide(r["keys"]).tokens]
            online, prev = [], None
            for tok in TOKEN.findall(r["keys"]):
                if not ("a" <= tok[0].lower() <= "z"):
                    continue
                t = det.decide(tok, prev).tokens[0]
                online.append(t.chosen.form)
                prev = t.chosen.end
            for g, a, b in zip(r["gold"], offline, online):
                gf = gold_form(g)
                n += 1
                ok["offline"] += a == gf
                ok["online"] += b == gf
        print(f"{name:18s} tokens {n:6d}  sentence-level error {1 - ok['offline'] / n:6.2%}  "
              f"left-context-only error {1 - ok['online'] / n:6.2%}", flush=True)


if __name__ == "__main__":
    main()
