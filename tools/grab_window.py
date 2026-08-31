# Grabs a window's pixels from the screen (BitBlt works where PrintWindow
# does not for GPU-composited windows) and optionally posts clicks first.
# usage: grab_window.py --title "Flutter Gallery" --shot out.png
#        [--scale S] [--delay D] [x,y ...]
import ctypes
import sys
import time
from ctypes import wintypes

import PIL.Image

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

# Be DPI aware so window rects and client coordinates are physical pixels,
# matching what BitBlt captures and what a DPI-aware app expects in clicks.
# Without this every coordinate is virtualized on a scaled display.
try:
    user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
except Exception:
    user32.SetProcessDPIAware()

WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP = 0x0202
MK_LBUTTON = 0x0001
SRCCOPY = 0x00CC0020


def find_window(title_part: str):
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def enum_cb(hwnd, lparam):
        if user32.IsWindowVisible(hwnd):
            length = user32.GetWindowTextLengthW(hwnd)
            buf = ctypes.create_unicode_buffer(length + 1)
            user32.GetWindowTextW(hwnd, buf, length + 1)
            if title_part in buf.value:
                found.append(hwnd)
        return True

    user32.EnumWindows(enum_cb, 0)
    return found[0] if found else None


def click(hwnd, x, y):
    lparam = (int(y) << 16) | (int(x) & 0xFFFF)
    user32.SendMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, lparam)
    time.sleep(0.05)
    user32.SendMessageW(hwnd, WM_LBUTTONUP, 0, lparam)


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", ctypes.c_long), ("dy", ctypes.c_long),
        ("mouseData", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wintypes.DWORD), ("mi", MOUSEINPUT)]


def sclick(hwnd, x, y):
    # Real input: move the cursor to the point and press/release the button.
    # SendMessage clicks are ignored by some windows (e.g. upstream Flutter).
    point = wintypes.POINT(int(x), int(y))
    user32.ClientToScreen(hwnd, ctypes.byref(point))
    user32.SetCursorPos(point.x, point.y)
    time.sleep(0.1)
    for flags in (0x0002, 0x0004):  # LEFTDOWN, LEFTUP
        event = INPUT()
        event.type = 0  # INPUT_MOUSE
        event.mi.dwFlags = flags
        user32.SendInput(1, ctypes.byref(event), ctypes.sizeof(event))
        time.sleep(0.08)


def srclick(hwnd, x, y):
    # The secondary button, which is what opens a text field's context menu.
    # Same real-input path as `sclick`: the embedder reads the button out of
    # the message, so a synthesized left click can never stand in for it.
    point = wintypes.POINT(int(x), int(y))
    user32.ClientToScreen(hwnd, ctypes.byref(point))
    user32.SetCursorPos(point.x, point.y)
    time.sleep(0.1)
    for flags in (0x0008, 0x0010):  # RIGHTDOWN, RIGHTUP
        event = INPUT()
        event.type = 0  # INPUT_MOUSE
        event.mi.dwFlags = flags
        user32.SendInput(1, ctypes.byref(event), ctypes.sizeof(event))
        time.sleep(0.08)


def swheel(hwnd, x, y, delta):
    # Real input: move the cursor over the point and turn the wheel.
    point = wintypes.POINT(int(x), int(y))
    user32.ClientToScreen(hwnd, ctypes.byref(point))
    user32.SetCursorPos(point.x, point.y)
    time.sleep(0.1)

    steps = abs(delta) // 120 or 1
    for _ in range(steps):
        event = INPUT()
        event.type = 0  # INPUT_MOUSE
        event.mi.dwFlags = 0x0800  # MOUSEEVENTF_WHEEL
        event.mi.mouseData = (120 if delta > 0 else -120) & 0xFFFFFFFF
        user32.SendInput(1, ctypes.byref(event), ctypes.sizeof(event))
        time.sleep(0.05)


def wheel(hwnd, x, y, delta):
    # Hover first: the embedder tracks the pointer before it routes a wheel.
    lparam = (int(y) << 16) | (int(x) & 0xFFFF)
    user32.SendMessageW(hwnd, 0x0200, 0, lparam)
    time.sleep(0.05)
    # WM_MOUSEWHEEL wants screen coordinates in the lparam.
    point = wintypes.POINT(int(x), int(y))
    user32.ClientToScreen(hwnd, ctypes.byref(point))
    lparam = ((point.y & 0xFFFF) << 16) | (point.x & 0xFFFF)
    user32.SendMessageW(hwnd, 0x020A, (delta << 16) & 0xFFFFFFFF, lparam)


def shot(hwnd, path, use_print_window=False):
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    w, h = rect.right - rect.left, rect.bottom - rect.top
    screen_dc = user32.GetWindowDC(hwnd) if use_print_window else user32.GetDC(None)
    mem_dc = gdi32.CreateCompatibleDC(screen_dc)
    bmp = gdi32.CreateCompatibleBitmap(screen_dc, w, h)
    gdi32.SelectObject(mem_dc, bmp)
    if use_print_window:
        # Renders the window's own content even when another window covers it;
        # BitBlt reads the screen and picks up whatever is on top.
        user32.PrintWindow(hwnd, mem_dc, 2)  # PW_RENDERFULLCONTENT
    else:
        gdi32.BitBlt(mem_dc, 0, 0, w, h, screen_dc, rect.left, rect.top, SRCCOPY)

    # Read back the DIB as bottom-up BGRA and write a 24-bit BMP.
    class BITMAPINFOHEADER(ctypes.Structure):
        _fields_ = [
            ("biSize", wintypes.DWORD), ("biWidth", ctypes.c_long),
            ("biHeight", ctypes.c_long), ("biPlanes", wintypes.WORD),
            ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
            ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", ctypes.c_long),
            ("biYPelsPerMeter", ctypes.c_long), ("biClrUsed", wintypes.DWORD),
            ("biClrImportant", wintypes.DWORD),
        ]

    bih = BITMAPINFOHEADER()
    bih.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bih.biWidth = w
    bih.biHeight = -h  # top-down
    bih.biPlanes = 1
    bih.biBitCount = 32
    bih.biCompression = 0
    buf = (ctypes.c_ubyte * (w * h * 4))()
    gdi32.GetDIBits(mem_dc, bmp, 0, h, buf, ctypes.byref(bih), 0)

    image = PIL.Image.frombytes("RGBA", (w, h), bytes(buf), "raw", "BGRA")
    image.convert("RGB").save(path)

    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem_dc)
    if use_print_window:
        user32.ReleaseDC(hwnd, screen_dc)
    else:
        user32.ReleaseDC(None, screen_dc)
    print(f"wrote {path} ({w}x{h})")


def main():
    args = sys.argv[1:]
    title = "Flutter Gallery"
    path = None
    use_print_window = False
    scale = 1.0
    delay = 0.8
    points = []
    sclick_points = []
    srclick_points = []
    wheels = []
    swheels = []
    resize = None
    i = 0
    while i < len(args):
        if args[i] == "--title":
            title = args[i + 1]
            i += 2
        elif args[i] == "--shot":
            path = args[i + 1]
            i += 2
        elif args[i] == "--pshot":
            # PrintWindow: the window's own pixels, occlusion-proof.
            path = args[i + 1]
            use_print_window = True
            i += 2
        elif args[i] == "--scale":
            scale = float(args[i + 1])
            i += 2
        elif args[i] == "--delay":
            delay = float(args[i + 1])
            i += 2
        elif args[i] == "--resize":
            rw, rh = args[i + 1].split(",")
            resize = (int(rw), int(rh))
            i += 2
        elif args[i] == "--sclick":
            x, y = args[i + 1].split(",")
            sclick_points.append((float(x) * scale, float(y) * scale))
            i += 2
        elif args[i] == "--srclick":
            x, y = args[i + 1].split(",")
            srclick_points.append((float(x) * scale, float(y) * scale))
            i += 2
        elif args[i] == "--swheel":
            x, y, delta = args[i + 1].split(",")
            swheels.append((float(x) * scale, float(y) * scale, int(delta)))
            i += 2
        elif args[i] == "--wheel":
            x, y, delta = args[i + 1].split(",")
            wheels.append((float(x) * scale, float(y) * scale, int(delta)))
            i += 2
        else:
            x, y = args[i].split(",")
            points.append((float(x) * scale, float(y) * scale))
            i += 1
    hwnd = find_window(title)
    if not hwnd:
        print("window not found")
        sys.exit(1)
    user32.SetForegroundWindow(hwnd)
    # Foreground rules can refuse SetForegroundWindow from a background
    # process, and then BitBlt/SendInput hit whatever maximized window is
    # really on top. Making the target temporarily topmost is not refusable.
    # HWND_TOPMOST, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW
    user32.SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0040)
    time.sleep(0.3)
    if resize:
        # SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
        user32.SetWindowPos(hwnd, None, 0, 0, resize[0], resize[1], 0x0016)
        time.sleep(0.8)
    for x, y in points:
        print(f"click at {x:.0f},{y:.0f}")
        click(hwnd, x, y)
        time.sleep(delay)
    for x, y in sclick_points:
        print(f"sclick at {x:.0f},{y:.0f}")
        sclick(hwnd, x, y)
        time.sleep(delay)
    for x, y in srclick_points:
        print(f"srclick at {x:.0f},{y:.0f}")
        srclick(hwnd, x, y)
        time.sleep(delay)
    for x, y, delta in wheels:
        print(f"wheel {delta} at {x:.0f},{y:.0f}")
        wheel(hwnd, x, y, delta)
        time.sleep(delay)
    for x, y, delta in swheels:
        print(f"swheel {delta} at {x:.0f},{y:.0f}")
        swheel(hwnd, x, y, delta)
        time.sleep(delay)
    if path:
        shot(hwnd, path, use_print_window)
    # Drop the always-on-top again so the window goes back to normal z-order.
    # HWND_NOTOPMOST, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE
    user32.SetWindowPos(hwnd, -2, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)


if __name__ == "__main__":
    main()
