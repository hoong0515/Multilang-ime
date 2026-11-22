"""Mozc 연동 개념 검증 시나리오. mozc_server가 떠 있어야 한다 (없으면 띄운다).

사용: python poc/mozc/run_poc.py
"""
import statistics
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mozc_client import C, MozcClient, candidates, launch_server, preedit_text, profile_dir, segments  # noqa: E402


def ensure_server() -> MozcClient:
    try:
        c = MozcClient()
        c.create_session()
        return c
    except (FileNotFoundError, OSError):
        launch_server()
        for _ in range(50):
            try:
                profile_dir()
                break
            except FileNotFoundError:
                time.sleep(0.2)
        c = MozcClient()
        c.create_session()
        return c


def new_session(c: MozcClient):
    c.create_session()
    c.command(C.SessionCommand.TURN_ON_IME, composition_mode=C.HIRAGANA)


def convert_kana(c: MozcClient, kana: str):
    c.set_composition(kana)
    return c.convert()


def main():
    c = ensure_server()
    print(f"server protocol {c.protocol_version}, version {c.product_version}")

    # 1. 가나 통째로 넣고 변환
    new_session(c)
    o = convert_kana(c, "きょうはいいてんきです")
    print("\n[1] UPDATE_COMPOSITION + 변환:", segments(o))

    # 2. 구간 이동과 각 구간 후보
    o = c.send_key(C.KeyEvent(special_key=C.KeyEvent.RIGHT))
    print("[2] 오른쪽 구간 후보:", [v for _, v in candidates(o, 5)])
    o = c.send_key(C.KeyEvent(special_key=C.KeyEvent.LEFT))
    o = c.send_key(C.KeyEvent(special_key=C.KeyEvent.RIGHT, modifier_keys=[C.KeyEvent.SHIFT]))
    print("    Shift+→ 로 첫 구간 늘리기:", segments(o))
    c.command(C.SessionCommand.REVERT)

    # 3. 학습: きしゃ -> 2위 이하 '汽車'를 골라 확정한 뒤 다시 변환
    new_session(c)
    o = convert_kana(c, "きしゃ")
    o = c.convert()  # 후보창 열기
    before = [v for _, v in candidates(o, 5)]
    target = next(i for i, v in candidates(o, 20) if v == "汽車")
    c.select(target)
    r = c.submit()
    new_session(c)
    after = segments(convert_kana(c, "きしゃ"))
    c.command(C.SessionCommand.REVERT)
    print(f"\n[3] 학습 전 후보: {before}\n    '汽車' 선택·확정 결과: {r.result.value!r}\n    다시 변환한 1위: {after}")

    # 4. 여러 읽기를 확률과 함께 (손글씨용 기능)
    new_session(c)
    evs = [C.SessionCommand.CompositionEvent(composition_string="はし", probability=0.6),
           C.SessionCommand.CompositionEvent(composition_string="はじ", probability=0.4)]
    o = c.command(C.SessionCommand.UPDATE_COMPOSITION, composition_events=evs)
    print("\n[4] 여러 읽기 조합:", preedit_text(o), "| 예측 후보:", [v for _, v in candidates(o, 8)])
    o = c.convert()
    o = c.convert()
    print("    변환 후보:", [v for _, v in candidates(o, 10)])
    c.command(C.SessionCommand.REVERT)

    # 5. 라이브 표시용: 변환 키 없이 조합만 했을 때 나오는 예측 후보
    new_session(c)
    o = c.type_romaji("kyouhaiitennki")
    print("\n[5] 변환 전 예측(suggestion) 후보:", [v for _, v in candidates(o, 5)])
    c.command(C.SessionCommand.REVERT)

    # 6. 지연 시간
    lat_set, lat_conv, lat_key = [], [], []
    for kana in ["きょうはいいてんきです", "あしたはあめがふる", "かいぎしつをよやくした", "にほんごをべんきょうする"] * 10:
        new_session(c)
        t = time.perf_counter(); c.set_composition(kana); lat_set.append(time.perf_counter() - t)
        t = time.perf_counter(); c.convert(); lat_conv.append(time.perf_counter() - t)
        c.command(C.SessionCommand.REVERT)
    new_session(c)
    for ch in "watashihanihongowobenkyoushiteimasu":
        t = time.perf_counter(); c.send_key(C.KeyEvent(key_code=ord(ch))); lat_key.append(time.perf_counter() - t)
    c.command(C.SessionCommand.REVERT)
    ms = lambda xs: f"중앙값 {statistics.median(xs) * 1000:.1f} ms, 최대 {max(xs) * 1000:.1f} ms"  # noqa: E731
    print(f"\n[6] 지연: 조합 교체 {ms(lat_set)} / 변환 {ms(lat_conv)} / 키 1개 {ms(lat_key)}")


if __name__ == "__main__":
    main()
