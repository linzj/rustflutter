// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

// Windows presenter for rustflutter.
//
// This is deliberately the smallest thing that puts a rendered frame on screen:
// a Win32 window that blits a BGRA8888 buffer with StretchDIBits. It is not the
// production path -- that is `shell/platform/windows` driving Impeller through
// the Shell, which still needs the Engine/RuntimeController rework. Until then
// this lets an app be looked at rather than merely diffed as a PNG.

#include "flutter/rust/ffi/rustflutter_ffi.h"

#include <windows.h>

#include <string>
#include <vector>

namespace {

struct Frame {
  int32_t width = 0;
  int32_t height = 0;
  const uint8_t* pixels = nullptr;
};

constexpr wchar_t kWindowClass[] = L"RustflutterWindow";

void PaintFrame(HWND hwnd, const Frame& frame) {
  PAINTSTRUCT ps;
  HDC hdc = BeginPaint(hwnd, &ps);

  RECT client;
  GetClientRect(hwnd, &client);

  BITMAPINFO info = {};
  info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
  info.bmiHeader.biWidth = frame.width;
  // Negative height means a top-down DIB, which matches the row order Skia
  // hands back. Without this the frame appears vertically mirrored.
  info.bmiHeader.biHeight = -frame.height;
  info.bmiHeader.biPlanes = 1;
  info.bmiHeader.biBitCount = 32;
  info.bmiHeader.biCompression = BI_RGB;

  SetStretchBltMode(hdc, HALFTONE);
  StretchDIBits(hdc,
                /*xDest=*/0, /*yDest=*/0,
                /*DestWidth=*/client.right - client.left,
                /*DestHeight=*/client.bottom - client.top,
                /*xSrc=*/0, /*ySrc=*/0,
                /*SrcWidth=*/frame.width, /*SrcHeight=*/frame.height,
                frame.pixels, &info, DIB_RGB_COLORS, SRCCOPY);

  EndPaint(hwnd, &ps);
}

LRESULT CALLBACK WindowProc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam) {
  auto* frame = reinterpret_cast<Frame*>(GetWindowLongPtr(hwnd, GWLP_USERDATA));

  switch (msg) {
    case WM_CREATE: {
      auto* create = reinterpret_cast<CREATESTRUCT*>(lparam);
      SetWindowLongPtr(hwnd, GWLP_USERDATA,
                       reinterpret_cast<LONG_PTR>(create->lpCreateParams));
      return 0;
    }
    case WM_PAINT:
      if (frame != nullptr && frame->pixels != nullptr) {
        PaintFrame(hwnd, *frame);
      }
      return 0;
    case WM_ERASEBKGND:
      // The frame covers the whole client area, so skipping the background
      // erase avoids a flash of white on resize.
      return 1;
    case WM_SIZE:
      InvalidateRect(hwnd, nullptr, FALSE);
      return 0;
    case WM_KEYDOWN:
      if (wparam == VK_ESCAPE) {
        PostMessage(hwnd, WM_CLOSE, 0, 0);
      }
      return 0;
    case WM_DESTROY:
      PostQuitMessage(0);
      return 0;
    default:
      break;
  }
  return DefWindowProc(hwnd, msg, wparam, lparam);
}

std::wstring Widen(const char* utf8) {
  if (utf8 == nullptr || utf8[0] == '\0') {
    return L"rustflutter";
  }
  int len = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, nullptr, 0);
  if (len <= 0) {
    return L"rustflutter";
  }
  std::wstring out(static_cast<size_t>(len - 1), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, utf8, -1, out.data(), len);
  return out;
}

}  // namespace

int32_t rf_window_show(int32_t width,
                       int32_t height,
                       const uint8_t* bgra,
                       size_t bgra_len,
                       const char* title) {
  if (bgra == nullptr || width <= 0 || height <= 0) {
    return -1;
  }
  const size_t needed =
      static_cast<size_t>(width) * static_cast<size_t>(height) * 4u;
  if (bgra_len < needed) {
    return -1;
  }

  Frame frame{width, height, bgra};
  HINSTANCE instance = GetModuleHandle(nullptr);

  WNDCLASSEX wc = {};
  wc.cbSize = sizeof(wc);
  wc.lpfnWndProc = WindowProc;
  wc.hInstance = instance;
  wc.hCursor = LoadCursor(nullptr, IDC_ARROW);
  wc.lpszClassName = kWindowClass;
  // Re-registering the same class in one process is not an error worth
  // failing on; a second window in the same run should just work.
  if (RegisterClassEx(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
    return -2;
  }

  // Size the window so the *client* area matches the frame exactly, otherwise
  // the border and title bar would eat into it and the frame would be scaled.
  RECT rect{0, 0, width, height};
  AdjustWindowRect(&rect, WS_OVERLAPPEDWINDOW, FALSE);

  std::wstring window_title = Widen(title);
  HWND hwnd = CreateWindowEx(0, kWindowClass, window_title.c_str(),
                             WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                             rect.right - rect.left, rect.bottom - rect.top,
                             nullptr, nullptr, instance, &frame);
  if (hwnd == nullptr) {
    return -3;
  }

  ShowWindow(hwnd, SW_SHOWNORMAL);
  UpdateWindow(hwnd);

  MSG msg;
  while (GetMessage(&msg, nullptr, 0, 0) > 0) {
    TranslateMessage(&msg);
    DispatchMessage(&msg);
  }
  return 0;
}
