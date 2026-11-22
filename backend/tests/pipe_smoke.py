"""백엔드 파이프 연결 시험: 연결 하나를 유지하며 키를 보내고 조합·확정 결과를 확인한다.

사용: (백엔드 실행 중) python backend/tests/pipe_smoke.py
"""
import ctypes
import json
import os
import statistics
import time
from ctypes import wintypes

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.CreateFileW.restype = wintypes.HANDLE
PIPE = r"\\.\pipe\multilang-ime." + "".join(c if c.isalnum() else "_" for c in os.environ.get("USERNAME", ""))


class Pipe:
    def __init__(self):
        self.h = k32.CreateFileW(PIPE, 0xC0000000, 0, None, 3, 0, None)
        if self.h in (None, wintypes.HANDLE(-1).value):
            raise ctypes.WinError(ctypes.get_last_error())
        mode = wintypes.DWORD(2)  # PIPE_READMODE_MESSAGE
        k32.SetNamedPipeHandleState(self.h, ctypes.byref(mode), None, None)

    def call(self, req: dict) -> dict:
        body = json.dumps(req).encode()
        out = ctypes.create_string_buffer(65536)
        n = wintypes.DWORD()
        if not k32.TransactNamedPipe(self.h, body, len(body), out, len(out), ctypes.byref(n), None):
            raise ctypes.WinError(ctypes.get_last_error())
        return json.loads(out.raw[:n.value])


def key(ch, **kw):
    vk = {" ": 0x20, "\n": 0x0D, "\b": 0x08, "\x1b": 0x1B, ".": 0xBE}.get(ch, ord(ch.upper()))
    return {"vk": vk, "shift": ch.isupper(), "caps": False, "ctrl": False, "alt": False, **kw}


def type_text(p: Pipe, text: str):
    committed, last = "", {}
    for ch in text:
        t = p.call({"op": "test", "key": key(ch)})
        if t["flush_first"]:
            committed += p.call({"op": "flush"}).get("commit") or ""
            committed += ch  # 키는 앱으로
            continue
        if not t["eaten"]:
            committed += ch
            continue
        last = p.call({"op": "key", "key": key(ch)})
        committed += last.get("commit") or ""
    return committed, "".join(s["text"] for s in last.get("preedit", []))


def main():
    p = Pipe()
    print(type_text(p, "dkssudgktpdy React dptj APIfmf tkdydgo."))
    print(type_text(p, "dhsmfdms skfTlrk whgek\n"))
    print(type_text(p, "gks\x1b"))  # Esc: 친 키 그대로
    p.call({"op": "reset"})
    lat = []
    for ch in "dkssudgktpdy" * 20:
        t = time.perf_counter()
        p.call({"op": "test", "key": key(ch)})
        p.call({"op": "key", "key": key(ch)})
        lat.append((time.perf_counter() - t) * 1000)
    p.call({"op": "reset"})
    print(f"key round trip (test+key): median {statistics.median(lat):.2f} ms, max {max(lat):.2f} ms")


if __name__ == "__main__":
    main()
