"""Rust 엔진과 Python v0 판정기의 결과 비교용 기준 파일을 만든다 (한·영·일).

사용: python eval/dump_parity.py [문장 수/파일] -> data/parity_koen.jsonl
"""
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from ime_v0.detector import Detector, Params  # noqa: E402
from ime_v0.models import load  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
FILES = ["ko_news", "ko_review", "en_news", "ja_news", "ja_news_bunsetsu", "ja_tatoeba_bunsetsu"]


def main(per_file: int = 300):
    p = json.loads((ROOT / "data/public/best_params_public_v4.json").read_text())
    det = Detector(load(), Params(**p), langs=("en", "ko", "ja"))
    rnd = random.Random(7)
    out = []
    for f in FILES:
        rows = [json.loads(l) for l in open(ROOT / f"data/public/{f}.jsonl", encoding="utf-8")]
        for r in rnd.sample(rows, min(per_file, len(rows))):
            dec = det.decide(r["keys"])
            out.append({
                "keys": r["keys"],
                "segments": [[s.lang if s.lang != "other" else None, s.keys, s.text] for s in dec.segments],
                "top3": [[[[l, n] for l, n in c.form] for c in t.ranked[:3]] for t in dec.tokens],
            })
    dst = ROOT / "data/parity_koen.jsonl"
    with open(dst, "w", encoding="utf-8") as fh:
        for o in out:
            fh.write(json.dumps(o, ensure_ascii=False) + "\n")
    print(f"{len(out)} sentences -> {dst}")


if __name__ == "__main__":
    main(*(int(a) for a in sys.argv[1:]))
