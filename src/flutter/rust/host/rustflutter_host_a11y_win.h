// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_A11Y_WIN_H_
#define FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_A11Y_WIN_H_

#include <windows.h>

#include <functional>
#include <memory>

#include "flutter/lib/ui/semantics/semantics_node.h"

namespace flutter {

//------------------------------------------------------------------------------
/// What the interface says, for a reader on Windows who is not looking at it.
///
/// The other end of the same tree the Android bridge serves. The framework
/// builds one description of the interface and both platforms answer from it;
/// what differs is only who is asking. Android asks through an
/// `AccessibilityNodeProvider`, Windows through UI Automation, and the two
/// vocabularies line up almost word for word because they are both describing
/// the same thing.
///
/// Upstream this is `AccessibilityBridgeWindows` plus
/// `FlutterPlatformNodeDelegateWindows`, and both of them are thin: the actual
/// UI Automation server is `AXPlatformNodeWin`, thirty thousand lines vendored
/// from Chromium under `third_party/accessibility`. There is no such library
/// here and there is not going to be one, so the provider interfaces are
/// implemented directly against a tree that is already flat: an id, a rectangle,
/// a label, some flags and a list of children. That is a small enough surface
/// that writing the COM by hand is smaller than the machinery for avoiding it.
///
/// Threading: the tree arrives from the platform thread and the questions
/// arrive from the window thread, which in this host are two different threads
/// (upstream they are one, so upstream needs no lock). The snapshot is
/// therefore taken under a mutex and every answer is read out of it -- the same
/// arrangement as the Java bridge, which synchronises for the same reason.
class AccessibilityBridgeWin {
 public:
  //----------------------------------------------------------------------------
  /// How an action a reader performed reaches the framework.
  ///
  /// Called on whichever thread UI Automation used, so an implementation has to
  /// hop to the platform thread itself. Upstream's
  /// `AccessibilityBridgeWindows::DispatchAccessibilityAction`.
  using ActionDispatcher =
      std::function<void(int32_t node_id, SemanticsAction action)>;

  /// The snapshot every answer is read out of, defined in the implementation.
  /// Public only because the provider objects there are not members of this
  /// class and have to name it.
  struct Tree;

  explicit AccessibilityBridgeWin(HWND window);
  ~AccessibilityBridgeWin();

  /// Where actions go. Set once, before the window is shown.
  void SetActionDispatcher(ActionDispatcher dispatch);

  /// The scale between the framework's logical pixels and the window's. UI
  /// Automation wants rectangles in physical screen pixels, and this process is
  /// per-monitor aware, so a client pixel already is a physical one.
  void SetDevicePixelRatio(double ratio);

  //----------------------------------------------------------------------------
  /// Takes one frame's semantics tree. Called from the platform thread.
  ///
  /// Records what changed but raises nothing: a provider that asked for COM
  /// threading belongs to the apartment its window is in, so the caller posts
  /// to the window thread and `RaisePendingEvents` runs there.
  void Update(const SemanticsNodeUpdates& update);

  //----------------------------------------------------------------------------
  /// Answers `WM_GETOBJECT`.
  ///
  /// Returns zero for a request this bridge does not serve, which the window
  /// proc must then pass to `DefWindowProc`. `*is_accessibility_request` is set
  /// when the message was an assistive technology asking for anything at all --
  /// which is the only notice Windows ever gives that a screen reader is
  /// running, and therefore the moment to turn semantics on. Upstream reads it
  /// the same way, in `FlutterWindow::OnGetObject`, and says so at length: there
  /// is an API for querying screen reader state and Narrator does not set it.
  LRESULT GetObject(WPARAM wparam,
                    LPARAM lparam,
                    bool* is_accessibility_request);

  /// Raises the events the last update owes. Window thread only.
  void RaisePendingEvents();

  /// Lets go of the window. Providers a client still holds keep answering, with
  /// nothing to say.
  void Shutdown();

 private:
  /// Held by a `shared_ptr` because a UI Automation client may hold a provider
  /// longer than the window lives, and a provider with a dangling bridge would
  /// answer from freed memory.
  std::shared_ptr<Tree> tree_;

  AccessibilityBridgeWin(const AccessibilityBridgeWin&) = delete;
  AccessibilityBridgeWin& operator=(const AccessibilityBridgeWin&) = delete;
};

}  // namespace flutter

#endif  // FLUTTER_RUST_HOST_RUSTFLUTTER_HOST_A11Y_WIN_H_
