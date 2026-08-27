# Raises the "rustflutter Gallery" window topmost, moves the REAL cursor
# (SendInput) to client coords, waits, grabs the client pixels.
# usage: real_hover.py out.png x,y [settle_seconds]
import ctypes
import sys
import time
from ctypes import wintypes

import PIL.Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

try:
    user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
except Exception:
    user32.SetProcessDPIAware()

HWND_TOPMOST = -1
HWND_NOTOPMOST = -2
SWP_NOMOVE = 0x0002
SWP_NOSIZE = 0x0001
SWP_SHOWWINDOW = 0x0040
SRCCOPY = 0x00CC0020


def find_window(title):
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, lp):
        if user32.IsWindowVisible(hwnd):
            n = user32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(n + 1)
            user32.GetWindowTextW(hwnd, buf, n + 1)
            if buf.value == title:
                found.append(hwnd)
        return True

    user32.EnumWindows(cb, 0)
    return found[0]


def real_move(sx, sy):
    # physical pixels -> absolute mickeys across the virtual screen
    vx = user32.GetSystemMetrics(76)
    vy = user32.GetSystemMetrics(77)
    vw = user32.GetSystemMetrics(78)
    vh = user32.GetSystemMetrics(79)

    class MOUSEINPUT(ctypes.Structure):
        _fields_ = [("dx", ctypes.c_long), ("dy", ctypes.c_long), ("mouseData", ctypes.c_ulong),
                    ("dwFlags", ctypes.c_ulong), ("time", ctypes.c_ulong), ("dwExtraInfo", ctypes.c_void_p)]

    class INPUT(ctypes.Structure):
        _fields_ = [("type", ctypes.c_ulong), ("mi", MOUSEINPUT)]

    dx = int((sx - vx) * 65535 / (vw - 1))
    dy = int((sy - vy) * 65535 / (vh - 1))
    inp = INPUT(0, MOUSEINPUT(dx, dy, 0, 0x8000 | 0x4000 | 0x0001, 0, None))
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(inp))


def grab_client(hwnd, path):
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    w, h = rect.right - rect.left, rect.bottom - rect.top
    pt = wintypes.POINT()
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, w, h)
    gdi32.SelectObject(mem, bmp)
    gdi32.BitBlt(mem, 0, 0, w, h, screen, pt.x, pt.y, SRCCOPY)

    class BITMAPINFOHEADER(ctypes.Structure):
        _fields_ = [
            ("biSize", wintypes.DWORD), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
            ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD),
            ("biCompression", wintypes.DWORD), ("biSizeImage", wintypes.DWORD),
            ("biXPelsPerMeter", ctypes.c_long), ("biYPelsPerMeter", ctypes.c_long),
            ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD),
        ]

    class BITMAPINFO(ctypes.Structure):
        _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]

    bi = BITMAPINFO()
    bi.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bi.bmiHeader.biWidth = w
    bi.bmiHeader.biHeight = -h
    bi.bmiHeader.biPlanes = 1
    bi.bmiHeader.biBitCount = 32
    buf = (ctypes.c_byte * (w * h * 4))()
    gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bi), 0)
    raw = bytes(buf)
    out = bytearray(w * h * 3)
    out[0::3] = raw[2::4]
    out[1::3] = raw[1::4]
    out[2::3] = raw[0::4]
    PIL.Image.frombytes("RGB", (w, h), bytes(out)).save(path)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, screen)
    print("wrote", path, (w, h))


def main():
    out = sys.argv[1]
    cx, cy = map(float, sys.argv[2].split(","))
    settle = float(sys.argv[3]) if len(sys.argv) > 3 else 2.0
    hwnd = find_window("rustflutter Gallery")
    user32.SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.6)
    origin = wintypes.POINT()
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    sx, sy = origin.x + cx, origin.y + cy
    print("client", cx, cy, "-> screen", sx, sy)
    # approach in steps so enter/move events stream in
    real_move(origin.x + 100, origin.y + 300)
    time.sleep(0.4)
    for i in range(1, 5):
        real_move(origin.x + 100 + (cx - 100) * i / 5, origin.y + 300 + (cy - 300) * i / 5)
        time.sleep(0.15)
    real_move(sx, sy)
    time.sleep(settle)
    grab_client(hwnd, out)
    user32.SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)


if __name__ == "__main__":
    main()
