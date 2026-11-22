"""v0 데모: 키 입력을 한 줄씩 넣으면 판정 결과와 후보를 보여준다.

일본어 IME처럼 '조합 → 1위 후보 자동 선택 → 후보 순환 → 확정 시 학습' 흐름을 흉내 낸다.

사용: python -m ime_v0.cli [--params data/private/best_params_koja.json] [--langs en,ko,ja]
  줄 입력      키 입력 (영문 자판 그대로, 예: dkssud React dptj)
  :N K         직전 문장의 N번째 토큰을 K번째 후보로 바꿔 확정 (학습)
  :ok          직전 문장을 그대로 확정 (학습)
  :q           종료
"""
from __future__ import annotations

import argparse
import json
from dataclasses import fields

from .detector import Detector, Params, UserHistory
from .models import load


def show(dec, p: Params):
    print("  =>", dec.text)
    for i, tr in enumerate(dec.tokens, 1):
        mark = "?" if tr.margin < p.ambiguous else " "
        alts = "  ".join(f"[{k}] {c.text}" for k, c in enumerate(tr.ranked[:3], 1))
        print(f"   {mark}{i:>2} {tr.keys:<14} {alts}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--params")
    ap.add_argument("--langs", default="en,ko,ja", help="켤 언어 (예: en,ko)")
    a = ap.parse_args()
    p = Params()
    if a.params:
        d = json.load(open(a.params))
        p = Params(**{f.name: d[f.name] for f in fields(Params) if f.name in d})
    det = Detector(load(), p, UserHistory(), langs=tuple(a.langs.split(",")))
    last = None
    print("키 입력을 넣으세요 (:q 종료). ? 표시는 모호한 토큰입니다.")
    while True:
        try:
            line = input("> ")
        except EOFError:
            break
        if line == ":q":
            break
        if line.startswith(":") and last:
            choice = {}
            if line.strip() != ":ok":
                n, k = map(int, line[1:].split())
                choice[n] = k
            for i, tr in enumerate(last.tokens, 1):
                c = tr.ranked[choice[i] - 1] if i in choice else tr.chosen
                det.h.record(tr.keys, c.form)
            det._memo.clear()
            print("  확정·학습했습니다.")
            last = None
            continue
        last = det.decide(line)
        show(last, p)


if __name__ == "__main__":
    main()
