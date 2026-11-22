"""등록 없이 DLL을 직접 불러 COM 배관을 확인한다 (64비트 Python 전용).

DllGetClassObject -> IClassFactory::CreateInstance -> QueryInterface(ITfTextInputProcessorEx 등)
사용: python tip/tests/com_smoke.py [dll 경로]
"""
import ctypes
import sys
import uuid
from ctypes import POINTER, byref, c_void_p, wintypes
from pathlib import Path

DLL = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "target/release/multilang_tip.dll"

CLSID = "{05C49E35-3B3F-4BC3-8D48-EF1336F9CD4D}"
IIDS = {
    "IUnknown": "{00000000-0000-0000-C000-000000000046}",
    "IClassFactory": "{00000001-0000-0000-C000-000000000046}",
    "ITfTextInputProcessor": "{AA80E7F7-2021-11D2-93E0-0060B067B86E}",
    "ITfTextInputProcessorEx": "{6E4E2102-F9CD-433D-B496-303CE03A6507}",
    "ITfKeyEventSink": "{AA80E7F5-2021-11D2-93E0-0060B067B86E}",
    "ITfCompositionSink": "{A781718C-579A-4B15-A280-32B8577ACC5E}",
    "ITfDisplayAttributeProvider": "{FEE47777-163C-4769-996A-6E9C50AD8F54}",
    "ITfEditSession(없어야 함)": "{AA80E803-2021-11D2-93E0-0060B067B86E}",
}


class GUID(ctypes.Structure):
    _fields_ = [("data", ctypes.c_ubyte * 16)]

    @classmethod
    def parse(cls, s):
        g = cls()
        ctypes.memmove(g.data, uuid.UUID(s).bytes_le, 16)
        return g


def vcall(obj, index, restype, *args, argtypes=()):
    vtbl = ctypes.cast(obj, POINTER(POINTER(c_void_p))).contents
    fn = ctypes.WINFUNCTYPE(restype, c_void_p, *argtypes)(vtbl[index])
    return fn(obj, *args)


def qi(obj, iid):
    out = c_void_p()
    hr = vcall(obj, 0, ctypes.c_long, byref(GUID.parse(iid)), byref(out), argtypes=(POINTER(GUID), POINTER(c_void_p)))
    return hr, out


def release(obj):
    return vcall(obj, 2, wintypes.ULONG)


def main():
    dll = ctypes.WinDLL(str(DLL))
    for name in ("DllGetClassObject", "DllCanUnloadNow", "DllRegisterServer", "DllUnregisterServer"):
        print(f"export {name}: {'ok' if hasattr(dll, name) else 'MISSING'}")

    factory = c_void_p()
    hr = dll.DllGetClassObject(byref(GUID.parse(CLSID)), byref(GUID.parse(IIDS["IClassFactory"])), byref(factory))
    print(f"DllGetClassObject: hr=0x{hr & 0xFFFFFFFF:08X}")
    bad = c_void_p()
    hr_bad = dll.DllGetClassObject(byref(GUID.parse("{00000000-1111-2222-3333-444444444444}")),
                                   byref(GUID.parse(IIDS["IClassFactory"])), byref(bad))
    print(f"다른 CLSID 거절: hr=0x{hr_bad & 0xFFFFFFFF:08X} (CLASS_E_CLASSNOTAVAILABLE=0x80040111)")

    obj = c_void_p()
    hr = vcall(factory, 3, ctypes.c_long, None, byref(GUID.parse(IIDS["IUnknown"])), byref(obj),
               argtypes=(c_void_p, POINTER(GUID), POINTER(c_void_p)))
    print(f"CreateInstance: hr=0x{hr & 0xFFFFFFFF:08X}")
    for name, iid in IIDS.items():
        if name in ("IUnknown", "IClassFactory"):
            continue
        h, p = qi(obj, iid)
        print(f"  QI {name}: {'ok' if h == 0 else f'0x{h & 0xFFFFFFFF:08X}'}")
        if h == 0:
            release(p)
    print("Release ->", release(obj), "/ factory Release ->", release(factory))


if __name__ == "__main__":
    main()
