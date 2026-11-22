"""공개 데이터(data/public/raw)로 평가셋을 만든다 -> data/public/<이름>.jsonl

일본어는 두 가지로 만든다.
  <이름>.jsonl           문장 전체를 한 번에 친 경우
  <이름>_bunsetsu.jsonl  문절(文節) 단위로 끊어 치고 변환한 경우. 문절 사이는 '|'로 나눈다
                         (조사·조동사·접미사는 앞 문절에 붙인다)
"""
import bz2
import json
import random
import sys
import unicodedata
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from extract_tweets import KANA, foreign_for_ja, foreign_for_koen, gold_of, is_ja_char, label_ja, label_koen  # noqa: E402
from ime_v0.hangul import is_hangul  # noqa: E402
from ime_v0.romaji import kata_to_hira, to_romaji  # noqa: E402

RAW = Path("data/public/raw")
OUT = Path("data/public")
PER_SOURCE = 4000
ATTACH = {"助詞", "助動詞", "接尾辞"}


def leipzig(name):
    p = RAW / name / f"{name}-sentences.txt"
    return [l.split("\t", 1)[1].strip() for l in p.read_text(encoding="utf-8").splitlines() if "\t" in l]


def tatoeba(name):
    with bz2.open(RAW / name, "rt", encoding="utf-8") as fh:
        return [l.rstrip("\n").split("\t")[2] for l in fh if l.count("\t") >= 2]


def nsmc():
    lines = (RAW / "nsmc_ratings_test.txt").read_text(encoding="utf-8").splitlines()[1:]
    return [l.split("\t")[1] for l in lines if l.count("\t") == 2 and l.split("\t")[1].strip()]


SOURCES = {
    "ko_news": ("koen", lambda: leipzig("kor_news_2022_10K")),
    "ko_wiki": ("koen", lambda: leipzig("kor_wikipedia_2021_10K")),
    "ko_web": ("koen", lambda: leipzig("kor-kr_web_2015_10K")),
    "ko_review": ("koen", nsmc),
    "ko_tatoeba": ("koen", lambda: tatoeba("kor_sentences.tsv.bz2")),
    "en_news": ("koen", lambda: leipzig("eng_news_2023_10K")),
    "ja_news": ("ja", lambda: leipzig("jpn_news_2020_10K")),
    "ja_wiki": ("ja", lambda: leipzig("jpn_wikipedia_2021_10K")),
    "ja_tatoeba": ("ja", lambda: tatoeba("jpn_sentences.tsv.bz2")),
}


def label_ja_bunsetsu(tagger, text):
    """문절 단위 키 입력과 라벨. 문절 경계에 '|'를 넣는다."""
    bunsetsu: list[list[tuple[str, str]]] = []  # [(종류, 텍스트)]: ja=가나, en/raw=그대로
    for w in tagger(text):
        s = w.surface
        pos = w.feature.pos1
        if all(is_ja_char(c) for c in s):
            k = w.feature.kana
            if not k or k == "*":
                if KANA.search(s) and all(KANA.match(c) or c == "ー" for c in s):
                    k = s
                else:
                    return None
            piece = ("ja", kata_to_hira(k))
        elif s.isascii() and s.isalpha():
            piece = ("en", s)
        else:
            if any(is_ja_char(c) for c in s):  # 가나·한자가 기호와 한 토큰에 묶인 경우 (二、三塁 등): 읽기를 알 수 없음
                return None
            piece = ("raw", s)
        if w.white_space and bunsetsu:
            bunsetsu[-1].append(("raw", w.white_space))
        if not bunsetsu or (pos not in ATTACH and piece[0] != "raw"):
            bunsetsu.append([piece])
        else:
            bunsetsu[-1].append(piece)
    keys, labels = [], []
    for bi, b in enumerate(bunsetsu):
        if bi:
            keys.append("|")
            labels.append("-")
        i = 0
        while i < len(b):
            kind, t = b[i]
            if kind == "ja":  # 이어진 가나는 한꺼번에 로마자로 (ん·っ 문맥)
                kana = t
                while i + 1 < len(b) and b[i + 1][0] == "ja":
                    i += 1
                    kana += b[i][1]
                r = to_romaji(kana)
                keys.append(r)
                labels += ["ja" if c.isalpha() else "-" for c in r]
            else:
                keys.append(t)
                labels += ["en" if c.isascii() and c.isalpha() else "-" for c in t]
            i += 1
    return "".join(keys), labels


def main():
    import fugashi

    tagger = fugashi.Tagger()
    rnd = random.Random(0)
    for name, (kind, loader) in SOURCES.items():
        sents = loader()
        rnd.shuffle(sents)
        rows, rows_b = [], []
        for i, text in enumerate(sents):
            if len(rows) >= PER_SOURCE:
                break
            text = " ".join(text.split())
            if kind == "ja":
                text = unicodedata.normalize("NFKC", text)
                if not KANA.search(text) or any(is_hangul(c) for c in text) or foreign_for_ja(text):
                    continue
                res = label_ja(tagger, text)
                res_b = label_ja_bunsetsu(tagger, text)
                if res is None or res_b is None:
                    continue
            else:
                if foreign_for_koen(text):
                    continue
                res = label_koen(text)
                res_b = None
            gold = gold_of(*res)
            if not gold:
                continue
            rid = f"{name}:{i}"
            rows.append({"id": rid, "text": text, "keys": res[0], "gold": gold})
            if res_b:
                rows_b.append({"id": rid, "text": text, "keys": res_b[0], "gold": gold_of(*res_b)})
        for suffix, rs in (("", rows), ("_bunsetsu", rows_b)):
            if not rs:
                continue
            p = OUT / f"{name}{suffix}.jsonl"
            with open(p, "w", encoding="utf-8") as fh:
                for r in rs:
                    fh.write(json.dumps(r, ensure_ascii=False) + "\n")
            print(f"{p.name}: {len(rs)} sentences, {sum(len(r['gold']) for r in rs)} tokens")


if __name__ == "__main__":
    main()
