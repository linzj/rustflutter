// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "flutter/rust/host/rustflutter_host_a11y_win.h"

#include <oleauto.h>
#include <uiautomation.h>

#include <algorithm>
#include <cmath>
#include <mutex>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

#include "flutter/fml/logging.h"

namespace flutter {

//------------------------------------------------------------------------------
/// The window itself, as a node.
///
/// UI Automation wants a single fragment root under the window. The framework
/// gives one root node per view, and this sits above it -- upstream has the
/// same extra node for the same reason (`AXFragmentRootWin`, whose one child is
/// the application's root), and so does the Java bridge, where it is the host
/// `View`.
///
/// `INT32_MIN` rather than zero because zero is the framework's own root, and
/// because a caller's hit-test identifier folds into the low range and may well
/// be zero too.
constexpr int32_t kA11yWindowNodeId = INT32_MIN;

/// One node as the bridge keeps it: what the framework said, plus the upward
/// link. The framework sends children only, because a tree is enough to draw;
/// a reader walks in every direction, so the parent is derived once here rather
/// than searched for on every question.
struct A11yTreeNode {
  SemanticsNode node;
  int32_t parent = kA11yWindowNodeId;
};

/// What the last update owes a listening client.
enum class A11yPendingKind {
  kStructure,
  kName,
  kValue,
  kToggle,
  kFocus,
};

struct A11yPendingEvent {
  A11yPendingKind kind = A11yPendingKind::kStructure;
  int32_t id = kA11yWindowNodeId;
  /// The value before and after, for the property-changed events that carry
  /// both. Empty for the kinds that do not.
  std::wstring was;
  std::wstring now;
  int32_t was_state = 0;
  int32_t now_state = 0;
};

class A11yNodeProvider;

namespace {

std::wstring Widen(const std::string& text) {
  if (text.empty()) {
    return std::wstring();
  }
  int length = MultiByteToWideChar(CP_UTF8, 0, text.c_str(),
                                   static_cast<int>(text.size()), nullptr, 0);
  if (length <= 0) {
    return std::wstring();
  }
  std::wstring wide(static_cast<size_t>(length), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, text.c_str(), static_cast<int>(text.size()),
                      wide.data(), length);
  return wide;
}

/// The state a toggle is in, in UI Automation's vocabulary.
ToggleState ToggleStateOf(const SemanticsNode& node) {
  switch (node.flags.isChecked) {
    case SemanticsCheckState::kTrue:
      return ToggleState_On;
    case SemanticsCheckState::kMixed:
      return ToggleState_Indeterminate;
    default:
      return ToggleState_Off;
  }
}

bool IsCheckable(const SemanticsNode& node) {
  return node.flags.isChecked != SemanticsCheckState::kNone;
}

bool IsEnabled(const SemanticsNode& node) {
  return node.flags.isEnabled != SemanticsTristate::kFalse;
}

bool HasAction(const SemanticsNode& node, SemanticsAction action) {
  return (node.actions & static_cast<int32_t>(action)) != 0;
}

//------------------------------------------------------------------------------
/// What kind of control a reader is being told about.
///
/// The framework's flags map one for one onto the control types upstream's
/// `AXPlatformNodeWin::ComputeUIAControlType` produces from the equivalent
/// `ax::mojom::Role`, and the two odd-looking rows are odd upstream too: a
/// switch is a *button* that supports the toggle pattern rather than a check
/// box, and a heading is plain text distinguished by its ARIA role. Both are
/// what Narrator expects to hear.
LONG ControlTypeOf(const SemanticsNode& node) {
  if (node.flags.isTextField) {
    return UIA_EditControlTypeId;
  }
  if (node.flags.isSlider) {
    return UIA_SliderControlTypeId;
  }
  if (node.flags.isLink) {
    return UIA_HyperlinkControlTypeId;
  }
  if (node.flags.isImage) {
    return UIA_ImageControlTypeId;
  }
  if (node.flags.isButton || IsCheckable(node)) {
    return UIA_ButtonControlTypeId;
  }
  if (!node.label.empty()) {
    return UIA_TextControlTypeId;
  }
  return UIA_GroupControlTypeId;
}

/// Whether a reader may put the keyboard here.
///
/// A node that accepts `kFocus` says so itself; a text field is focusable
/// whether or not the framework offered the action, because that is what an
/// edit control is.
bool IsFocusable(const SemanticsNode& node) {
  return node.flags.isTextField || HasAction(node, SemanticsAction::kFocus);
}

}  // namespace

//------------------------------------------------------------------------------
/// The snapshot every answer is read out of.
struct AccessibilityBridgeWin::Tree {
  std::mutex mutex;

  /// Null once the window is gone. Providers a client still holds go on
  /// answering, with an empty tree and no window to convert coordinates
  /// against.
  HWND window = nullptr;
  double device_pixel_ratio = 1.0;
  AccessibilityBridgeWin::ActionDispatcher dispatch;

  std::unordered_map<int32_t, A11yTreeNode> nodes;
  /// The nodes nobody claimed: the window node's children. One of them, in
  /// every frame this framework produces -- see the note in `Update`.
  std::vector<int32_t> roots;

  /// One provider per node, so that the element a client is holding on to is
  /// the element an event names. Raw pointers: a provider removes itself here
  /// when its last reference goes.
  std::unordered_map<int32_t, A11yNodeProvider*> providers;

  std::vector<A11yPendingEvent> pending;

  /// The provider for `id`, created if this is the first time anybody asked.
  /// Returned with a reference the caller owns. Call with `mutex` held.
  A11yNodeProvider* ProviderLocked(const std::shared_ptr<Tree>& self,
                                   int32_t id);

  /// Whether `id` names something this tree still has.
  bool KnowsLocked(int32_t id) const {
    return id == kA11yWindowNodeId || nodes.find(id) != nodes.end();
  }

  /// The children of `id` in reading order. Call with `mutex` held.
  const std::vector<int32_t>* ChildrenLocked(int32_t id) const {
    if (id == kA11yWindowNodeId) {
      return &roots;
    }
    auto found = nodes.find(id);
    return found == nodes.end() ? nullptr
                                : &found->second.node.childrenInTraversalOrder;
  }
};

//------------------------------------------------------------------------------
/// One node, as UI Automation asks about it.
///
/// Upstream this is `AXPlatformNodeWin`, reached through
/// `FlutterPlatformNodeDelegateWindows`; the interfaces implemented here are
/// the ones that class implements too, minus everything only a document needs.
/// `IRawElementProviderSimple` is what a node *is*,
/// `IRawElementProviderFragment` is where it sits, and a control pattern is one
/// thing a reader can do to it.
///
/// The window node is this same class with `id_ == kA11yWindowNodeId`. Upstream
/// splits it out (`AXFragmentRootPlatformNodeWin`) because its root has a
/// delegate of a different type; here the difference is three answers, and a
/// second class would repeat the other twenty.
class A11yNodeProvider final : public IRawElementProviderSimple,
                               public IRawElementProviderFragment,
                               public IRawElementProviderFragmentRoot,
                               public IInvokeProvider,
                               public IToggleProvider,
                               public IValueProvider {
 public:
  A11yNodeProvider(std::shared_ptr<AccessibilityBridgeWin::Tree> tree,
                   int32_t id)
      : tree_(std::move(tree)), id_(id) {}

  bool IsRoot() const { return id_ == kA11yWindowNodeId; }

  // -- IUnknown ---------------------------------------------------------------

  IFACEMETHODIMP QueryInterface(REFIID riid, void** object) override {
    if (object == nullptr) {
      return E_INVALIDARG;
    }
    *object = nullptr;

    if (riid == IID_IUnknown || riid == IID_IRawElementProviderSimple) {
      *object = static_cast<IRawElementProviderSimple*>(this);
    } else if (riid == IID_IRawElementProviderFragment) {
      *object = static_cast<IRawElementProviderFragment*>(this);
    } else if (riid == IID_IRawElementProviderFragmentRoot) {
      // Only the window is a fragment root. Saying so for every node would
      // have a client treat each of them as the top of its own tree.
      if (!IsRoot()) {
        return E_NOINTERFACE;
      }
      *object = static_cast<IRawElementProviderFragmentRoot*>(this);
    } else if (riid == IID_IInvokeProvider) {
      if (!SupportsInvoke()) {
        return E_NOINTERFACE;
      }
      *object = static_cast<IInvokeProvider*>(this);
    } else if (riid == IID_IToggleProvider) {
      if (!SupportsToggle()) {
        return E_NOINTERFACE;
      }
      *object = static_cast<IToggleProvider*>(this);
    } else if (riid == IID_IValueProvider) {
      if (!SupportsValue()) {
        return E_NOINTERFACE;
      }
      *object = static_cast<IValueProvider*>(this);
    } else {
      return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
  }

  IFACEMETHODIMP_(ULONG) AddRef() override {
    return static_cast<ULONG>(InterlockedIncrement(&references_));
  }

  IFACEMETHODIMP_(ULONG) Release() override {
    const LONG remaining = InterlockedDecrement(&references_);
    if (remaining > 0) {
      return static_cast<ULONG>(remaining);
    }
    // The bridge holds these by raw pointer, so the entry goes before the
    // object does.
    {
      std::lock_guard<std::mutex> lock(tree_->mutex);
      auto found = tree_->providers.find(id_);
      if (found != tree_->providers.end() && found->second == this) {
        tree_->providers.erase(found);
      }
    }
    delete this;
    return 0;
  }

  // -- IRawElementProviderSimple ----------------------------------------------

  IFACEMETHODIMP get_ProviderOptions(ProviderOptions* options) override {
    if (options == nullptr) {
      return E_INVALIDARG;
    }
    // `UseComThreading` is what keeps every question on the thread that owns
    // the window. Upstream's `AXPlatformNodeWin::get_ProviderOptions` asks for
    // the same two, plus two more that only matter to an IAccessible.
    *options = static_cast<ProviderOptions>(ProviderOptions_ServerSideProvider |
                                            ProviderOptions_UseComThreading);
    return S_OK;
  }

  IFACEMETHODIMP GetPatternProvider(PATTERNID pattern,
                                    IUnknown** provider) override {
    if (provider == nullptr) {
      return E_INVALIDARG;
    }
    *provider = nullptr;
    switch (pattern) {
      case UIA_InvokePatternId:
        if (SupportsInvoke()) {
          *provider = static_cast<IInvokeProvider*>(this);
        }
        break;
      case UIA_TogglePatternId:
        if (SupportsToggle()) {
          *provider = static_cast<IToggleProvider*>(this);
        }
        break;
      case UIA_ValuePatternId:
        if (SupportsValue()) {
          *provider = static_cast<IValueProvider*>(this);
        }
        break;
      default:
        break;
    }
    if (*provider != nullptr) {
      AddRef();
    }
    return S_OK;
  }

  IFACEMETHODIMP GetPropertyValue(PROPERTYID property, VARIANT* value) override {
    if (value == nullptr) {
      return E_INVALIDARG;
    }
    VariantInit(value);

    SemanticsNode node;
    const bool known = Snapshot(&node);

    switch (property) {
      case UIA_NamePropertyId: {
        // A node with nothing of its own to say is not nameless: a field with
        // no label is read by what is in it. Upstream reaches the same place
        // through its name-from-contents rules.
        std::wstring name = Widen(node.label);
        if (name.empty() && !node.flags.isObscured) {
          name = Widen(node.value);
        }
        if (IsRoot() || name.empty()) {
          return S_OK;
        }
        value->vt = VT_BSTR;
        value->bstrVal = SysAllocString(name.c_str());
        return S_OK;
      }
      case UIA_ControlTypePropertyId:
        value->vt = VT_I4;
        value->lVal = IsRoot() ? UIA_PaneControlTypeId : ControlTypeOf(node);
        return S_OK;
      case UIA_AriaRolePropertyId:
        // The one role the control type cannot carry: a heading is text, and
        // "heading" is the word that makes a reader offer to jump between
        // them.
        if (!IsRoot() && node.flags.isHeader) {
          value->vt = VT_BSTR;
          value->bstrVal = SysAllocString(L"heading");
        }
        return S_OK;
      case UIA_AutomationIdPropertyId: {
        // The framework's own node id, spelled out. Nothing a reader says out
        // loud -- it is what an automated test names an element by, and this
        // bridge is verified by one.
        std::wstring id = std::to_wstring(id_);
        value->vt = VT_BSTR;
        value->bstrVal = SysAllocString(id.c_str());
        return S_OK;
      }
      case UIA_HelpTextPropertyId:
        if (!IsRoot() && !node.hint.empty()) {
          value->vt = VT_BSTR;
          value->bstrVal = SysAllocString(Widen(node.hint).c_str());
        }
        return S_OK;
      case UIA_IsEnabledPropertyId:
        value->vt = VT_BOOL;
        value->boolVal =
            (IsRoot() || IsEnabled(node)) ? VARIANT_TRUE : VARIANT_FALSE;
        return S_OK;
      case UIA_IsKeyboardFocusablePropertyId:
        value->vt = VT_BOOL;
        value->boolVal =
            (!IsRoot() && IsFocusable(node)) ? VARIANT_TRUE : VARIANT_FALSE;
        return S_OK;
      case UIA_HasKeyboardFocusPropertyId:
        value->vt = VT_BOOL;
        value->boolVal =
            (!IsRoot() && node.flags.isFocused == SemanticsTristate::kTrue)
                ? VARIANT_TRUE
                : VARIANT_FALSE;
        return S_OK;
      case UIA_IsControlElementPropertyId:
      case UIA_IsContentElementPropertyId:
        value->vt = VT_BOOL;
        value->boolVal = VARIANT_TRUE;
        return S_OK;
      case UIA_IsOffscreenPropertyId:
        // A node the framework did not paint is not in the tree at all, so
        // everything still in it is on screen by construction.
        value->vt = VT_BOOL;
        value->boolVal = (known || IsRoot()) ? VARIANT_FALSE : VARIANT_TRUE;
        return S_OK;
      case UIA_IsPasswordPropertyId:
        value->vt = VT_BOOL;
        value->boolVal =
            (!IsRoot() && node.flags.isObscured) ? VARIANT_TRUE : VARIANT_FALSE;
        return S_OK;
      default:
        // An empty VARIANT is UI Automation's "no answer", and the great
        // majority of properties get one.
        return S_OK;
    }
  }

  IFACEMETHODIMP get_HostRawElementProvider(
      IRawElementProviderSimple** provider) override {
    if (provider == nullptr) {
      return E_INVALIDARG;
    }
    *provider = nullptr;
    // Only the fragment root has a window behind it; that is what makes the
    // fragment part of the desktop's tree at all. Upstream's
    // `AXFragmentRootWin` does this and every node under it returns null, as
    // here.
    if (!IsRoot()) {
      return S_OK;
    }
    HWND window = Window();
    if (window == nullptr) {
      return S_OK;
    }
    return UiaHostProviderFromHwnd(window, provider);
  }

  // -- IRawElementProviderFragment --------------------------------------------

  IFACEMETHODIMP Navigate(NavigateDirection direction,
                          IRawElementProviderFragment** result) override {
    if (result == nullptr) {
      return E_INVALIDARG;
    }
    *result = nullptr;

    std::lock_guard<std::mutex> lock(tree_->mutex);
    if (!tree_->KnowsLocked(id_)) {
      return S_OK;
    }

    int32_t target = kA11yWindowNodeId;
    switch (direction) {
      case NavigateDirection_Parent: {
        if (IsRoot()) {
          // The desktop is above this, and UI Automation supplies it from the
          // host provider.
          return S_OK;
        }
        target = tree_->nodes[id_].parent;
        break;
      }
      case NavigateDirection_FirstChild:
      case NavigateDirection_LastChild: {
        const std::vector<int32_t>* children = tree_->ChildrenLocked(id_);
        if (children == nullptr || children->empty()) {
          return S_OK;
        }
        target = direction == NavigateDirection_FirstChild ? children->front()
                                                           : children->back();
        break;
      }
      case NavigateDirection_NextSibling:
      case NavigateDirection_PreviousSibling: {
        if (IsRoot()) {
          return S_OK;
        }
        const std::vector<int32_t>* siblings =
            tree_->ChildrenLocked(tree_->nodes[id_].parent);
        if (siblings == nullptr) {
          return S_OK;
        }
        auto at = std::find(siblings->begin(), siblings->end(), id_);
        if (at == siblings->end()) {
          return S_OK;
        }
        if (direction == NavigateDirection_NextSibling) {
          if (++at == siblings->end()) {
            return S_OK;
          }
        } else {
          if (at == siblings->begin()) {
            return S_OK;
          }
          --at;
        }
        target = *at;
        break;
      }
      default:
        return S_OK;
    }

    if (!tree_->KnowsLocked(target)) {
      return S_OK;
    }
    *result = static_cast<IRawElementProviderFragment*>(
        tree_->ProviderLocked(tree_, target));
    return S_OK;
  }

  IFACEMETHODIMP GetRuntimeId(SAFEARRAY** runtime_id) override {
    if (runtime_id == nullptr) {
      return E_INVALIDARG;
    }
    // `UiaAppendRuntimeId` tells UI Automation to prefix what follows with
    // something identifying this window, so the id is unique across the desktop
    // rather than only within the fragment. The framework's node id is the rest
    // of it -- which is what makes an element stay the same element across
    // frames, and why the framework works to keep those ids stable.
    int32_t id[] = {UiaAppendRuntimeId, id_};
    *runtime_id = SafeArrayCreateVector(VT_I4, 0, 2);
    if (*runtime_id == nullptr) {
      return E_OUTOFMEMORY;
    }
    for (LONG index = 0; index < 2; ++index) {
      SafeArrayPutElement(*runtime_id, &index, &id[index]);
    }
    return S_OK;
  }

  IFACEMETHODIMP get_BoundingRectangle(UiaRect* rect) override {
    if (rect == nullptr) {
      return E_INVALIDARG;
    }
    *rect = {0, 0, 0, 0};

    HWND window = Window();
    if (window == nullptr) {
      return S_OK;
    }

    if (IsRoot()) {
      RECT client{};
      if (GetClientRect(window, &client) == 0) {
        return S_OK;
      }
      *rect = ToScreen(window, client.left, client.top, client.right,
                       client.bottom);
      return S_OK;
    }

    SemanticsNode node;
    double ratio = 1.0;
    if (!Snapshot(&node, &ratio)) {
      return S_OK;
    }

    // Logical pixels from the framework, physical pixels for UI Automation --
    // and because this process is per-monitor aware, a client pixel already is
    // a physical one.
    *rect = ToScreen(window, std::lround(node.rect.left() * ratio),
                     std::lround(node.rect.top() * ratio),
                     std::lround(node.rect.right() * ratio),
                     std::lround(node.rect.bottom() * ratio));
    return S_OK;
  }

  IFACEMETHODIMP GetEmbeddedFragmentRoots(SAFEARRAY** roots) override {
    if (roots == nullptr) {
      return E_INVALIDARG;
    }
    // Nothing is embedded: there are no platform views in this framework, and
    // a fragment root inside a fragment is what those would need.
    *roots = nullptr;
    return S_OK;
  }

  IFACEMETHODIMP SetFocus() override {
    Dispatch(SemanticsAction::kFocus);
    return S_OK;
  }

  IFACEMETHODIMP get_FragmentRoot(
      IRawElementProviderFragmentRoot** root) override {
    if (root == nullptr) {
      return E_INVALIDARG;
    }
    std::lock_guard<std::mutex> lock(tree_->mutex);
    *root = static_cast<IRawElementProviderFragmentRoot*>(
        tree_->ProviderLocked(tree_, kA11yWindowNodeId));
    return S_OK;
  }

  // -- IRawElementProviderFragmentRoot ----------------------------------------

  IFACEMETHODIMP ElementProviderFromPoint(
      double x,
      double y,
      IRawElementProviderFragment** result) override {
    if (result == nullptr) {
      return E_INVALIDARG;
    }
    *result = nullptr;
    HWND window = Window();
    if (window == nullptr) {
      return S_OK;
    }

    POINT point{static_cast<LONG>(std::lround(x)),
                static_cast<LONG>(std::lround(y))};
    if (ScreenToClient(window, &point) == 0) {
      return S_OK;
    }

    std::lock_guard<std::mutex> lock(tree_->mutex);
    const double ratio =
        tree_->device_pixel_ratio > 0 ? tree_->device_pixel_ratio : 1.0;

    // Deepest first, in the order the tree was painted -- which is what makes
    // the answer the thing a finger would have found. Upstream's
    // `FlutterPlatformNodeDelegateWindows::HitTestSync` descends the same way.
    const int32_t hit =
        HitTestLocked(kA11yWindowNodeId, point.x / ratio, point.y / ratio);
    if (hit == kA11yWindowNodeId) {
      return S_OK;
    }
    *result = static_cast<IRawElementProviderFragment*>(
        tree_->ProviderLocked(tree_, hit));
    return S_OK;
  }

  IFACEMETHODIMP GetFocus(IRawElementProviderFragment** result) override {
    if (result == nullptr) {
      return E_INVALIDARG;
    }
    *result = nullptr;

    std::lock_guard<std::mutex> lock(tree_->mutex);
    for (const auto& [id, entry] : tree_->nodes) {
      if (entry.node.flags.isFocused == SemanticsTristate::kTrue) {
        *result = static_cast<IRawElementProviderFragment*>(
            tree_->ProviderLocked(tree_, id));
        return S_OK;
      }
    }
    return S_OK;
  }

  // -- IInvokeProvider --------------------------------------------------------

  IFACEMETHODIMP Invoke() override {
    Dispatch(SemanticsAction::kTap);
    return S_OK;
  }

  // -- IToggleProvider --------------------------------------------------------

  IFACEMETHODIMP Toggle() override {
    // A switch is flipped by being tapped: there is no separate "toggle" the
    // framework knows about, and neither is there upstream -- the Java bridge
    // sends `kTap` for the same gesture.
    Dispatch(SemanticsAction::kTap);
    return S_OK;
  }

  IFACEMETHODIMP get_ToggleState(ToggleState* state) override {
    if (state == nullptr) {
      return E_INVALIDARG;
    }
    SemanticsNode node;
    *state = Snapshot(&node) ? ToggleStateOf(node) : ToggleState_Off;
    return S_OK;
  }

  // -- IValueProvider ---------------------------------------------------------

  IFACEMETHODIMP SetValue(LPCWSTR) override {
    // Nothing in this framework's action set puts text into a field from the
    // outside; upstream has `kSetText` and this port does not carry it yet.
    // Refusing is the honest answer -- a reader told the value was set, which
    // then finds it unchanged, has been lied to.
    return UIA_E_NOTSUPPORTED;
  }

  IFACEMETHODIMP get_Value(BSTR* value) override {
    if (value == nullptr) {
      return E_INVALIDARG;
    }
    SemanticsNode node;
    if (!Snapshot(&node) || node.flags.isObscured) {
      // An obscured field's contents are exactly what must not be read out.
      // The Java bridge refuses the same way.
      *value = SysAllocString(L"");
      return S_OK;
    }
    *value = SysAllocString(Widen(node.value).c_str());
    return S_OK;
  }

  IFACEMETHODIMP get_IsReadOnly(BOOL* read_only) override {
    if (read_only == nullptr) {
      return E_INVALIDARG;
    }
    // True for everything, because `SetValue` refuses. It is one fact said
    // twice, which is what UI Automation asks for.
    *read_only = TRUE;
    return S_OK;
  }

 private:
  ~A11yNodeProvider() = default;

  HWND Window() const {
    std::lock_guard<std::mutex> lock(tree_->mutex);
    return tree_->window;
  }

  static UiaRect ToScreen(HWND window, long left, long top, long right,
                          long bottom) {
    // Corners rather than origin-and-size, which is upstream's
    // `FlutterPlatformNodeDelegateWindows::GetBoundsRect` and for its reason:
    // the offset is what moves, and converting a width would be converting a
    // difference.
    POINT origin{static_cast<LONG>(left), static_cast<LONG>(top)};
    POINT extent{static_cast<LONG>(right), static_cast<LONG>(bottom)};
    ClientToScreen(window, &origin);
    ClientToScreen(window, &extent);
    return UiaRect{static_cast<double>(origin.x), static_cast<double>(origin.y),
                   static_cast<double>(extent.x - origin.x),
                   static_cast<double>(extent.y - origin.y)};
  }

  /// Copies out what this node currently says. False when the node has gone,
  /// which is not an error: a reader asking about something that has just been
  /// rebuilt is a race, not a mistake.
  bool Snapshot(SemanticsNode* out, double* ratio = nullptr) const {
    std::lock_guard<std::mutex> lock(tree_->mutex);
    if (ratio != nullptr) {
      *ratio = tree_->device_pixel_ratio > 0 ? tree_->device_pixel_ratio : 1.0;
    }
    auto found = tree_->nodes.find(id_);
    if (found == tree_->nodes.end()) {
      return false;
    }
    *out = found->second.node;
    return true;
  }

  bool SupportsInvoke() const {
    SemanticsNode node;
    if (IsRoot() || !Snapshot(&node)) {
      return false;
    }
    // Upstream's `AXNodeData::IsInvocable`: something that does a thing when
    // activated and keeps no state of its own. A switch keeps state, so it is
    // a toggle instead, and a field is *activated* rather than invoked -- both
    // exclusions are upstream's, and both stop a reader announcing two ways of
    // doing one thing.
    return HasAction(node, SemanticsAction::kTap) && !IsCheckable(node) &&
           !node.flags.isTextField && IsEnabled(node);
  }

  bool SupportsToggle() const {
    SemanticsNode node;
    return !IsRoot() && Snapshot(&node) && IsCheckable(node);
  }

  bool SupportsValue() const {
    SemanticsNode node;
    return !IsRoot() && Snapshot(&node) && node.flags.isTextField;
  }

  /// The deepest node under `id` containing the point, or `id` itself.
  /// Call with `mutex` held.
  int32_t HitTestLocked(int32_t id, double x, double y) const {
    const std::vector<int32_t>* children = tree_->ChildrenLocked(id);
    if (children != nullptr) {
      // Backwards, because a later child painted over an earlier one and what
      // is on top is what a reader is pointing at.
      for (auto child = children->rbegin(); child != children->rend(); ++child) {
        auto found = tree_->nodes.find(*child);
        if (found == tree_->nodes.end()) {
          continue;
        }
        const SkRect& rect = found->second.node.rect;
        if (x < rect.left() || x >= rect.right() || y < rect.top() ||
            y >= rect.bottom()) {
          continue;
        }
        return HitTestLocked(*child, x, y);
      }
    }
    return id;
  }

  void Dispatch(SemanticsAction action) {
    AccessibilityBridgeWin::ActionDispatcher dispatch;
    {
      std::lock_guard<std::mutex> lock(tree_->mutex);
      dispatch = tree_->dispatch;
    }
    if (dispatch && !IsRoot()) {
      dispatch(id_, action);
    }
  }

  std::shared_ptr<AccessibilityBridgeWin::Tree> tree_;
  const int32_t id_;
  LONG references_ = 1;

  A11yNodeProvider(const A11yNodeProvider&) = delete;
  A11yNodeProvider& operator=(const A11yNodeProvider&) = delete;
};

A11yNodeProvider* AccessibilityBridgeWin::Tree::ProviderLocked(
    const std::shared_ptr<Tree>& self,
    int32_t id) {
  auto found = providers.find(id);
  if (found != providers.end()) {
    found->second->AddRef();
    return found->second;
  }
  auto* provider = new A11yNodeProvider(self, id);
  providers[id] = provider;
  return provider;
}

//------------------------------------------------------------------------------

AccessibilityBridgeWin::AccessibilityBridgeWin(HWND window)
    : tree_(std::make_shared<Tree>()) {
  tree_->window = window;
}

AccessibilityBridgeWin::~AccessibilityBridgeWin() {
  Shutdown();
}

void AccessibilityBridgeWin::SetActionDispatcher(ActionDispatcher dispatch) {
  std::lock_guard<std::mutex> lock(tree_->mutex);
  tree_->dispatch = std::move(dispatch);
}

void AccessibilityBridgeWin::SetDevicePixelRatio(double ratio) {
  std::lock_guard<std::mutex> lock(tree_->mutex);
  tree_->device_pixel_ratio = ratio > 0 ? ratio : 1.0;
}

void AccessibilityBridgeWin::Update(const SemanticsNodeUpdates& update) {
  std::lock_guard<std::mutex> lock(tree_->mutex);

  std::unordered_map<int32_t, A11yTreeNode> next;
  next.reserve(update.size());
  for (const auto& [id, node] : update) {
    next[id].node = node;
  }
  // A child names its parent once the whole frame is in, because the update is
  // flat and a child may arrive before the node that claims it.
  for (const auto& [id, node] : update) {
    for (int32_t child : node.childrenInTraversalOrder) {
      auto found = next.find(child);
      if (found != next.end()) {
        found->second.parent = id;
      }
    }
  }

  // Whatever nobody claimed sits directly under the window. The framework
  // gives one such node per view -- reading order inside it is carried by
  // `childrenInTraversalOrder`, which is why it exists -- so the sort below is
  // only a tie-break that a well-formed frame never reaches.
  std::vector<int32_t> next_roots;
  for (const auto& [id, entry] : next) {
    if (entry.parent == kA11yWindowNodeId) {
      next_roots.push_back(id);
    }
  }
  std::sort(next_roots.begin(), next_roots.end());

  // What a listening client is owed. Structure first: a client told the
  // children changed re-reads them, and every property event after that would
  // be about an element it has already thrown away.
  bool structure_changed =
      next.size() != tree_->nodes.size() || next_roots != tree_->roots;
  for (const auto& [id, entry] : next) {
    auto before = tree_->nodes.find(id);
    if (before == tree_->nodes.end()) {
      structure_changed = true;
      continue;
    }
    const SemanticsNode& was = before->second.node;
    const SemanticsNode& now = entry.node;
    if (was.childrenInTraversalOrder != now.childrenInTraversalOrder) {
      structure_changed = true;
    }
    if (was.label != now.label) {
      tree_->pending.push_back({A11yPendingKind::kName, id, Widen(was.label),
                                Widen(now.label), 0, 0});
    }
    if (was.value != now.value && !now.flags.isObscured) {
      tree_->pending.push_back({A11yPendingKind::kValue, id, Widen(was.value),
                                Widen(now.value), 0, 0});
    }
    if (was.flags.isChecked != now.flags.isChecked) {
      tree_->pending.push_back({A11yPendingKind::kToggle,
                                id,
                                {},
                                {},
                                static_cast<int32_t>(ToggleStateOf(was)),
                                static_cast<int32_t>(ToggleStateOf(now))});
    }
    if (was.flags.isFocused != now.flags.isFocused &&
        now.flags.isFocused == SemanticsTristate::kTrue) {
      tree_->pending.push_back({A11yPendingKind::kFocus, id, {}, {}, 0, 0});
    }
  }
  if (structure_changed) {
    tree_->pending.insert(
        tree_->pending.begin(),
        A11yPendingEvent{A11yPendingKind::kStructure, kA11yWindowNodeId});
  }

  tree_->nodes = std::move(next);
  tree_->roots = std::move(next_roots);
}

void AccessibilityBridgeWin::RaisePendingEvents() {
  // Nothing to tell a desktop with no screen reader on it. This is also what
  // keeps the ordinary case free: `UiaClientsAreListening` is false until
  // something connects.
  if (!UiaClientsAreListening()) {
    std::lock_guard<std::mutex> lock(tree_->mutex);
    tree_->pending.clear();
    return;
  }

  std::vector<std::pair<A11yPendingEvent, A11yNodeProvider*>> targets;
  {
    std::lock_guard<std::mutex> lock(tree_->mutex);
    std::vector<A11yPendingEvent> events;
    events.swap(tree_->pending);
    for (const A11yPendingEvent& event : events) {
      if (!tree_->KnowsLocked(event.id)) {
        continue;
      }
      targets.emplace_back(event, tree_->ProviderLocked(tree_, event.id));
    }
  }

  // Outside the lock: raising an event calls into the client, and a client
  // that answers by asking a question would deadlock against a lock still held
  // here.
  for (auto& [event, provider] : targets) {
    auto* simple = static_cast<IRawElementProviderSimple*>(provider);
    switch (event.kind) {
      case A11yPendingKind::kStructure:
        UiaRaiseStructureChangedEvent(
            simple, StructureChangeType_ChildrenInvalidated, nullptr, 0);
        break;
      case A11yPendingKind::kName:
      case A11yPendingKind::kValue: {
        VARIANT was;
        VARIANT now;
        VariantInit(&was);
        VariantInit(&now);
        was.vt = VT_BSTR;
        was.bstrVal = SysAllocString(event.was.c_str());
        now.vt = VT_BSTR;
        now.bstrVal = SysAllocString(event.now.c_str());
        UiaRaiseAutomationPropertyChangedEvent(
            simple,
            event.kind == A11yPendingKind::kName ? UIA_NamePropertyId
                                                 : UIA_ValueValuePropertyId,
            was, now);
        VariantClear(&was);
        VariantClear(&now);
        break;
      }
      case A11yPendingKind::kToggle: {
        VARIANT was;
        VARIANT now;
        VariantInit(&was);
        VariantInit(&now);
        was.vt = VT_I4;
        was.lVal = event.was_state;
        now.vt = VT_I4;
        now.lVal = event.now_state;
        UiaRaiseAutomationPropertyChangedEvent(
            simple, UIA_ToggleToggleStatePropertyId, was, now);
        VariantClear(&was);
        VariantClear(&now);
        break;
      }
      case A11yPendingKind::kFocus:
        UiaRaiseAutomationEvent(simple, UIA_AutomationFocusChangedEventId);
        break;
    }
    provider->Release();
  }
}

LRESULT AccessibilityBridgeWin::GetObject(WPARAM wparam,
                                          LPARAM lparam,
                                          bool* is_accessibility_request) {
  // Only the low half of lparam carries the object id: it is sometimes
  // sign-extended and sometimes not, which upstream's `OnGetObject` says in the
  // same words because it was found the same way.
  const DWORD object_id = static_cast<DWORD>(static_cast<DWORD_PTR>(lparam));
  const bool is_uia = object_id == static_cast<DWORD>(UiaRootObjectId);
  const bool is_msaa = object_id == static_cast<DWORD>(OBJID_CLIENT);

  if (is_accessibility_request != nullptr) {
    // Either question means something is reading the screen. Windows never
    // says so directly -- there is a system parameter for it and Narrator does
    // not set it -- so this is the notice, and upstream takes it here too.
    *is_accessibility_request = is_uia || is_msaa;
  }

  if (!is_uia) {
    // MSAA would need an `IAccessible`, which is a second, older tree with its
    // own vocabulary. Narrator, NVDA and JAWS all speak UI Automation, and
    // answering nothing here has a client that asked for MSAA ask for UIA
    // instead.
    return 0;
  }

  HWND window = nullptr;
  A11yNodeProvider* root = nullptr;
  {
    std::lock_guard<std::mutex> lock(tree_->mutex);
    window = tree_->window;
    if (window == nullptr) {
      return 0;
    }
    root = tree_->ProviderLocked(tree_, kA11yWindowNodeId);
  }

  LRESULT result = UiaReturnRawElementProvider(
      window, wparam, lparam, static_cast<IRawElementProviderSimple*>(root));
  root->Release();
  return result;
}

void AccessibilityBridgeWin::Shutdown() {
  HWND window = nullptr;
  {
    std::lock_guard<std::mutex> lock(tree_->mutex);
    window = tree_->window;
    tree_->window = nullptr;
    tree_->dispatch = nullptr;
    tree_->nodes.clear();
    tree_->roots.clear();
    tree_->pending.clear();
  }
  if (window != nullptr) {
    // The documented way to tell UI Automation a window's provider is going
    // away. Without it a client can keep a reference to a fragment whose window
    // has been destroyed.
    UiaReturnRawElementProvider(window, 0, 0, nullptr);
  }
}

}  // namespace flutter
