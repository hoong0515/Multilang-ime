"""두벌식 오토마타: QWERTY 키 입력 <-> 한글.

compose(keys)    키 입력 문자열을 두벌식으로 조합한 결과와 통계를 돌려준다.
to_keys(text)    한글 텍스트를 두벌식 키 입력으로 되돌린다 (평가 데이터 생성용).
"""
from __future__ import annotations

from dataclasses import dataclass

CHO = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ"
JUNG = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ"
JONG = ["", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ",
        "ㄿ", "ㅀ", "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ"]

KEY_TO_JAMO = {
    "r": "ㄱ", "R": "ㄲ", "s": "ㄴ", "e": "ㄷ", "E": "ㄸ", "f": "ㄹ", "a": "ㅁ",
    "q": "ㅂ", "Q": "ㅃ", "t": "ㅅ", "T": "ㅆ", "d": "ㅇ", "w": "ㅈ", "W": "ㅉ",
    "c": "ㅊ", "z": "ㅋ", "x": "ㅌ", "v": "ㅍ", "g": "ㅎ",
    "k": "ㅏ", "o": "ㅐ", "O": "ㅒ", "i": "ㅑ", "j": "ㅓ", "p": "ㅔ", "P": "ㅖ",
    "u": "ㅕ", "h": "ㅗ", "y": "ㅛ", "n": "ㅜ", "b": "ㅠ", "m": "ㅡ", "l": "ㅣ",
}
# Shift가 의미 없는 대문자는 소문자와 같은 자모
for _k in "abcdfghijklmnsuvxyz":
    KEY_TO_JAMO.setdefault(_k.upper(), KEY_TO_JAMO[_k])

JAMO_TO_KEY = {}
for _k, _j in KEY_TO_JAMO.items():
    if _k.islower() or _j not in JAMO_TO_KEY:
        JAMO_TO_KEY.setdefault(_j, _k)
for _j in "ㄲㄸㅃㅆㅉㅒㅖ":
    JAMO_TO_KEY[_j] = {"ㄲ": "R", "ㄸ": "E", "ㅃ": "Q", "ㅆ": "T", "ㅉ": "W", "ㅒ": "O", "ㅖ": "P"}[_j]

VOWEL_COMB = {("ㅗ", "ㅏ"): "ㅘ", ("ㅗ", "ㅐ"): "ㅙ", ("ㅗ", "ㅣ"): "ㅚ", ("ㅜ", "ㅓ"): "ㅝ",
              ("ㅜ", "ㅔ"): "ㅞ", ("ㅜ", "ㅣ"): "ㅟ", ("ㅡ", "ㅣ"): "ㅢ"}
FINAL_COMB = {("ㄱ", "ㅅ"): "ㄳ", ("ㄴ", "ㅈ"): "ㄵ", ("ㄴ", "ㅎ"): "ㄶ", ("ㄹ", "ㄱ"): "ㄺ",
              ("ㄹ", "ㅁ"): "ㄻ", ("ㄹ", "ㅂ"): "ㄼ", ("ㄹ", "ㅅ"): "ㄽ", ("ㄹ", "ㅌ"): "ㄾ",
              ("ㄹ", "ㅍ"): "ㄿ", ("ㄹ", "ㅎ"): "ㅀ", ("ㅂ", "ㅅ"): "ㅄ"}
VOWEL_SPLIT = {v: k for k, v in VOWEL_COMB.items()}
FINAL_SPLIT = {v: k for k, v in FINAL_COMB.items()}

CONSONANTS = set(CHO) | set(FINAL_SPLIT)
VOWELS = set(JUNG)


def is_vowel(j: str) -> bool:
    return j in VOWELS


def syllable(cho: str, jung: str, jong: str = "") -> str:
    return chr(0xAC00 + (CHO.index(cho) * 21 + JUNG.index(jung)) * 28 + JONG.index(jong))


@dataclass
class Composed:
    text: str
    syllables: int = 0          # 완성된 음절 수
    orphan_cons: int = 0        # 모음 없이 남은 자음 수 (ㅋㅋ 등)
    orphan_vowels: int = 0      # 자음 없이 남은 모음 수 (ㅠㅠ 등)
    other: int = 0              # 자모가 아닌 문자 수
    orphan_list: str = ""       # 낱자모로 남은 글자들

    @property
    def orphans(self) -> int:
        return self.orphan_cons + self.orphan_vowels


def compose(keys: str) -> Composed:
    out: list[str] = []
    res = Composed("")
    cho = jung = jong = ""

    def flush():
        nonlocal cho, jung, jong
        if cho and jung:
            out.append(syllable(cho, jung, jong))
            res.syllables += 1
        elif cho:
            out.append(cho)
            res.orphan_cons += 1
            res.orphan_list += cho
        elif jung:
            out.append(jung)
            res.orphan_vowels += 1
            res.orphan_list += jung
        cho = jung = jong = ""

    for ch in keys:
        j = KEY_TO_JAMO.get(ch)
        if j is None:
            flush()
            out.append(ch)
            res.other += 1
            continue
        if not is_vowel(j):
            if jong:
                comb = FINAL_COMB.get((jong, j))
                if comb:
                    jong = comb
                else:
                    flush()
                    cho = j
            elif cho and jung and j in JONG:
                jong = j
            else:
                flush()
                cho = j
        else:
            if jong:
                if jong in FINAL_SPLIT:
                    keep, move = FINAL_SPLIT[jong]
                    jong = keep
                else:
                    move, jong = jong, ""
                flush()
                cho, jung = move, j
            elif jung:
                comb = VOWEL_COMB.get((jung, j))
                if comb:
                    jung = comb
                else:
                    flush()
                    jung = j
            else:
                jung = j  # cho만 있거나 빈 상태
    flush()
    res.text = "".join(out)
    return res


def decompose_char(ch: str) -> list[str]:
    """한 글자를 키 입력 순서의 자모 목록으로."""
    code = ord(ch) - 0xAC00
    if 0 <= code < 11172:
        cho, rest = divmod(code, 21 * 28)
        jung, jong = divmod(rest, 28)
        parts = [CHO[cho]]
        v = JUNG[jung]
        parts += list(VOWEL_SPLIT.get(v, (v,)))
        f = JONG[jong]
        if f:
            parts += list(FINAL_SPLIT.get(f, (f,)))
        return parts
    if ch in VOWEL_SPLIT:
        return list(VOWEL_SPLIT[ch])
    if ch in FINAL_SPLIT:
        return list(FINAL_SPLIT[ch])
    if ch in JAMO_TO_KEY:
        return [ch]
    return []


def is_hangul(ch: str) -> bool:
    return 0xAC00 <= ord(ch) <= 0xD7A3 or 0x3131 <= ord(ch) <= 0x3163


def to_keys(text: str) -> str:
    out = []
    for ch in text:
        parts = decompose_char(ch)
        if parts:
            out.extend(JAMO_TO_KEY[p] for p in parts)
        else:
            out.append(ch)
    return "".join(out)
