import pytest

from ime_v0.romaji import parse, to_romaji


@pytest.mark.parametrize("kana, keys", [
    ("きょう", "kyou"),
    ("こんにちは", "konnnichiha"),
    ("しんぶん", "shinbunn"),
    ("きって", "kitte"),
    ("まっちゃ", "maccha"),
    ("コーヒー", "ko-hi-"),
    ("ほんや", "honnya"),
    ("ふぁいる", "fairu"),
    ("を", "wo"),
    ("あっ", "axtu"),
])
def test_to_romaji(kana, keys):
    assert to_romaji(kana) == keys


@pytest.mark.parametrize("keys, kana", [
    ("kyou", "きょう"),
    ("konnnichiha", "こんにちは"),
    ("konnichiha", "こんいちは"),   # IME와 같은 동작: nn이 아니면 n+i
    ("shinbun", "しんぶん"),
    ("kitte", "きって"),
    ("sinbunn", "しんぶん"),
    ("ko-hi-", "こーひー"),
    ("tukutta", "つくった"),
])
def test_parse(keys, kana):
    assert parse(keys).kana == kana
    assert parse(keys).leftover == 0


def test_english_leaves_leftovers():
    p = parse("strength")
    assert p.leftover > 0


@pytest.mark.parametrize("kana", ["きょうはいいてんきですね", "がっこうにいった", "こんばんは", "ちょっとまって", "しんかんせん"])
def test_roundtrip(kana):
    assert parse(to_romaji(kana)).kana == kana
