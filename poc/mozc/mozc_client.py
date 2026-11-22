"""Mozc 연동 개념 검증: Windows에서 mozc_server를 띄우고 이름 있는 파이프로 직접 대화한다.

- 서버 실행: Mozc 서버는 샌드박스 런처가 만든 스레드 토큰이 없으면 실행을 거부한다(base/run_level.cc).
  그래서 일시 정지 상태로 띄운 뒤, 우리 프로세스 토큰을 가장(impersonation) 토큰으로 복제해
  메인 스레드에 붙이고 재개한다. (Mozc 클라이언트의 ServerLauncher가 하는 일을 흉내)
- 통신: 서버가 사용자 프로필 폴더에 쓴 session.ipc(IPCPathInfo)에서 키를 읽고,
  \\\\.\\pipe\\mozc.<key>.session 에 메시지 모드로 붙어 commands.Input을 보내고 commands.Output을 받는다.
  요청 하나마다 연결 하나 (src/ipc/win32_ipc.cc).
"""
from __future__ import annotations

import ctypes
import os
import sys
import time
from ctypes import wintypes
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "gen"))
from ipc import ipc_pb2  # noqa: E402
from protocol import commands_pb2 as C  # noqa: E402

SERVER = Path(__file__).resolve().parents[2] / "third_party/mozc_bin/extract/PFiles/Mozc/mozc_server.exe"

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
adv = ctypes.WinDLL("advapi32", use_last_error=True)
k32.GetCurrentProcess.restype = wintypes.HANDLE
adv.OpenProcessToken.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE)]
adv.DuplicateTokenEx.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.c_void_p, ctypes.c_int, ctypes.c_int,
                                 ctypes.POINTER(wintypes.HANDLE)]
adv.SetThreadToken.argtypes = [ctypes.POINTER(wintypes.HANDLE), wintypes.HANDLE]
k32.ResumeThread.argtypes = [wintypes.HANDLE]
k32.CloseHandle.argtypes = [wintypes.HANDLE]


class STARTUPINFO(ctypes.Structure):
    _fields_ = [("cb", wintypes.DWORD), ("lpReserved", wintypes.LPWSTR), ("lpDesktop", wintypes.LPWSTR),
                ("lpTitle", wintypes.LPWSTR), ("dwX", wintypes.DWORD), ("dwY", wintypes.DWORD),
                ("dwXSize", wintypes.DWORD), ("dwYSize", wintypes.DWORD), ("dwXCountChars", wintypes.DWORD),
                ("dwYCountChars", wintypes.DWORD), ("dwFillAttribute", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
                ("wShowWindow", wintypes.WORD), ("cbReserved2", wintypes.WORD), ("lpReserved2", ctypes.c_void_p),
                ("hStdInput", wintypes.HANDLE), ("hStdOutput", wintypes.HANDLE), ("hStdError", wintypes.HANDLE)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", wintypes.HANDLE), ("hThread", wintypes.HANDLE),
                ("dwProcessId", wintypes.DWORD), ("dwThreadId", wintypes.DWORD)]


CREATE_SUSPENDED = 0x4
CREATE_NO_WINDOW = 0x08000000
TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_IMPERSONATE = 0x2, 0x8, 0x4
SecurityImpersonation, TokenImpersonation = 2, 2


def _check(ok, what):
    if not ok:
        raise ctypes.WinError(ctypes.get_last_error(), what)


def launch_server(exe: Path = SERVER) -> int:
    si = STARTUPINFO()
    si.cb = ctypes.sizeof(si)
    pi = PROCESS_INFORMATION()
    _check(k32.CreateProcessW(str(exe), None, None, None, False, CREATE_SUSPENDED | CREATE_NO_WINDOW,
                              None, str(exe.parent), ctypes.byref(si), ctypes.byref(pi)), "CreateProcessW")
    tok, imp = wintypes.HANDLE(), wintypes.HANDLE()
    _check(adv.OpenProcessToken(k32.GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, ctypes.byref(tok)),
           "OpenProcessToken")
    _check(adv.DuplicateTokenEx(tok, TOKEN_IMPERSONATE | TOKEN_QUERY, None, SecurityImpersonation,
                                TokenImpersonation, ctypes.byref(imp)), "DuplicateTokenEx")
    th = wintypes.HANDLE(pi.hThread)
    _check(adv.SetThreadToken(ctypes.byref(th), imp), "SetThreadToken")
    k32.ResumeThread(pi.hThread)
    for h in (tok, imp, pi.hThread, pi.hProcess):
        k32.CloseHandle(h)
    return pi.dwProcessId


def profile_dir() -> Path:
    """서버가 session.ipc를 쓰는 폴더 (LocalAppData 계열 + Mozc). 실제로 생긴 곳을 찾는다."""
    home = Path(os.environ["USERPROFILE"]) / "AppData"
    for base in (home / "LocalLow", home / "Local"):
        if (base / "Mozc" / "session.ipc").exists():
            return base / "Mozc"
    raise FileNotFoundError("session.ipc not found (server not running?)")


GENERIC_RW = 0xC0000000
OPEN_EXISTING = 3
PIPE_READMODE_MESSAGE = 2
ERROR_MORE_DATA = 234
INVALID_HANDLE = wintypes.HANDLE(-1).value
k32.CreateFileW.restype = wintypes.HANDLE


def read_shared(path: Path) -> bytes:
    """서버가 잡고 있는 파일을 Mozc 클라이언트와 같은 공유 플래그(읽기·쓰기·삭제)로 읽는다."""
    GENERIC_READ, SHARE_ALL = 0x80000000, 0x7
    h = k32.CreateFileW(str(path), GENERIC_READ, SHARE_ALL, None, OPEN_EXISTING, 0, None)
    if h in (None, INVALID_HANDLE):
        raise ctypes.WinError(ctypes.get_last_error(), f"open {path}")
    try:
        buf = ctypes.create_string_buffer(4096)
        n = wintypes.DWORD()
        _check(k32.ReadFile(h, buf, len(buf), ctypes.byref(n), None), "ReadFile")
        return buf.raw[:n.value]
    finally:
        k32.CloseHandle(h)


class MozcClient:
    def __init__(self):
        info = ipc_pb2.IPCPathInfo()
        info.ParseFromString(read_shared(profile_dir() / "session.ipc"))
        self.pipe = rf"\\.\pipe\mozc.{info.key}.session"
        self.protocol_version = info.protocol_version
        self.product_version = info.product_version
        self.session_id = None

    def call(self, inp: C.Input) -> C.Output:
        for _ in range(50):
            h = k32.CreateFileW(self.pipe, GENERIC_RW, 0, None, OPEN_EXISTING, 0, None)
            if h not in (None, INVALID_HANDLE):
                break
            err = ctypes.get_last_error()
            if err != 231:  # ERROR_PIPE_BUSY: 서버가 직전 연결의 ACK를 기다리는 중
                raise ctypes.WinError(err, "CreateFileW pipe")
            k32.WaitNamedPipeW(self.pipe, 1000)
        else:
            raise TimeoutError("pipe busy")
        try:
            mode = wintypes.DWORD(PIPE_READMODE_MESSAGE)
            k32.SetNamedPipeHandleState(h, ctypes.byref(mode), None, None)
            data = inp.SerializeToString()
            n = wintypes.DWORD()
            _check(k32.WriteFile(h, data, len(data), ctypes.byref(n), None), "WriteFile")
            chunks = []
            buf = ctypes.create_string_buffer(65536)
            while True:
                ok = k32.ReadFile(h, buf, len(buf), ctypes.byref(n), None)
                chunks.append(buf.raw[:n.value])
                if ok:
                    break
                if ctypes.get_last_error() != ERROR_MORE_DATA:
                    raise ctypes.WinError(ctypes.get_last_error(), "ReadFile")
        finally:
            k32.CloseHandle(h)  # 연결을 끊는 것이 서버에 대한 ACK
        out = C.Output()
        out.ParseFromString(b"".join(chunks))
        return out

    # 세션 명령 ---------------------------------------------------------
    def create_session(self) -> int:
        out = self.call(C.Input(type=C.Input.CREATE_SESSION))
        self.session_id = out.id
        return out.id

    def delete_session(self):
        self.call(C.Input(type=C.Input.DELETE_SESSION, id=self.session_id))

    def send_key(self, key: C.KeyEvent) -> C.Output:
        return self.call(C.Input(type=C.Input.SEND_KEY, id=self.session_id, key=key))

    def type_romaji(self, text: str) -> C.Output:
        out = None
        for ch in text:
            out = self.send_key(C.KeyEvent(key_code=ord(ch)))
        return out

    def command(self, ctype, **kw) -> C.Output:
        return self.call(C.Input(type=C.Input.SEND_COMMAND, id=self.session_id,
                                 command=C.SessionCommand(type=ctype, **kw)))

    def set_composition(self, kana: str) -> C.Output:
        ev = C.SessionCommand.CompositionEvent(composition_string=kana, probability=1.0)
        return self.command(C.SessionCommand.UPDATE_COMPOSITION, composition_events=[ev])

    def convert(self) -> C.Output:
        return self.send_key(C.KeyEvent(special_key=C.KeyEvent.SPACE))

    def submit(self) -> C.Output:
        return self.command(C.SessionCommand.SUBMIT)

    def select(self, cand_id: int) -> C.Output:
        return self.command(C.SessionCommand.SELECT_CANDIDATE, id=cand_id)


def preedit_text(out: C.Output) -> str:
    return "".join(s.value for s in out.preedit.segment)


def segments(out: C.Output) -> list[str]:
    return [s.value for s in out.preedit.segment]


def candidates(out: C.Output, n: int = 9) -> list[tuple[int, str]]:
    src = out.all_candidate_words if out.all_candidate_words.candidates else None
    if src is not None:
        return [(c.id, c.value) for c in src.candidates[:n]]
    return [(c.id, c.value) for c in out.candidate_window.candidate[:n]]
