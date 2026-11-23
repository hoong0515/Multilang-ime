"""Rust 엔진이 읽을 모델 파일을 만든다 (data/engine_model.json).

n-gram 집계, 빈도 사전, 판정 파라미터를 그대로 옮긴다. Python v0과 Rust 엔진이 같은 수치를 쓰게 하려는 것.
사용: python -m ime_v0.export_model [파라미터 JSON] [출력 경로]
기본 파라미터는 data/model_params.json (git에 있는 판정 파라미터, 공개 데이터 튜닝 v4).
CI는 requirements-model.txt(고정 버전, Python 3.10)로 만들며 개발용 파일과 바이트까지 같다.
"""
import json
import sys
from pathlib import Path

from .models import load

ROOT = Path(__file__).resolve().parent.parent


def ngram_dict(lm):
    return {
        "order": lm.n,
        "discount": lm.d,
        "vocab_size": lm.V,
        # levels[k][context] = {char: count}
        "levels": [{ctx: dict(chars) for ctx, chars in level.items()} for level in lm.counts],
        # totals[k][context] = [합계, 종류 수] (Python과 같은 합산 순서의 값을 그대로 쓰기 위해)
        "totals": [{ctx: list(t) for ctx, t in level.items()} for level in lm.totals],
    }


def main(params_path=None, out_path=None):
    params_path = Path(params_path or ROOT / "data/model_params.json")
    out_path = Path(out_path or ROOT / "data/engine_model.json")
    m = load()
    data = {
        "version": 1,
        "params": json.loads(params_path.read_text()),
        "ngram": {"en": ngram_dict(m.en), "ko": ngram_dict(m.ko), "ja": ngram_dict(m.ja)},
        "zipf": {"en": m.en_zipf, "ko": m.ko_zipf, "ja": m.ja_zipf},
    }
    out_path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    print(f"{out_path} ({out_path.stat().st_size / 1e6:.1f} MB)")


if __name__ == "__main__":
    main(*sys.argv[1:])
