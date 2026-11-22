"""IME 방식 로마자 입력 <-> 히라가나.

to_romaji(kana)  가나를 사람이 IME에 칠 법한 로마자 키 입력으로 (평가·학습 데이터 생성용)
parse(keys)      로마자 키 입력을 IME처럼 가나로 조합하고, 가나가 안 된 글자 수를 센다
"""
from __future__ import annotations

from dataclasses import dataclass

_BASE = """
あa いi うu えe おo かka きki くku けke こko さsa しshi すsu せse そso たta ちchi つtsu てte とto
なna にni ぬnu ねne のno はha ひhi ふfu へhe ほho まma みmi むmu めme もmo やya ゆyu よyo
らra りri るru れre ろro わwa をwo がga ぎgi ぐgu げge ごgo ざza じji ずzu ぜze ぞzo
だda ぢdi づdu でde どdo ばba びbi ぶbu べbe ぼbo ぱpa ぴpi ぷpu ぺpe ぽpo ゔvu
きゃkya きゅkyu きょkyo しゃsha しゅshu しょsho ちゃcha ちゅchu ちょcho にゃnya にゅnyu にょnyo
ひゃhya ひゅhyu ひょhyo みゃmya みゅmyu みょmyo りゃrya りゅryu りょryo ぎゃgya ぎゅgyu ぎょgyo
じゃja じゅju じょjo びゃbya びゅbyu びょbyo ぴゃpya ぴゅpyu ぴょpyo ぢゃdya ぢゅdyu ぢょdyo
しぇshe ちぇche じぇje ふぁfa ふぃfi ふぇfe ふぉfo てぃthi でぃdhi でゅdhu うぃwi うぇwe うぉwho
ゔぁva ゔぃvi ゔぇve ゔぉvo つぁtsa
ぁxa ぃxi ぅxu ぇxe ぉxo ゃxya ゅxyu ょxyo ゎxwa ゐwi ゑwe
"""
KANA_TO_ROMAJI: dict[str, str] = {}
for _item in _BASE.split():
    _k = "".join(c for c in _item if not c.isascii())
    KANA_TO_ROMAJI[_k] = _item[len(_k):]

# 파싱용: 대체 철자도 받는다
ROMAJI_TO_KANA: dict[str, str] = {v: k for k, v in KANA_TO_ROMAJI.items()}
ROMAJI_TO_KANA.update({
    "si": "し", "ti": "ち", "tu": "つ", "hu": "ふ", "zi": "じ", "di": "ぢ", "du": "づ",
    "sya": "しゃ", "syu": "しゅ", "syo": "しょ", "tya": "ちゃ", "tyu": "ちゅ", "tyo": "ちょ",
    "cya": "ちゃ", "cyu": "ちゅ", "cyo": "ちょ", "jya": "じゃ", "jyu": "じゅ", "jyo": "じょ",
    "zya": "じゃ", "zyu": "じゅ", "zyo": "じょ", "la": "ぁ", "li": "ぃ", "lu": "ぅ", "le": "ぇ", "lo": "ぉ",
    "xtu": "っ", "ltu": "っ", "xtsu": "っ", "lya": "ゃ", "lyu": "ゅ", "lyo": "ょ", "nn": "ん", "n'": "ん",
    "ca": "か", "cu": "く", "co": "こ", "qa": "くぁ", "ye": "いぇ", "wu": "う",
})
MAX_LEN = max(map(len, ROMAJI_TO_KANA))
VOWELS = set("aiueo")


def kata_to_hira(s: str) -> str:
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


def to_romaji(kana: str) -> str:
    """히라가나·가타카나 -> 로마자. 가나가 아닌 글자는 그대로 둔다."""
    s = kata_to_hira(kana)
    out: list[str] = []
    i = 0
    pending_sokuon = False
    while i < len(s):
        c = s[i]
        if c == "っ":
            pending_sokuon = True
            i += 1
            continue
        piece = None
        for L in (2, 1):
            if s[i:i + L] in KANA_TO_ROMAJI:
                piece = KANA_TO_ROMAJI[s[i:i + L]]
                i += L
                break
        if piece is None:
            if c == "ん":
                nxt = ""
                if i + 1 < len(s):
                    for L in (2, 1):
                        if s[i + 1:i + 1 + L] in KANA_TO_ROMAJI:
                            nxt = KANA_TO_ROMAJI[s[i + 1:i + 1 + L]]
                            break
                piece = "nn" if (not nxt or nxt[0] in VOWELS or nxt[0] in "yn") else "n"
            elif c == "ー":
                piece = "-"
            else:
                piece = c
            i += 1
        if pending_sokuon:
            piece = (piece[0] + piece) if piece[0].isalpha() and piece[0] not in VOWELS else "xtu" + piece
            pending_sokuon = False
        out.append(piece)
    if pending_sokuon:
        out.append("xtu")
    return "".join(out)


@dataclass
class Parsed:
    kana: str
    leftover: int      # 가나가 되지 못한 로마자 글자 수


def parse(keys: str) -> Parsed:
    s = keys.lower()
    out: list[str] = []
    left = 0
    i = 0
    while i < len(s):
        c = s[i]
        if c == "-":
            out.append("ー")
            i += 1
            continue
        # 같은 자음 두 번 (n 제외) -> っ
        if i + 1 < len(s) and c == s[i + 1] and c.isalpha() and c not in VOWELS and c != "n":
            out.append("っ")
            i += 1
            continue
        if c == "n" and i + 1 < len(s) and s[i + 1] not in VOWELS and s[i + 1] not in "yn'":
            out.append("ん")
            i += 1
            continue
        for L in range(min(MAX_LEN, len(s) - i), 0, -1):
            k = ROMAJI_TO_KANA.get(s[i:i + L])
            if k:
                out.append(k)
                i += L
                break
        else:
            if c == "n" and i == len(s) - 1:
                out.append("ん")
            else:
                out.append(keys[i])
                left += 1
            i += 1
    return Parsed("".join(out), left)
