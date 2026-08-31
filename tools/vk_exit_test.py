# Launches an exe, waits for its window, optionally screenshots it, then posts
# WM_CLOSE and reports the process exit code. Used to verify clean shutdown
# (exit code 0 vs 0xC0000005 access violation) per backend.
# usage: vk_exit_test.py <exe> [--title T] [--shot out.png] [--wait S]
import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes

user32 = ctypes.windll.user32
WM_CLOSE = 0x0010


def find_window(title_part, pid):
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def enum_cb(hwnd, lparam):
        if user32.IsWindowVisible(hwnd):
            wpid = wintypes.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
            if wpid.value == pid:
                length = user32.GetWindowTextLengthW(hwnd)
                buf = ctypes.create_unicode_buffer(length + 1)
                user32.GetWindowTextW(hwnd, buf, length + 1)
                if title_part in buf.value:
                    found.append(hwnd)
        return True

    user32.EnumWindows(enum_cb, 0)
    return found[0] if found else None


def main():
    args = sys.argv[1:]
    exe = args[0]
    title = "Gallery"
    shot_path = None
    wait_s = 6.0
    close_after = 2.0
    i = 1
    while i < len(args):
        if args[i] == "--title":
            title = args[i + 1]
            i += 2
        elif args[i] == "--shot":
            shot_path = args[i + 1]
            i += 2
        elif args[i] == "--wait":
            wait_s = float(args[i + 1])
            i += 2
        else:
            i += 1

    log_path = os.path.join(os.environ.get("TEMP", "."), "vk_exit_test.log")
    log = open(log_path, "w")
    proc = subprocess.Popen([exe], stdout=log, stderr=subprocess.STDOUT)

    hwnd = None
    deadline = time.time() + wait_s
    while time.time() < deadline and proc.poll() is None:
        hwnd = find_window(title, proc.pid)
        if hwnd:
            break
        time.sleep(0.3)

    if not hwnd:
        print("window not found")
        if proc.poll() is None:
            proc.kill()
        print(f"exit code: {proc.wait()}")
        sys.exit(1)
    print(f"window found: hwnd={hwnd}")

    if shot_path:
        shot_proc = subprocess.run(
            [sys.executable, "tools/grab_window.py", "--title", title,
             "--shot", shot_path, "--delay", "0.1"],
            capture_output=True, text=True)
        print(shot_proc.stdout.strip() or shot_proc.stderr.strip())

    time.sleep(close_after)
    print("posting WM_CLOSE")
    user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)

    try:
        rc = proc.wait(timeout=20)
    except subprocess.TimeoutExpired:
        print("process did not exit within 20s after WM_CLOSE; killing")
        proc.kill()
        rc = proc.wait()
    log.close()
    try:
        with open(log_path, errors="replace") as f:
            tail = f.read()
        interesting = [l for l in tail.splitlines() if l.strip()]
        print("--- child log (last 15 lines) ---")
        for line in interesting[-15:]:
            print(line)
        print("--- end child log ---")
    except OSError:
        pass
    print(f"exit code: {rc}" + (" (0xC0000005 ACCESS_VIOLATION)" if rc == -1073741819 else ""))
    sys.exit(0 if rc == 0 else 1)


if __name__ == "__main__":
    main()
