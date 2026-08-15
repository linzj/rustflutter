// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

package io.flutter.rustflutter;

import android.app.Activity;
import android.app.ActivityManager;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.graphics.PixelFormat;
import android.os.Build;
import android.os.Bundle;
import android.os.LocaleList;
import android.text.format.DateFormat;
import android.util.Log;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.DisplayCutout;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Locale;

/**
 * The Android half of the rustflutter host.
 *
 * <p>Upstream this role is spread across FlutterActivity, FlutterView,
 * FlutterJNI, TextInputPlugin and a plugin per channel. It is one class here
 * because the fork has one window, one engine and no plugin registrar: what is
 * left after those go is a Surface, a touch stream, an input connection, and
 * the handful of Android APIs the platform channels need.
 *
 * <p>Every application in this fork uses this class unchanged and differs only
 * in which native library its manifest names. That is what lets the packaging
 * script build nine APKs from one Java source.
 */
public class RustflutterActivity extends Activity implements SurfaceHolder.Callback {
  private static final String TAG = "rustflutter";

  /** Which .so to load, from {@code <meta-data android:name="rustflutter.library">}. */
  private static final String LIBRARY_META_DATA = "rustflutter.library";

  // Must match HostRequest in rustflutter_host_android.cc.
  private static final int HOST_SHOW_KEYBOARD = 0;
  private static final int HOST_HIDE_KEYBOARD = 1;
  private static final int HOST_FINISH = 2;
  private static final int HOST_CLIPBOARD_GET = 3;
  private static final int HOST_CLIPBOARD_SET = 4;
  private static final int HOST_CLIPBOARD_HAS_STRINGS = 5;
  private static final int HOST_SET_TASK_LABEL = 6;
  private static final int HOST_RESTART_INPUT = 7;

  // PointerData::Change, which the host passes straight through.
  private static final int CHANGE_CANCEL = 0;
  private static final int CHANGE_ADD = 1;
  private static final int CHANGE_REMOVE = 2;
  private static final int CHANGE_DOWN = 4;
  private static final int CHANGE_MOVE = 5;
  private static final int CHANGE_UP = 6;

  private static RustflutterActivity sInstance;

  /**
   * The framework's editing state, mirrored for the IME.
   *
   * <p>Static because the host reaches it from JNI without an instance, and
   * because there is only ever one field focused at a time. The model in C++
   * remains the authority; this is a copy kept so that {@link EditorInfo} can
   * describe the field the IME is about to edit.
   */
  private static String sText = "";

  private static int sSelectionBase = 0;
  private static int sSelectionExtent = 0;
  private static int sComposingBase = -1;
  private static int sComposingExtent = -1;

  /**
   * Whether the framework has a text field focused.
   *
   * A view that always answers {@link View#onCheckIsTextEditor()} with true is a
   * view the IME may open over at any time, and it did: the keyboard came up on
   * an application with no text in it at all. The framework already says when a
   * field is focused -- that is what `TextInput.setClient` is -- so this is that
   * message, kept where the view can see it.
   */
  private static boolean sHasClient = false;

  private HostView mView;
  private boolean mStarted;
  private boolean mSurfaceReady;

  // -- Lifecycle --------------------------------------------------------------

  @Override
  protected void onCreate(Bundle savedInstanceState) {
    super.onCreate(savedInstanceState);
    sInstance = this;

    String library = libraryName();
    try {
      System.loadLibrary(library);
    } catch (UnsatisfiedLinkError error) {
      Log.e(TAG, "Could not load lib" + library + ".so", error);
      finish();
      return;
    }

    mView = new HostView(this);
    mView.getHolder().addCallback(this);
    // Opaque: the Surface is what the engine draws every pixel of, and a
    // translucent one would make the compositor blend it with a window
    // background that is never visible.
    mView.getHolder().setFormat(PixelFormat.OPAQUE);
    setContentView(mView);
    mView.setFocusable(true);
    mView.setFocusableInTouchMode(true);
    mView.requestFocus();

    // The soft keyboard resizes the window rather than covering it, so a field
    // near the bottom of the screen stays visible while it is being typed into.
    getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);

    // The view is laid out behind the status and navigation bars, and the
    // framework pads its content back out of the way -- that is what
    // MediaQuery.padding and SafeArea are for. Upstream does exactly this in
    // FlutterActivity.configureStatusBarForFullscreenFlutterExperience: draw
    // the system bar backgrounds, tint the status bar, and lay out fullscreen.
    // Without it every inset this Activity reports would be zero and the
    // framework would have nothing to avoid.
    getWindow().addFlags(WindowManager.LayoutParams.FLAG_DRAWS_SYSTEM_BAR_BACKGROUNDS);
    if (Build.VERSION.SDK_INT < 35) {
      getWindow().setStatusBarColor(0x40000000);
    }
    getWindow()
        .getDecorView()
        .setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
  }

  @Override
  protected void onDestroy() {
    if (mStarted) {
      nativeStop();
      mStarted = false;
    }
    if (sInstance == this) {
      sInstance = null;
    }
    super.onDestroy();
  }

  @Override
  protected void onResume() {
    super.onResume();
    if (mStarted) {
      nativeLifecycle("AppLifecycleState.resumed");
    }
  }

  @Override
  protected void onPause() {
    if (mStarted) {
      // Upstream's Android embedder reports `inactive` here and `paused` in
      // onStop, which is the same split the Windows host makes between losing
      // focus and being hidden.
      nativeLifecycle("AppLifecycleState.inactive");
    }
    super.onPause();
  }

  @Override
  protected void onStop() {
    if (mStarted) {
      nativeLifecycle("AppLifecycleState.paused");
    }
    super.onStop();
  }

  @Override
  public void onConfigurationChanged(Configuration configuration) {
    super.onConfigurationChanged(configuration);
    if (mStarted) {
      nativeSettingsChanged(settingsJson(), localesJson());
    }
  }

  @Override
  @SuppressWarnings("deprecation")
  public void onBackPressed() {
    if (!mStarted) {
      super.onBackPressed();
      return;
    }
    // The host answers by finishing this Activity if nothing over there was
    // listening. See HostPlatformView::SendPopRoute.
    nativeBackPressed();
  }

  // -- The Surface ------------------------------------------------------------

  @Override
  public void surfaceCreated(SurfaceHolder holder) {
    // Nothing yet: the size arrives with surfaceChanged, which Android always
    // sends straight after this, and the engine needs a size to start.
  }

  @Override
  public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
    float density = getResources().getDisplayMetrics().density;
    if (!mStarted) {
      nativeSurfaceCreated(holder.getSurface(), width, height, density, refreshRate());
      mSurfaceReady = true;
      nativeStart(icuDataPath(), settingsJson(), localesJson(), filesPath(), externalFilesPath());
      mStarted = true;
      return;
    }
    nativeSurfaceChanged(width, height, density);
  }

  @Override
  public void surfaceDestroyed(SurfaceHolder holder) {
    // The Surface is going away while the shell may still be rasterising into
    // it. Tearing the whole shell down is heavier than upstream, which detaches
    // the surface and keeps the engine -- but this fork has one Activity and no
    // engine cache, so "the Surface is gone" and "the application is gone" are
    // the same event.
    if (mStarted) {
      nativeStop();
      mStarted = false;
      mSurfaceReady = false;
    }
  }

  private float refreshRate() {
    try {
      float hertz = getWindowManager().getDefaultDisplay().getRefreshRate();
      return hertz > 1.0f ? hertz : 60.0f;
    } catch (Exception error) {
      return 60.0f;
    }
  }

  // -- What the platform channels need ----------------------------------------

  /**
   * One request from the host. Called on the Android main thread, which is the
   * engine's platform thread.
   *
   * @return the answer, or null when there is none.
   */
  public static String onHostRequest(int what, String argument) {
    RustflutterActivity activity = sInstance;
    if (activity == null) {
      return null;
    }
    switch (what) {
      case HOST_SHOW_KEYBOARD:
        activity.showKeyboard();
        return null;
      case HOST_HIDE_KEYBOARD:
        activity.hideKeyboard();
        return null;
      case HOST_FINISH:
        activity.finish();
        return null;
      case HOST_CLIPBOARD_GET:
        return activity.clipboardText();
      case HOST_CLIPBOARD_SET:
        activity.setClipboardText(argument);
        return null;
      case HOST_CLIPBOARD_HAS_STRINGS:
        return activity.clipboardText() != null ? "1" : "0";
      case HOST_SET_TASK_LABEL:
        activity.setTaskLabel(argument);
        return null;
      case HOST_RESTART_INPUT:
        // "1" when a field has just been focused, "0" when it has gone.
        sHasClient = "1".equals(argument);
        activity.restartInput();
        if (!sHasClient) {
          activity.hideKeyboard();
        }
        return null;
      default:
        return null;
    }
  }

  /** The framework's editing state, on its way to the IME. */
  public static void onEditingState(
      String text, int selectionBase, int selectionExtent, int composingBase, int composingExtent) {
    sText = text == null ? "" : text;
    sSelectionBase = selectionBase;
    sSelectionExtent = selectionExtent;
    sComposingBase = composingBase;
    sComposingExtent = composingExtent;
  }

  private void showKeyboard() {
    mView.requestFocus();
    inputMethodManager().showSoftInput(mView, 0);
  }

  private void hideKeyboard() {
    inputMethodManager().hideSoftInputFromWindow(mView.getWindowToken(), 0);
  }

  private void restartInput() {
    inputMethodManager().restartInput(mView);
  }

  private InputMethodManager inputMethodManager() {
    return (InputMethodManager) getSystemService(Context.INPUT_METHOD_SERVICE);
  }

  private String clipboardText() {
    ClipboardManager clipboard =
        (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
    if (clipboard == null || !clipboard.hasPrimaryClip()) {
      return null;
    }
    ClipData clip = clipboard.getPrimaryClip();
    if (clip == null || clip.getItemCount() == 0) {
      return null;
    }
    CharSequence text = clip.getItemAt(0).coerceToText(this);
    return text == null || text.length() == 0 ? null : text.toString();
  }

  private void setClipboardText(String text) {
    ClipboardManager clipboard =
        (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
    if (clipboard != null) {
      clipboard.setPrimaryClip(ClipData.newPlainText("text", text));
    }
  }

  private void setTaskLabel(String label) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
      setTaskDescription(new ActivityManager.TaskDescription(label));
    }
  }

  // -- What Java reads and the framework consumes ------------------------------

  /**
   * The {@code flutter/settings} payload.
   *
   * <p>The same three keys the Windows host sends, read from {@link
   * Configuration} rather than from the registry. Assembled as text because
   * every value in it is a boolean, a number or one of two known words -- there
   * is nothing here that could need escaping.
   */
  private String settingsJson() {
    Configuration configuration = getResources().getConfiguration();
    boolean dark =
        (configuration.uiMode & Configuration.UI_MODE_NIGHT_MASK)
            == Configuration.UI_MODE_NIGHT_YES;
    boolean twentyFourHour = DateFormat.is24HourFormat(this);
    float scale = configuration.fontScale > 0 ? configuration.fontScale : 1.0f;
    return "{\"alwaysUse24HourFormat\":"
        + twentyFourHour
        + ",\"textScaleFactor\":"
        + scale
        + ",\"platformBrightness\":\""
        + (dark ? "dark" : "light")
        + "\"}";
  }

  /**
   * The {@code flutter/localization} payload.
   *
   * <p>A flat array of four strings per locale -- language, country, script,
   * variant -- because that is the shape {@code
   * Engine::HandleLocalizationPlatformMessage} reads, on every platform.
   */
  private String localesJson() {
    StringBuilder json = new StringBuilder("{\"method\":\"setLocale\",\"args\":[");
    LocaleList locales = getResources().getConfiguration().getLocales();
    boolean first = true;
    for (int index = 0; index < locales.size(); index++) {
      Locale locale = locales.get(index);
      if (locale == null || locale.getLanguage().isEmpty()) {
        continue;
      }
      if (!first) {
        json.append(',');
      }
      first = false;
      json.append(quote(locale.getLanguage()))
          .append(',')
          .append(quote(locale.getCountry()))
          .append(',')
          .append(quote(locale.getScript()))
          .append(',')
          .append(quote(locale.getVariant()));
    }
    return json.append("]}").toString();
  }

  /** A JSON string. Locale codes are letters and digits, but this is cheap. */
  private static String quote(String value) {
    StringBuilder out = new StringBuilder("\"");
    for (int index = 0; index < value.length(); index++) {
      char character = value.charAt(index);
      if (character == '"' || character == '\\') {
        out.append('\\');
      }
      out.append(character);
    }
    return out.append('"').toString();
  }

  /**
   * Where the engine finds icudtl.dat.
   *
   * <p>An asset has no path an ordinary file reader can open, so it is copied
   * out once. Upstream embeds the same data in the engine library as assembly,
   * which needs a build rule this fork does not have -- the copy costs a few
   * milliseconds on first launch and nothing afterwards.
   */
  private String icuDataPath() {
    File target = new File(getFilesDir(), "icudtl.dat");
    if (target.exists() && target.length() > 0) {
      return target.getAbsolutePath();
    }
    try (InputStream source = getAssets().open("icudtl.dat");
        OutputStream sink = new FileOutputStream(target)) {
      byte[] buffer = new byte[64 * 1024];
      int read;
      while ((read = source.read(buffer)) > 0) {
        sink.write(buffer, 0, read);
      }
    } catch (IOException error) {
      Log.e(TAG, "Could not unpack icudtl.dat", error);
      return "";
    }
    return target.getAbsolutePath();
  }

  /**
   * Where this application may keep files, and where a person may put files for
   * it.
   *
   * <p>Both are directories only Android can name: the path depends on the
   * package and the user, and an application built on Rust's standard library
   * has no way to ask. They are handed over at startup and end up in the
   * environment; see nativeStart.
   *
   * <p>Asking for them is also what creates them, with the ownership that makes
   * them readable -- a directory made by hand under {@code Android/data} is not
   * the same thing and cannot be listed by the application it is named after.
   */
  private String filesPath() {
    File directory = getFilesDir();
    return directory == null ? "" : directory.getAbsolutePath();
  }

  private String externalFilesPath() {
    File directory = getExternalFilesDir(null);
    if (directory == null) {
      return "";
    }
    // Made now rather than on first use, so that `adb push` has somewhere to
    // push to before the application has ever written anything.
    new File(directory, "Pictures").mkdirs();
    return directory.getAbsolutePath();
  }

  private String libraryName() {
    try {
      ActivityInfo info =
          getPackageManager().getActivityInfo(getComponentName(), PackageManager.GET_META_DATA);
      if (info.metaData != null) {
        String name = info.metaData.getString(LIBRARY_META_DATA);
        if (name != null && !name.isEmpty()) {
          return name;
        }
      }
    } catch (PackageManager.NameNotFoundException error) {
      // Falls through to the default below.
    }
    return "rustflutter_app";
  }

  // -- The view ---------------------------------------------------------------

  /**
   * The Surface, the touch stream and the input connection.
   *
   * <p>An inner class rather than a file of its own: everything it does is
   * forward one Android callback to one native function, and separating them
   * would only put the two halves of each pair further apart.
   */
  private static final class HostView extends SurfaceView {
    HostView(Context context) {
      super(context);
    }

    /**
     * Tells the framework what the system is covering.
     *
     * <p>Ported from {@code FlutterView.onApplyWindowInsets}, including the
     * split it makes at API 30. Two kinds of inset go native, and the framework
     * keeps them apart for a reason: <em>view padding</em> is what the system
     * draws over -- the status bar, a notch, the gesture bar -- and does not
     * move when the keyboard opens; <em>view insets</em> is what is pushing
     * content out of the way, which is the keyboard and essentially nothing
     * else.
     *
     * <p>Everything here is in physical pixels, which is what {@code
     * ViewportMetrics} carries; the framework divides by the device pixel
     * ratio.
     */
    @Override
    public WindowInsets onApplyWindowInsets(WindowInsets insets) {
      WindowInsets applied = super.onApplyWindowInsets(insets);

      int paddingTop;
      int paddingRight;
      int paddingBottom;
      int paddingLeft;
      int insetTop = 0;
      int insetRight = 0;
      int insetBottom = 0;
      int insetLeft = 0;

      if (Build.VERSION.SDK_INT >= 30) {
        android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars());
        paddingTop = bars.top;
        paddingRight = bars.right;
        paddingBottom = bars.bottom;
        paddingLeft = bars.left;

        android.graphics.Insets ime = insets.getInsets(WindowInsets.Type.ime());
        insetTop = ime.top;
        insetRight = ime.right;
        insetBottom = ime.bottom; // Typically the only non-zero one.
        insetLeft = ime.left;

        // A cutout is not a system bar, so it has to be merged in separately:
        // take whichever reaches further into the view, side by side.
        DisplayCutout cutout = insets.getDisplayCutout();
        if (cutout != null) {
          android.graphics.Insets waterfall = cutout.getWaterfallInsets();
          paddingTop = Math.max(Math.max(paddingTop, waterfall.top), cutout.getSafeInsetTop());
          paddingRight =
              Math.max(Math.max(paddingRight, waterfall.right), cutout.getSafeInsetRight());
          paddingBottom =
              Math.max(Math.max(paddingBottom, waterfall.bottom), cutout.getSafeInsetBottom());
          paddingLeft = Math.max(Math.max(paddingLeft, waterfall.left), cutout.getSafeInsetLeft());
        }
      } else {
        // Before API 30 there is no way to ask for the keyboard's inset
        // specifically, so upstream guesses: a bottom inset worth more than
        // 18% of the screen is a keyboard rather than a navigation bar. The
        // heuristic and the number are `FlutterView.guessBottomKeyboardInset`.
        int keyboard = 0;
        int screenHeight = getRootView().getHeight();
        if (insets.getSystemWindowInsetBottom() >= screenHeight * 0.18) {
          keyboard = insets.getSystemWindowInsetBottom();
        }
        int visibility = getWindowSystemUiVisibility();
        boolean statusBarVisible = (View.SYSTEM_UI_FLAG_FULLSCREEN & visibility) == 0;
        boolean navigationBarVisible = (View.SYSTEM_UI_FLAG_HIDE_NAVIGATION & visibility) == 0;

        paddingTop = statusBarVisible ? insets.getSystemWindowInsetTop() : 0;
        paddingRight = insets.getSystemWindowInsetRight();
        paddingBottom =
            navigationBarVisible && keyboard == 0 ? insets.getSystemWindowInsetBottom() : 0;
        paddingLeft = insets.getSystemWindowInsetLeft();
        insetBottom = keyboard;
      }

      nativeInsets(
          paddingTop,
          paddingRight,
          paddingBottom,
          paddingLeft,
          insetTop,
          insetRight,
          insetBottom,
          insetLeft);
      return applied;
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
      final long micros = event.getEventTime() * 1000L;
      final int action = event.getActionMasked();
      switch (action) {
        case MotionEvent.ACTION_DOWN:
        case MotionEvent.ACTION_POINTER_DOWN: {
          int index = event.getActionIndex();
          int id = event.getPointerId(index);
          // Add before Down, as every Flutter embedder does: the framework
          // learns that a pointer exists and where it is, and only then that it
          // is touching.
          send(event, index, id, CHANGE_ADD, micros);
          send(event, index, id, CHANGE_DOWN, micros);
          return true;
        }
        case MotionEvent.ACTION_MOVE: {
          // One event can carry a move for every finger down.
          for (int index = 0; index < event.getPointerCount(); index++) {
            send(event, index, event.getPointerId(index), CHANGE_MOVE, micros);
          }
          return true;
        }
        case MotionEvent.ACTION_UP:
        case MotionEvent.ACTION_POINTER_UP: {
          int index = event.getActionIndex();
          int id = event.getPointerId(index);
          send(event, index, id, CHANGE_UP, micros);
          send(event, index, id, CHANGE_REMOVE, micros);
          return true;
        }
        case MotionEvent.ACTION_CANCEL: {
          for (int index = 0; index < event.getPointerCount(); index++) {
            int id = event.getPointerId(index);
            send(event, index, id, CHANGE_CANCEL, micros);
            send(event, index, id, CHANGE_REMOVE, micros);
          }
          return true;
        }
        default:
          return false;
      }
    }

    private static void send(MotionEvent event, int index, int id, int change, long micros) {
      nativePointer(
          id, change, event.getX(index), event.getY(index), micros, event.getPressure(index));
    }

    @Override
    public boolean onCheckIsTextEditor() {
      return sHasClient;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo attributes) {
      attributes.inputType = android.text.InputType.TYPE_CLASS_TEXT;
      attributes.imeOptions =
          EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_FULLSCREEN;
      attributes.initialSelStart = sSelectionBase;
      attributes.initialSelEnd = sSelectionExtent;
      return new HostInputConnection(this);
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
      // Hardware keys, which a phone rarely has and a tablet with a keyboard
      // does. The editing keys go to the field; a printable key becomes text.
      if (nativeEditingKey(keyCode, event.isShiftPressed())) {
        return true;
      }
      if (keyCode == KeyEvent.KEYCODE_ENTER || keyCode == KeyEvent.KEYCODE_NUMPAD_ENTER) {
        nativeEditorAction();
        return true;
      }
      int codePoint = event.getUnicodeChar(event.getMetaState());
      if (codePoint != 0) {
        nativeText(new String(Character.toChars(codePoint)));
        return true;
      }
      return super.onKeyDown(keyCode, event);
    }
  }

  /**
   * What the IME edits through.
   *
   * <p>{@code BaseInputConnection} in its dumb mode: it has an editable of its
   * own that nothing reads, and every method that matters is overridden to send
   * the edit to the framework's model instead. Upstream mirrors the framework's
   * text into a real {@code Editable} and lets the IME edit that; this way
   * round there is one authority rather than two that have to be kept in step.
   */
  private static final class HostInputConnection extends BaseInputConnection {
    HostInputConnection(View view) {
      super(view, /* fullEditor= */ false);
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
      nativeText(text == null ? "" : text.toString());
      return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
      String value = text == null ? "" : text.toString();
      nativeComposing(value, value.length());
      return true;
    }

    @Override
    public boolean finishComposingText() {
      nativeComposingEnd();
      return true;
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
      for (int count = 0; count < beforeLength; count++) {
        nativeEditingKey(KeyEvent.KEYCODE_DEL, false);
      }
      for (int count = 0; count < afterLength; count++) {
        nativeEditingKey(KeyEvent.KEYCODE_FORWARD_DEL, false);
      }
      return true;
    }

    @Override
    public boolean sendKeyEvent(KeyEvent event) {
      if (event.getAction() != KeyEvent.ACTION_DOWN) {
        return true;
      }
      if (nativeEditingKey(event.getKeyCode(), event.isShiftPressed())) {
        return true;
      }
      if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
        nativeEditorAction();
        return true;
      }
      int codePoint = event.getUnicodeChar(event.getMetaState());
      if (codePoint != 0) {
        nativeText(new String(Character.toChars(codePoint)));
      }
      return true;
    }

    @Override
    public boolean performEditorAction(int editorAction) {
      nativeEditorAction();
      return true;
    }

    @Override
    public CharSequence getTextBeforeCursor(int length, int flags) {
      int end = Math.min(Math.max(sSelectionBase, 0), sText.length());
      int start = Math.max(0, end - length);
      return sText.substring(start, end);
    }

    @Override
    public CharSequence getTextAfterCursor(int length, int flags) {
      int start = Math.min(Math.max(sSelectionExtent, 0), sText.length());
      int end = Math.min(sText.length(), start + length);
      return sText.substring(start, end);
    }

    @Override
    public CharSequence getSelectedText(int flags) {
      int start = Math.min(sSelectionBase, sSelectionExtent);
      int end = Math.max(sSelectionBase, sSelectionExtent);
      start = Math.min(Math.max(start, 0), sText.length());
      end = Math.min(Math.max(end, 0), sText.length());
      return start == end ? null : sText.substring(start, end);
    }
  }

  // -- The native half --------------------------------------------------------

  private static native void nativeSurfaceCreated(
      Surface surface, int width, int height, float devicePixelRatio, float refreshRate);

  private static native void nativeStart(
      String icuDataPath,
      String settingsJson,
      String localesJson,
      String filesPath,
      String externalFilesPath);

  private static native void nativeSurfaceChanged(int width, int height, float devicePixelRatio);

  private static native void nativeInsets(
      int paddingTop,
      int paddingRight,
      int paddingBottom,
      int paddingLeft,
      int insetTop,
      int insetRight,
      int insetBottom,
      int insetLeft);

  private static native void nativeStop();

  private static native void nativeLifecycle(String state);

  private static native void nativeSettingsChanged(String settingsJson, String localesJson);

  private static native void nativePointer(
      int pointerId, int phase, float x, float y, long timestampMicros, float pressure);

  private static native void nativeText(String text);

  private static native void nativeComposing(String text, int cursor);

  private static native void nativeComposingEnd();

  private static native boolean nativeEditingKey(int keyCode, boolean shift);

  private static native void nativeEditorAction();

  private static native boolean nativeBackPressed();
}
