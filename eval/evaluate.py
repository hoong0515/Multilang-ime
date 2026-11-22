"""판정기 오판률 측정, 파라미터 탐색, 후보 선택·학습 시뮬레이션.

사용:
  python eval/evaluate.py data/private/tweets_koen.jsonl              # 기본 파라미터로 평가
  python eval/evaluate.py data/private/tweets_koen.jsonl --tune 120   # dev에서 탐색 후 test 평가

평가는 두 번 돈다.
  1) 학습 없음: 매 토큰을 처음 보는 것처럼 판정
  2) 학습 있음: test 트윗을 시간순으로 넣으며, 각 트윗을 입력한 뒤 사용자가 최종 확정한 형태
     (= 정답, 틀렸으면 후보창이나 수동으로 고쳤다고 가정)를 UserHistory에 기록
"""
import argparse
import hashlib
import json
import random
import sys
import time
from collections import Counter
from dataclasses import asdict, fields
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from ime_v0.detector import Detector, Params, UserHistory  # noqa: E402
from ime_v0.models import load  # noqa: E402


def split_of(row) -> str:
    return "dev" if int(hashlib.md5(row["id"].encode()).hexdigest(), 16) % 5 == 0 else "test"


def gold_form(g):
    return tuple((lang, n) for lang, n in g)


def kind(form) -> str:
    return "+".join(l for l, _ in form)


def evaluate(det: Detector, rows, learn: bool = False, show_errors: int = 0):
    by_kind, ok_kind = Counter(), Counter()
    rank_of_gold = Counter()      # 틀렸을 때 정답이 후보 몇 위였나
    confusion = Counter()         # 틀렸을 때 (정답 종류 -> 예측 종류)
    amb_total = amb_on_error = 0
    tweets_ok = 0
    errors = []
    t0 = time.perf_counter()
    for r in rows:
        dec = det.decide(r["keys"])
        if len(dec.tokens) != len(r["gold"]):
            raise RuntimeError(f"token mismatch in {r['id']}")
        all_ok = True
        for g, tr in zip(r["gold"], dec.tokens):
            gf = gold_form(g)
            k = kind(gf)
            by_kind[k] += 1
            amb = tr.margin < det.p.ambiguous
            amb_total += amb
            if tr.chosen.form == gf:
                ok_kind[k] += 1
            else:
                all_ok = False
                amb_on_error += amb
                confusion[f"{k} -> {kind(tr.chosen.form)}"] += 1
                forms = [c.form for c in tr.ranked]
                rank_of_gold[forms.index(gf) + 1 if gf in forms and forms.index(gf) < 3 else "4+"] += 1
                if len(errors) < show_errors:
                    errors.append((r["text"], g, [list(x) for x in tr.chosen.form], tr.chosen.text))
            if learn:
                det.h.record(tr.keys, gf)
        tweets_ok += all_ok
    dt = time.perf_counter() - t0
    total = sum(by_kind.values())
    n_err = total - sum(ok_kind.values())
    res = {
        "tokens": total,
        "token_error_rate": round(n_err / total, 5),
        "tweet_error_rate": round(1 - tweets_ok / len(rows), 4),
        "by_kind": {k: {"n": n, "error_rate": round(1 - ok_kind[k] / n, 4)} for k, n in by_kind.most_common()},
        "errors_gold_rank": {str(k): v for k, v in sorted(rank_of_gold.items(), key=lambda x: str(x[0]))},
        "confusion_top": dict(confusion.most_common(8)),
        "ambiguous_token_rate": round(amb_total / total, 4),
        "errors_flagged_ambiguous": round(amb_on_error / max(1, n_err), 4),
        "ms_per_token": round(1000 * dt / total, 3),
    }
    return res, errors


def balanced_score(res) -> float:
    """언어별 데이터 양 차이가 크므로 종류별 오류율의 가중 평균으로 튜닝한다 (혼용 토큰은 절반 가중)."""
    tot = wsum = 0.0
    for k, v in res["by_kind"].items():
        w = 1.0 if "+" not in k else 0.5
        if v["n"] < 30:
            continue
        tot += w * v["error_rate"]
        wsum += w
    return tot / wsum


RANGES = {"en_dict": (0, 3), "ko_dict": (0, 3), "zipf_floor": (0, 4), "orphan": (0, 10), "orphan_expr": (-3, 3),
          "shift_noise": (0, 8), "switch": (0, 8), "split": (0, 16), "particle": (0, 12),
          "ja_dict": (0, 3), "ja_leftover": (0, 10), "caps_en": (2, 16), "short_switch": (0, 10), "switch_cjk": (0, 10)}


def tune(m, dev, iters, rnd, langs, start: Params):
    best_p = start
    best = balanced_score(evaluate(Detector(m, best_p, langs=langs), dev)[0])
    print(f"start dev score {best:.4f}")
    for i in range(iters):
        base = asdict(best_p)
        cand = dict(base)
        for name, (lo, hi) in RANGES.items():
            if i % 3 == 0:
                cand[name] = rnd.uniform(lo, hi)
            else:
                cand[name] = min(hi, max(lo, base[name] + rnd.gauss(0, (hi - lo) * 0.08)))
        p = Params(**cand)
        sc = balanced_score(evaluate(Detector(m, p, langs=langs), dev)[0])
        if sc < best:
            best, best_p = sc, p
            print(f"[{i}] dev {sc:.4f} " + json.dumps({k: round(v, 2) for k, v in asdict(p).items() if k in RANGES}),
                  flush=True)
    return best_p


def load_params(path):
    p = Params()
    if path:
        d = json.load(open(path))
        p = Params(**{f.name: d[f.name] for f in fields(Params) if f.name in d})
    return p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("data", nargs="+", help="jsonl 파일들 (여러 개면 합쳐서 평가)")
    ap.add_argument("--langs", default="en,ko")
    ap.add_argument("--tune", type=int, default=0)
    ap.add_argument("--dev-sample", type=int, default=1500, help="파일당 dev 표본 수")
    ap.add_argument("--errors", type=int, default=20)
    ap.add_argument("--params", help="시작 파라미터 JSON")
    ap.add_argument("--save", help="튜닝 결과 저장 경로")
    a = ap.parse_args()
    langs = tuple(a.langs.split(","))

    rnd = random.Random(0)
    dev, test = [], []
    for path in a.data:
        rows = [json.loads(l) for l in open(path, encoding="utf-8")]
        d = [r for r in rows if split_of(r) == "dev"]
        dev += rnd.sample(d, min(a.dev_sample, len(d)))
        test += [r for r in rows if split_of(r) == "test"]
    test.sort(key=lambda r: int(r["id"]) if r["id"].isdigit() else 0)  # 트윗: 스노플레이크 id = 시간순
    m = load()
    params = load_params(a.params)
    if a.tune:
        params = tune(m, dev, a.tune, rnd, langs, params)
        if a.save:
            Path(a.save).write_text(json.dumps(asdict(params), indent=1))
            print(f"saved {a.save}")

    print("langs", langs, "params", json.dumps({k: round(v, 2) for k, v in asdict(params).items()}))
    res0, errors = evaluate(Detector(m, params, langs=langs), test, show_errors=a.errors)
    res1, _ = evaluate(Detector(m, params, UserHistory(), langs=langs), test, learn=True)
    print(json.dumps({"test_tweets": len(test), "no_learning": res0, "with_learning": res1}, ensure_ascii=False, indent=1))
    print("errors (no learning):")
    for text, g, f, t in errors:
        print(f"  {text[:46]!r} gold={g} pred={f} -> {t!r}")


if __name__ == "__main__":
    main()
