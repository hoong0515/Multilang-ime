import pytest

from ime_v0.hangul import compose, to_keys


@pytest.mark.parametrize("keys, text", [
    ("dkssudgktpdy", "안녕하세요"),
    ("rkqt", "값"),
    ("rkqtdl", "값이"),
    ("dhkswjs", "완전"),
    ("Tkd", "쌍"),
    ("zzz", "ㅋㅋㅋ"),
    ("bb", "ㅠㅠ"),
    ("gks", "한"),
    ("gksrmf", "한글"),
    ("dmlrmf", "의글"),   # ㅡ+ㅣ 결합 + 받침 이동
    ("ekfr", "닭"),
    ("ekfrdms", "닭은"),
    ("APIfmf", "몌ㅑ를"),   # Shift+P = ㅖ
])
def test_compose(keys, text):
    assert compose(keys).text == text


def test_hello_is_broken_korean():
    r = compose("hello")
    assert r.text == "ㅗ디ㅣㅐ"
    assert r.syllables == 1 and r.orphans == 3


@pytest.mark.parametrize("text", ["안녕하세요", "값이", "완전", "닭은", "쌍쌍바", "ㅋㅋㅋ", "ㅠㅠ", "읽었다", "괜찮아", "뷁"])
def test_roundtrip(text):
    assert compose(to_keys(text)).text == text
