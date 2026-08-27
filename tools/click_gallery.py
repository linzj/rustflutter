# Drives synthetic mouse clicks at the running "rustflutter Gallery" window.
# Coordinates are given in the capture PNG's pixel space (physical pixels);
# they are scaled to the actual client rect before posting.
import ctypes
import sys
import time
from ctypes import wintypes

user32 = ctypes.windll.user32

# See grab_window.py: DPI awareness makes client coordinates physical pixels,
# the same space a DPI-aware app hit-tests in.
try:
    user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
except Exception:
    user32.SetProcessDPIAware()

WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP = 0x0202
MK_LBUTTON = 0x0001


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
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    cw, ch = rect.right - rect.left, rect.bottom - rect.top
    # Coordinates come in as capture-png pixels; the capture is the client
    # area, so scale by the ratio supplied on the command line if given.
    cx, cy = int(x), int(y)
    lparam = (cy << 16) | (cx & 0xFFFF)
    user32.SendMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, lparam)
    time.sleep(0.05)
    user32.SendMessageW(hwnd, WM_LBUTTONUP, 0, lparam)


def main():
    # usage: click_gallery.py x,y [x,y ...] [--scale S] [--delay D]
    args = sys.argv[1:]
    scale = 1.0
    delay = 0.8
    points = []
    i = 0
    while i < len(args):
        if args[i] == "--scale":
            scale = float(args[i + 1])
            i += 2
        elif args[i] == "--delay":
            delay = float(args[i + 1])
            i += 2
        else:
            x, y = args[i].split(",")
            points.append((float(x) * scale, float(y) * scale))
            i += 1
    hwnd = find_window("rustflutter Gallery")
    if not hwnd:
        print("window not found")
        sys.exit(1)
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    print(f"client rect: {rect.right - rect.left}x{rect.bottom - rect.top}")
    for x, y in points:
        print(f"click at {x:.0f},{y:.0f}")
        click(hwnd, x, y)
        time.sleep(delay)


if __name__ == "__main__":
    main()
