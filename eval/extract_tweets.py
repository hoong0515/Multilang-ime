"""X 아카이브(tweets.js)에서 평가 데이터를 뽑는다.

사용: python -I eval/extract_tweets.py <tweets.js 경로> <출력 디렉터리>
  -> <출력>/tweets_koen.jsonl  한글·영어만 있는 트윗
  -> <출력>/tweets_ja.jsonl    가나가 있고 한글이 없는 트윗 (한자는 fugashi로 읽기를 구해 로마자로)

- 본인이 쓴 트윗만 (RT 제외), URL·멘션·해시태그 제거
- 각 트윗은 실제로 쳤을 키 입력(keys)과, 알파벳 덩어리마다 정답 언어 구간(gold)을 가진다
- 출력은 개인 데이터이므로 git에 올리지 않는 data/private/ 에 둔다
"""
import html
import json
import re
import sys
import unicodedata
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from ime_v0.hangul import is_hangul, to_keys  # noqa: E402
from ime_v0.models import japanese_reading  # noqa: E402
from ime_v0.romaji import to_romaji  # noqa: E402

URL = re.compile(r"https?://\S+")
MENTION = re.compile(r"[@#][\w가-힣ぁ-んァ-ヶー一-龯]+")
KANA = re.compile(r"[ぁ-ゟァ-ヺ]")
# 사람이 직접 친 트윗만: 트위터 클라이언트 앱 (게임·공유 버튼 자동 게시 제외)
CLIENTS = {"Twitter for Android", "Twitter Web Client", "Twitter Web App", "tweecha OLD", "iOS", "Twitter for iPad",
           "Twitter for Windows", "Twitter for iPhone", "MOTOBLUR", "TweetDeck", "Mobile Web (M2)", "twtkr for Android",
           "ツイッターするやつ", "트윗매니저"}
SOURCE_TAG = re.compile(r"<[^>]+>")
JA_RUN = re.compile(r"[ぁ-ゟ゠-ヿ一-鿿々〆ヶー]+")


def is_ja_char(c: str) -> bool:
    return bool(JA_RUN.fullmatch(c))


def foreign_for_koen(text: str) -> bool:
    # 한글·ASCII 알파벳 외의 글자(가나, 한자, è 같은 악센트 문자 등)가 있으면 제외
    return any(c.isalpha() and not is_hangul(c) and not c.isascii() for c in text)


def foreign_for_ja(text: str) -> bool:
    return any(c.isalpha() and not c.isascii() and not is_ja_char(c) for c in text)


def label_koen(text: str):
    keys, labels = [], []
    for ch in text:
        if is_hangul(ch):
            k = to_keys(ch)
            keys.append(k)
            labels += ["ko"] * len(k)
        else:
            keys.append(ch)
            labels.append("en" if ch.isascii() and ch.isalpha() else "-")
    return "".join(keys), labels


def label_ja(tagger, text: str):
    keys, labels = [], []
    pos = 0
    for m in JA_RUN.finditer(text):
        for ch in text[pos:m.start()]:
            keys.append(ch)
            labels.append("en" if ch.isascii() and ch.isalpha() else "-")
        reading = japanese_reading(tagger, m.group())
        if reading is None:
            return None
        r = to_romaji(reading)
        keys.append(r)
        labels += ["ja" if c.isalpha() else "-" for c in r]
        pos = m.end()
    for ch in text[pos:]:
        keys.append(ch)
        labels.append("en" if ch.isascii() and ch.isalpha() else "-")
    return "".join(keys), labels


def gold_of(keys: str, labels):
    """알파벳 덩어리마다 [[언어, 키 수], ...]."""
    gold, cur = [], []
    for ch, lab in zip(keys, labels):
        if ch.isascii() and ch.isalpha():
            if cur and cur[-1][0] == lab:
                cur[-1][1] += 1
            else:
                cur.append([lab, 1])
        elif cur:
            gold.append(cur)
            cur = []
    if cur:
        gold.append(cur)
    return gold


def main(src: str, dst_dir: str):
    import fugashi

    tagger = fugashi.Tagger()
    raw = Path(src).read_text(encoding="utf-8")
    data = json.loads(raw[raw.index("["):])
    outs = {"koen": [], "ja": []}
    for item in data:
        t = item["tweet"]
        text = t["full_text"]
        if text.startswith("RT @") or SOURCE_TAG.sub("", t["source"]) not in CLIENTS:
            continue
        text = html.unescape(URL.sub(" ", text))
        text = MENTION.sub(" ", text)
        text = " ".join(text.split())
        if not any(c.isascii() and c.isalpha() for c in text) and not any(is_hangul(c) or is_ja_char(c) for c in text):
            continue
        if KANA.search(text) and not any(is_hangul(c) for c in text):
            # 전각 영숫자·반각 가나 정리 (한글 낱자모가 바뀌므로 일본어 쪽에만 적용)
            text = unicodedata.normalize("NFKC", text)
            if foreign_for_ja(text):
                continue
            res = label_ja(tagger, text)
            if res is None:
                continue
            kind = "ja"
        elif not foreign_for_koen(text):
            res = label_koen(text)
            kind = "koen"
        else:
            continue
        keys, labels = res
        gold = gold_of(keys, labels)
        if not gold:
            continue
        outs[kind].append({"id": t["id_str"], "date": t["created_at"], "text": text, "keys": keys, "gold": gold})
    Path(dst_dir).mkdir(parents=True, exist_ok=True)
    for kind, rows in outs.items():
        dst = Path(dst_dir) / f"tweets_{kind}.jsonl"
        with open(dst, "w", encoding="utf-8") as fh:
            for row in rows:
                fh.write(json.dumps(row, ensure_ascii=False) + "\n")
        print(f"{len(rows)} tweets -> {dst}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
