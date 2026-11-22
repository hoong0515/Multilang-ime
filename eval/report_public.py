"""공개 데이터 평가: 공개 dev로 튜닝하고, 출처별 test 결과를 표로 낸다.

사용: python eval/report_public.py [--tune 80] [--params 시작.json]
"""
import argparse
import json
import random
import sys
from dataclasses import asdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from evaluate import evaluate, load_params, split_of, tune  # noqa: E402
from ime_v0.detector import Detector, UserHistory  # noqa: E402
from ime_v0.models import load  # noqa: E402

PUB = Path("data/public")
BASE_FILES = ["ko_news", "ko_wiki", "ko_web", "ko_review", "ko_tatoeba", "en_news", "ja_news", "ja_wiki", "ja_tatoeba"]
BUNSETSU_FILES = ["ja_news_bunsetsu", "ja_wiki_bunsetsu", "ja_tatoeba_bunsetsu"]
TUNE_FILES = BASE_FILES + BUNSETSU_FILES
REPORT_FILES = TUNE_FILES
PRIVATE = {"tweets_koen": Path("data/private/tweets_koen.jsonl"), "tweets_ja": Path("data/private/tweets_ja.jsonl")}
LANGS = ("en", "ko", "ja")


def read(p):
    return [json.loads(l) for l in open(p, encoding="utf-8")]


def summarize(res):
    bk = res["by_kind"]
    single = {k: v["error_rate"] for k, v in bk.items() if "+" not in k}
    mixed_n = sum(v["n"] for k, v in bk.items() if "+" in k)
    mixed_err = sum(v["n"] * v["error_rate"] for k, v in bk.items() if "+" in k)
    rank2 = res["errors_gold_rank"].get("2", 0)
    n_err = round(res["token_error_rate"] * res["tokens"])
    return {"tokens": res["tokens"], "all": res["token_error_rate"], **single,
            "mixed_n": mixed_n, "mixed": round(mixed_err / mixed_n, 4) if mixed_n else None,
            "rank2_share": round(rank2 / n_err, 3) if n_err else None}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tune", type=int, default=0)
    ap.add_argument("--params")
    ap.add_argument("--dev-sample", type=int, default=500)
    ap.add_argument("--save", default="data/public/best_params_public_v3.json")
    ap.add_argument("--compare", default="data/public/best_params_public_v2.json", help="비교용 파라미터 (이전 버전)")
    a = ap.parse_args()
    m = load()
    rnd = random.Random(0)
    params = load_params(a.params)
    if a.tune:
        dev = []
        for f in TUNE_FILES:
            d = [r for r in read(PUB / f"{f}.jsonl") if split_of(r) == "dev"]
            dev += rnd.sample(d, min(a.dev_sample, len(d)))
        params = tune(m, dev, a.tune, rnd, LANGS, params)
        Path(a.save).write_text(json.dumps(asdict(params), indent=1))
        print(f"saved {a.save}", flush=True)

    sets = {f: PUB / f"{f}.jsonl" for f in REPORT_FILES}
    sets.update({k: v for k, v in PRIVATE.items() if v.exists()})
    table = {}
    for label, p in (("public_tuned", params), ("compare", load_params(a.compare))):
        for name, path in sets.items():
            test = [r for r in read(path) if split_of(r) == "test"]
            test.sort(key=lambda r: int(r["id"]) if r["id"].isdigit() else 0)
            r0, _ = evaluate(Detector(m, p, langs=LANGS), test)
            row = {"no_learning": summarize(r0)}
            if label == "public_tuned":
                r1, _ = evaluate(Detector(m, p, UserHistory(), langs=LANGS), test, learn=True)
                row["with_learning"] = summarize(r1)
            table.setdefault(name, {})[label] = row
            print(name, label, json.dumps(row), flush=True)
    Path("data/public/report.json").write_text(json.dumps(table, indent=1, ensure_ascii=False))
    print("saved data/public/report.json")


if __name__ == "__main__":
    main()
