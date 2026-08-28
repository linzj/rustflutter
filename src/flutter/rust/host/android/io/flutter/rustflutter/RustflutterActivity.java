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
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;
import android.graphics.Rect;
import android.util.SparseArray;

import org.json.JSONArray;
import org.json.JSONObject;

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

  /**
   * The engine, when the application was linked against the shared one. Named
   * rather than discovered because there is only ever one of it, and packaged
   * beside the application by make_apk.py when the application needs it.
   */
  private static final String ENGINE_LIBRARY = "rustflutter_engine";

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

  /**
   * The focused field's configuration, as {@code TextInput.setClient} described
   * it, for {@link #onCreateInputConnection} to turn into an {@link EditorInfo}.
   *
   * <p>Names rather than numbers cross from C++, so every Android constant is
   * spelled here rather than copied there -- which is also where upstream keeps
   * them, in {@code TextInputPlugin.inputTypeFromTextInputType}.
   */
  private static String sInputType = "";

  private static String sInputAction = "";
  private static boolean sObscureText = false;
  private static boolean sAutocorrect = true;
  private static boolean sEnableSuggestions = true;
  private static boolean sPersonalizedLearning = true;
  private static boolean sNumberSigned = false;
  private static boolean sNumberDecimal = false;
  private static String sCapitalization = "";

  /**
   * The IME's standing request to be told when the text changes, if it made
   * one -- upstream's {@code mExtractRequest}, set from the
   * {@code GET_EXTRACTED_TEXT_MONITOR} flag.
   */
  private static android.view.inputmethod.ExtractedTextRequest sExtractRequest;

  private HostView mView;
  private boolean mStarted;
  private boolean mSurfaceReady;

  // -- Lifecycle --------------------------------------------------------------

  @Override
  protected void onCreate(Bundle savedInstanceState) {
    super.onCreate(savedInstanceState);
    sInstance = this;

    // The engine, when it is a library of its own rather than part of the
    // application's. Loaded first and by name, because loading it is what runs
    // its JNI_OnLoad -- the application's library names it as a dependency, so
    // the linker would bring it in either way, but a dependency pulled in that
    // way is never handed the VM.
    //
    // Its absence is not an error and is not a flag either: an application
    // linked against the engine archive has the engine inside its own library
    // and there is nothing here to load. That the .so is there is the fact.
    try {
      System.loadLibrary(ENGINE_LIBRARY);
    } catch (UnsatisfiedLinkError error) {
      Log.d(TAG, "No lib" + ENGINE_LIBRARY + ".so; the engine is linked in.");
    }

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
    // The one real view stands in for every semantics node; the provider makes
    // them virtual children of it. Upstream's FlutterView does the same, for
    // the same reason: Android's accessibility tree is a tree of Views, and
    // there is only ever going to be one of those here.
    mView.setSemanticsProvider(new SemanticsProvider());
    watchAccessibility();

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
      stopWatchingAccessibility();
      nativeStop();
      mStarted = false;
      mSurfaceReady = false;
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
    if (!mSurfaceReady) {
      // The application was in the background and has been brought back. The
      // engine never went away; this is a new Surface for it to draw the same
      // screen into. Not nativeSurfaceCreated, which is the one that starts the
      // application -- starting it a second time is exactly the bug this
      // avoids.
      //
      // Upstream reattaches from surfaceCreated rather than from here
      // (FlutterSurfaceView.connectSurfaceToRenderer, which reaches
      // FlutterJNI.onSurfaceCreated), and sends the size afterwards as a
      // separate onSurfaceChanged. This host needs a size to hand the shell
      // along with the window, and Android always calls surfaceChanged straight
      // after surfaceCreated, so the two arrive together -- which is the same
      // reason the first attach is here rather than there.
      nativeSurfaceRecreated(holder.getSurface(), width, height, density);
      mSurfaceReady = true;
      return;
    }
    nativeSurfaceChanged(width, height, density);
  }

  @Override
  public void surfaceDestroyed(SurfaceHolder holder) {
    // Only the Surface. Android reclaims it whenever the Activity stops being
    // visible -- another application came to the front, or the screen went off
    // -- and that is not the application ending, so the shell, the framework
    // and everything the reader had done stay up. Tearing them down here is
    // what used to happen, and it meant every trip to another application came
    // back to the first screen with the state gone.
    //
    // The application ends at onDestroy, where nativeStop is. Upstream draws
    // the line in the same place: FlutterSurfaceView.surfaceDestroyed reaches
    // FlutterRenderer.stopRenderingToSurface and stops there, and the engine is
    // destroyed only from onDetach.
    //
    // The mSurfaceReady guard is upstream's `if (surface != null)` in
    // stopRenderingToSurface: this can arrive for a Surface that has already
    // been given back, and telling the shell twice is not the same as telling
    // it once.
    if (mStarted && mSurfaceReady) {
      nativeSurfaceDestroyed();
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
        // "0" when the field has gone; otherwise "1" and the configuration,
        // newline separated -- see TextInputHandler::Descriptor.
        setInputConfiguration(argument);
        activity.restartInput();
        if (!sHasClient) {
          activity.hideKeyboard();
        }
        return null;
      default:
        return null;
    }
  }

  /**
   * Unpacks what {@code TextInputHandler::Descriptor} packed: "0" for a field
   * that has gone, otherwise "1" and the six things an {@link EditorInfo} is
   * built from.
   */
  private static void setInputConfiguration(String descriptor) {
    String[] parts = descriptor == null ? new String[0] : descriptor.split("\n", -1);
    sHasClient = parts.length > 0 && "1".equals(parts[0]);
    if (!sHasClient) {
      return;
    }
    sInputType = parts.length > 1 ? parts[1] : "";
    sInputAction = parts.length > 2 ? parts[2] : "";
    sObscureText = parts.length > 3 && "1".equals(parts[3]);
    // Upstream's defaults are true, so a missing field is on rather than off.
    sAutocorrect = parts.length <= 4 || "1".equals(parts[4]);
    sEnableSuggestions = parts.length <= 5 || "1".equals(parts[5]);
    sPersonalizedLearning = parts.length <= 6 || "1".equals(parts[6]);
    sNumberSigned = parts.length > 7 && "1".equals(parts[7]);
    sNumberDecimal = parts.length > 8 && "1".equals(parts[8]);
    sCapitalization = parts.length > 9 ? parts[9] : "";
  }

  /**
   * Upstream's {@code TextInputPlugin.inputTypeFromTextInputType}.
   *
   * <p>This is what tells the IME what kind of field it is editing, and the
   * absence of it is what stopped the keyboard composing at all: without
   * {@code TYPE_TEXT_FLAG_AUTO_CORRECT} an IME commits every keystroke as it
   * arrives, so there is no composing region to build a suggestion on and no
   * strip of candidates over the keys. Typing "wor" got "wor" and never "work".
   */
  private static int inputTypeFor() {
    switch (sInputType) {
      case "TextInputType.datetime":
        return android.text.InputType.TYPE_CLASS_DATETIME;
      case "TextInputType.number":
        return android.text.InputType.TYPE_CLASS_NUMBER
            | (sNumberSigned ? android.text.InputType.TYPE_NUMBER_FLAG_SIGNED : 0)
            | (sNumberDecimal ? android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL : 0)
            | (sObscureText ? android.text.InputType.TYPE_NUMBER_VARIATION_PASSWORD : 0);
      case "TextInputType.phone":
        return android.text.InputType.TYPE_CLASS_PHONE;
      case "TextInputType.none":
        return android.text.InputType.TYPE_NULL;
      default:
        break;
    }

    int type = android.text.InputType.TYPE_CLASS_TEXT;
    switch (sInputType) {
      case "TextInputType.multiline":
        type |= android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE;
        break;
      case "TextInputType.emailAddress":
      // Upstream puts Twitter with the email addresses rather than giving it
      // an arm of its own, which is a statement about @ being on the keyboard.
      case "TextInputType.twitter":
        type |= android.text.InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS;
        break;
      case "TextInputType.url":
      case "TextInputType.webSearch":
        type |= android.text.InputType.TYPE_TEXT_VARIATION_URI;
        break;
      case "TextInputType.visiblePassword":
        type |= android.text.InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
        break;
      case "TextInputType.name":
        type |= android.text.InputType.TYPE_TEXT_VARIATION_PERSON_NAME;
        break;
      // "address", not "streetAddress": upstream's `TextInputType._names`
      // spells this one differently from its Dart getter, and a case matching
      // the getter would never fire.
      case "TextInputType.address":
        type |= android.text.InputType.TYPE_TEXT_VARIATION_POSTAL_ADDRESS;
        break;
      default:
        break;
    }

    if (sObscureText) {
      // Upstream's comment, and it is a warning rather than a note: "both
      // required. Some devices ignore TYPE_TEXT_FLAG_NO_SUGGESTIONS."
      type |= android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
      type |= android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD;
    } else {
      if (sAutocorrect) {
        type |= android.text.InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
      }
      if (!sEnableSuggestions) {
        type |= android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        type |= android.text.InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
      }
    }

    switch (sCapitalization) {
      case "TextCapitalization.characters":
        type |= android.text.InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS;
        break;
      case "TextCapitalization.words":
        type |= android.text.InputType.TYPE_TEXT_FLAG_CAP_WORDS;
        break;
      case "TextCapitalization.sentences":
        type |= android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES;
        break;
      default:
        break;
    }
    return type;
  }

  /**
   * The enter key's action -- upstream's `enterAction` in
   * {@code createInputConnection}.
   *
   * <p>With no action asked for, upstream defaults a multi-line field to none
   * and every other to done: a field that takes several lines wants a return
   * key that inserts one.
   */
  private static int imeActionFor(int inputType) {
    switch (sInputAction) {
      case "TextInputAction.none":
        return EditorInfo.IME_ACTION_NONE;
      case "TextInputAction.go":
        return EditorInfo.IME_ACTION_GO;
      case "TextInputAction.search":
        return EditorInfo.IME_ACTION_SEARCH;
      case "TextInputAction.send":
        return EditorInfo.IME_ACTION_SEND;
      case "TextInputAction.next":
        return EditorInfo.IME_ACTION_NEXT;
      case "TextInputAction.previous":
        return EditorInfo.IME_ACTION_PREVIOUS;
      case "TextInputAction.done":
        return EditorInfo.IME_ACTION_DONE;
      default:
        return (android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE & inputType) != 0
            ? EditorInfo.IME_ACTION_NONE
            : EditorInfo.IME_ACTION_DONE;
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
    notifyInputMethod();
  }

  /**
   * Tells the IME the editing state moved -- upstream's
   * {@code InputConnectionAdaptor.didChangeEditingState}, whose own comment is
   * the reason this exists: "<b>updateSelection is mandatory.</b>
   * updateExtractedText and updateCursorAnchorInfo are on demand".
   *
   * <p>None of it was being sent. An IME that is never told where the caret is
   * or what is composing cannot keep a composition across keystrokes, so it
   * starts a new word at every letter: "wor" arrives as w, then o, then r, and
   * the keyboard never has a word in hand to finish into "work".
   *
   * <p>Posted to the view rather than called here: the framework's state
   * arrives on whichever thread edited it, and these are main-thread calls.
   */
  private static void notifyInputMethod() {
    final RustflutterActivity activity = sInstance;
    if (activity == null || activity.mView == null || !sHasClient) {
      return;
    }
    final String text = sText;
    final int selectionStart = Math.min(Math.max(sSelectionBase, 0), text.length());
    final int selectionEnd = Math.min(Math.max(sSelectionExtent, 0), text.length());
    final int composingStart = sComposingBase;
    final int composingEnd = sComposingExtent;
    activity.mView.post(
        () -> {
          InputMethodManager imm = activity.inputMethodManager();
          if (imm == null) {
            return;
          }
          // Upstream sends this every time and lets the manager decide:
          // "InputMethodManager#updateSelection skips sending the message if
          // none of the parameters have changed since the last time".
          imm.updateSelection(
              activity.mView, selectionStart, selectionEnd, composingStart, composingEnd);
          android.view.inputmethod.ExtractedTextRequest request = sExtractRequest;
          if (request != null) {
            android.view.inputmethod.ExtractedText extracted =
                new android.view.inputmethod.ExtractedText();
            extracted.text = text;
            extracted.startOffset = 0;
            extracted.partialStartOffset = -1;
            extracted.partialEndOffset = -1;
            extracted.selectionStart = selectionStart;
            extracted.selectionEnd = selectionEnd;
            imm.updateExtractedText(activity.mView, request.token, extracted);
          }
        });
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
    private AccessibilityNodeProvider mSemanticsProvider;

    HostView(Context context) {
      super(context);
    }

    void setSemanticsProvider(AccessibilityNodeProvider provider) {
      mSemanticsProvider = provider;
    }

    @Override
    public AccessibilityNodeProvider getAccessibilityNodeProvider() {
      return mSemanticsProvider;
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
      attributes.inputType = inputTypeFor();
      attributes.imeOptions =
          EditorInfo.IME_FLAG_NO_FULLSCREEN | imeActionFor(attributes.inputType);
      // Upstream's guard, kept: the flag arrived in API 26 and this package's
      // manifest goes back to 24 (`make_apk.py --min-sdk`, whose default is
      // that). javac inlines the constant so the reference itself is safe on an
      // older device; what the check is for is not asking a platform that never
      // had the flag to honour it.
      if (!sPersonalizedLearning && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        attributes.imeOptions |= EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING;
      }
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
    public boolean setComposingRegion(int start, int end) {
      // How a suggestion replaces a word: the keyboard puts the region back
      // around what was already committed and then sets its text. Upstream's
      // adaptor lets its shared `Editable` do this; there is no shared editable
      // here, so it goes to the model in C++ that is the one authority.
      nativeComposingRegion(start, end);
      return true;
    }

    @Override
    public boolean setSelection(int start, int end) {
      nativeSetSelection(start, end);
      return true;
    }

    @Override
    public boolean finishComposingText() {
      nativeComposingEnd();
      return true;
    }

    /**
     * The whole field, for an IME that would rather read it in one go.
     *
     * <p>Upstream implements this with a comment worth repeating: "When there's
     * not enough vertical screen space, the IME may enter fullscreen mode and
     * this method will be used to get (a portion of) the currently edited text.
     * <b>Samsung keyboard seems to use this method instead of
     * InputConnection#getText{Before,After}Cursor.</b>"
     *
     * <p>Which is the whole of why it is here. {@code BaseInputConnection}'s
     * default answers from the dummy editable it keeps -- always empty, because
     * nothing writes to it -- so a keyboard that reads the field this way saw an
     * empty field, had no word in hand, and offered no candidates for one.
     */
    @Override
    public android.view.inputmethod.ExtractedText getExtractedText(
        android.view.inputmethod.ExtractedTextRequest request, int flags) {
      // Upstream enables text monitoring from the same flag, and the pushes
      // it turns on are `notifyInputMethod`'s.
      sExtractRequest =
          (flags & android.view.inputmethod.InputConnection.GET_EXTRACTED_TEXT_MONITOR) != 0
              ? request
              : null;
      android.view.inputmethod.ExtractedText extracted =
          new android.view.inputmethod.ExtractedText();
      extracted.text = sText;
      extracted.startOffset = 0;
      extracted.partialStartOffset = -1;
      extracted.partialEndOffset = -1;
      extracted.selectionStart = Math.min(Math.max(sSelectionBase, 0), sText.length());
      extracted.selectionEnd = Math.min(Math.max(sSelectionExtent, 0), sText.length());
      return extracted;
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

  // -- Accessibility ----------------------------------------------------------

  /**
   * What one semantics node says, as it arrived from the framework.
   *
   * <p>A flat copy rather than a live reference: the framework's tree is rebuilt
   * every frame and is not this thread's to hold, while a screen reader asks
   * about nodes whenever it gets round to it.
   */
  private static final class SemanticsNode {
    int id;
    int actions;
    boolean button;
    boolean textField;
    boolean header;
    boolean image;
    boolean link;
    boolean obscured;
    boolean liveRegion;
    boolean checkable;
    boolean checked;
    boolean hasEnabled;
    boolean enabled;
    boolean selected;
    boolean focused;
    String label = "";
    String value = "";
    String hint = "";
    float left;
    float top;
    float right;
    float bottom;
    int[] children = new int[0];

    /** What a screen reader reads out: the name, then what it currently says. */
    CharSequence spoken() {
      StringBuilder text = new StringBuilder();
      if (label.length() > 0) {
        text.append(label);
      }
      // An obscured field's contents are exactly what must not be read aloud.
      if (!obscured && value.length() > 0) {
        if (text.length() > 0) {
          text.append(", ");
        }
        text.append(value);
      }
      return text.toString();
    }
  }

  /** Bits of flutter::SemanticsAction. Four copies of this set upstream. */
  private static final int ACTION_TAP = 1;
  private static final int ACTION_LONG_PRESS = 1 << 1;
  private static final int ACTION_SCROLL_LEFT = 1 << 2;
  private static final int ACTION_SCROLL_RIGHT = 1 << 3;
  private static final int ACTION_SCROLL_UP = 1 << 4;
  private static final int ACTION_SCROLL_DOWN = 1 << 5;
  private static final int ACTION_GAIN_ACCESSIBILITY_FOCUS = 1 << 15;
  private static final int ACTION_LOSE_ACCESSIBILITY_FOCUS = 1 << 16;

  /** The last tree the framework sent, by node id. */
  private static final SparseArray<SemanticsNode> sSemantics = new SparseArray<>();

  /** Node ids in the order they arrived, which is the order they are read in. */
  private static int[] sSemanticsOrder = new int[0];

  /** Ids that are somebody's child, so the rest are the root's. */
  private static final java.util.HashSet<Integer> sSemanticsChildren = new java.util.HashSet<>();

  /** Which node the reader's finger is on, so a change can be announced. */
  private static int sAccessibilityFocus = -1;

  /**
   * Receives one frame's semantics tree from the host.
   *
   * <p>Called from the platform thread. The nodes are copied into the static
   * table under its own lock and the view is asked to re-read, which is what
   * upstream's {@code AccessibilityBridge.updateSemantics} does with the two
   * buffers it is handed.
   */
  @SuppressWarnings("unused")
  private static void onSemanticsUpdate(String json) {
    try {
      JSONArray array = new JSONArray(json);
      SparseArray<SemanticsNode> parsed = new SparseArray<>();
      int[] order = new int[array.length()];
      java.util.HashSet<Integer> childrenOfSomething = new java.util.HashSet<>();
      for (int i = 0; i < array.length(); i++) {
        JSONObject object = array.getJSONObject(i);
        SemanticsNode node = new SemanticsNode();
        node.id = object.getInt("id");
        node.actions = object.getInt("actions");
        node.button = object.optBoolean("button");
        node.textField = object.optBoolean("textField");
        node.header = object.optBoolean("header");
        node.image = object.optBoolean("image");
        node.link = object.optBoolean("link");
        node.obscured = object.optBoolean("obscured");
        node.liveRegion = object.optBoolean("liveRegion");
        node.checkable = object.optBoolean("checkable");
        node.checked = object.optBoolean("checked");
        node.hasEnabled = object.optBoolean("hasEnabled");
        node.enabled = object.optBoolean("enabled", true);
        node.selected = object.optBoolean("selected");
        node.focused = object.optBoolean("focused");
        node.label = object.optString("label", "");
        node.value = object.optString("value", "");
        node.hint = object.optString("hint", "");
        node.left = (float) object.optDouble("left", 0);
        node.top = (float) object.optDouble("top", 0);
        node.right = (float) object.optDouble("right", 0);
        node.bottom = (float) object.optDouble("bottom", 0);
        JSONArray children = object.optJSONArray("children");
        if (children != null) {
          node.children = new int[children.length()];
          for (int c = 0; c < children.length(); c++) {
            node.children[c] = children.getInt(c);
            childrenOfSomething.add(node.children[c]);
          }
        }
        parsed.put(node.id, node);
        order[i] = node.id;
      }
      synchronized (sSemantics) {
        sSemantics.clear();
        for (int i = 0; i < parsed.size(); i++) {
          sSemantics.put(parsed.keyAt(i), parsed.valueAt(i));
        }
        sSemanticsOrder = order;
        sSemanticsChildren.clear();
        sSemanticsChildren.addAll(childrenOfSomething);
      }

      // Tell the reader the screen changed. Without this it goes on describing
      // the tree it read the first time, which is worse than saying nothing:
      // it is confidently wrong.
      final RustflutterActivity activity = sInstance;
      if (activity != null && activity.mView != null) {
        activity.mView.post(
            new Runnable() {
              @Override
              public void run() {
                if (activity.mView != null) {
                  activity.mView.sendAccessibilityEvent(
                      AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
                }
              }
            });
      }
    } catch (Exception error) {
      Log.w(TAG, "could not read the semantics update: " + error);
    }
  }

  /** A snapshot of the tree, for the provider to answer from. */
  private static SemanticsNode semanticsNode(int id) {
    synchronized (sSemantics) {
      return sSemantics.get(id);
    }
  }

  private static int[] semanticsRoots() {
    synchronized (sSemantics) {
      int count = 0;
      for (int id : sSemanticsOrder) {
        if (!sSemanticsChildren.contains(id)) {
          count++;
        }
      }
      int[] roots = new int[count];
      int at = 0;
      for (int id : sSemanticsOrder) {
        if (!sSemanticsChildren.contains(id)) {
          roots[at++] = id;
        }
      }
      return roots;
    }
  }

  /**
   * Turns the framework's tree into the one Android asks about.
   *
   * <p>Upstream this is {@code AccessibilityBridge}, and the shape is the same:
   * an {@link AccessibilityNodeProvider} over a single real {@link View}, whose
   * children are virtual and are the semantics nodes. Android calls it whenever
   * a reader's finger moves; nothing here is per-frame.
   */
  private final class SemanticsProvider extends AccessibilityNodeProvider {
    @Override
    public AccessibilityNodeInfo createAccessibilityNodeInfo(int virtualViewId) {
      if (virtualViewId == View.NO_ID) {
        // The host view itself, whose children are the top-level nodes.
        AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(mView);
        mView.onInitializeAccessibilityNodeInfo(info);
        for (int id : semanticsRoots()) {
          info.addChild(mView, id);
        }
        return info;
      }

      SemanticsNode node = semanticsNode(virtualViewId);
      if (node == null) {
        return null;
      }

      AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(mView, virtualViewId);
      info.setPackageName(getPackageName());
      info.setSource(mView, virtualViewId);
      info.setParent(mView);
      info.setClassName(className(node));
      info.setText(node.spoken());
      info.setContentDescription(node.spoken());
      if (node.hint.length() > 0) {
        info.setHintText(node.hint);
      }

      info.setVisibleToUser(true);
      info.setEnabled(!node.hasEnabled || node.enabled);
      info.setCheckable(node.checkable);
      info.setChecked(node.checked);
      info.setSelected(node.selected);
      info.setFocusable(true);
      info.setFocused(node.focused);
      info.setPassword(node.obscured);
      info.setEditable(node.textField);
      if (Build.VERSION.SDK_INT >= 28) {
        info.setHeading(node.header);
      }

      // Where on the glass. The framework works in logical pixels and Android
      // wants the view's own, so this is the one place the density comes back
      // in. Without a rectangle a reader cannot find the node by touch at all.
      float density = getResources().getDisplayMetrics().density;
      int[] origin = new int[2];
      mView.getLocationOnScreen(origin);
      Rect bounds =
          new Rect(
              Math.round(node.left * density),
              Math.round(node.top * density),
              Math.round(node.right * density),
              Math.round(node.bottom * density));
      info.setBoundsInParent(bounds);
      Rect onScreen = new Rect(bounds);
      onScreen.offset(origin[0], origin[1]);
      info.setBoundsInScreen(onScreen);

      // What the reader can do here. Accessibility focus is always offered,
      // because it is how touch exploration works rather than something the
      // application chose to support.
      info.addAction(
          sAccessibilityFocus == virtualViewId
              ? AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS
              : AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS);
      if ((node.actions & ACTION_TAP) != 0) {
        info.addAction(AccessibilityNodeInfo.ACTION_CLICK);
        info.setClickable(true);
      }
      if ((node.actions & ACTION_LONG_PRESS) != 0) {
        info.addAction(AccessibilityNodeInfo.ACTION_LONG_CLICK);
        info.setLongClickable(true);
      }
      if ((node.actions & (ACTION_SCROLL_UP | ACTION_SCROLL_LEFT)) != 0) {
        info.addAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD);
        info.setScrollable(true);
      }
      if ((node.actions & (ACTION_SCROLL_DOWN | ACTION_SCROLL_RIGHT)) != 0) {
        info.addAction(AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD);
        info.setScrollable(true);
      }

      for (int child : node.children) {
        if (semanticsNode(child) != null) {
          info.addChild(mView, child);
        }
      }
      return info;
    }

    /**
     * The Android widget a reader is told this behaves like.
     *
     * <p>Screen readers say "button" and "switch" from the class name rather
     * than from the flags, which is why upstream's bridge does the same
     * mapping.
     */
    private String className(SemanticsNode node) {
      if (node.textField) {
        return "android.widget.EditText";
      }
      if (node.checkable) {
        return "android.widget.Switch";
      }
      if (node.button) {
        return "android.widget.Button";
      }
      if (node.image) {
        return "android.widget.ImageView";
      }
      return "android.view.View";
    }

    @Override
    public boolean performAction(int virtualViewId, int action, Bundle arguments) {
      SemanticsNode node = semanticsNode(virtualViewId);
      if (node == null) {
        return false;
      }
      switch (action) {
        case AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS:
          sAccessibilityFocus = virtualViewId;
          nativeSemanticsAction(virtualViewId, ACTION_GAIN_ACCESSIBILITY_FOCUS);
          sendSemanticsEvent(virtualViewId, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED);
          return true;
        case AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS:
          if (sAccessibilityFocus == virtualViewId) {
            sAccessibilityFocus = -1;
          }
          nativeSemanticsAction(virtualViewId, ACTION_LOSE_ACCESSIBILITY_FOCUS);
          sendSemanticsEvent(
              virtualViewId, AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED);
          return true;
        case AccessibilityNodeInfo.ACTION_CLICK:
          nativeSemanticsAction(virtualViewId, ACTION_TAP);
          return true;
        case AccessibilityNodeInfo.ACTION_LONG_CLICK:
          nativeSemanticsAction(virtualViewId, ACTION_LONG_PRESS);
          return true;
        case AccessibilityNodeInfo.ACTION_SCROLL_FORWARD:
          nativeSemanticsAction(
              virtualViewId,
              (node.actions & ACTION_SCROLL_UP) != 0 ? ACTION_SCROLL_UP : ACTION_SCROLL_LEFT);
          return true;
        case AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD:
          nativeSemanticsAction(
              virtualViewId,
              (node.actions & ACTION_SCROLL_DOWN) != 0 ? ACTION_SCROLL_DOWN : ACTION_SCROLL_RIGHT);
          return true;
        default:
          return false;
      }
    }

    @Override
    public AccessibilityNodeInfo findFocus(int focus) {
      if (focus == AccessibilityNodeInfo.FOCUS_ACCESSIBILITY && sAccessibilityFocus != -1) {
        return createAccessibilityNodeInfo(sAccessibilityFocus);
      }
      return null;
    }
  }

  private void sendSemanticsEvent(int virtualViewId, int type) {
    if (mView == null) {
      return;
    }
    AccessibilityEvent event = AccessibilityEvent.obtain(type);
    event.setPackageName(getPackageName());
    event.setSource(mView, virtualViewId);
    android.view.accessibility.AccessibilityManager manager =
        (android.view.accessibility.AccessibilityManager)
            getSystemService(Context.ACCESSIBILITY_SERVICE);
    if (manager != null && manager.isEnabled()) {
      manager.sendAccessibilityEvent(event);
    }
  }

  /**
   * Watches for an accessibility service arriving or leaving, and tells the
   * framework whether to build a semantics tree at all.
   *
   * <p>The gate is {@code isEnabled} rather than touch exploration, which is
   * upstream's choice too ({@code AccessibilityBridge}'s
   * {@code accessibilityStateChangeListener}): a screen reader is not the only
   * thing that reads an accessibility tree, and a service that is not a screen
   * reader still deserves an answer. Touch exploration is watched separately,
   * because it is what decides whether a reader is dragging a finger around --
   * a different question from whether anybody is listening.
   */
  private void watchAccessibility() {
    AccessibilityManager manager =
        (AccessibilityManager) getSystemService(Context.ACCESSIBILITY_SERVICE);
    if (manager == null) {
      return;
    }
    mAccessibilityManager = manager;
    mAccessibilityListener =
        new AccessibilityManager.AccessibilityStateChangeListener() {
          @Override
          public void onAccessibilityStateChanged(boolean enabled) {
            nativeSemanticsEnabled(enabled);
            if (!enabled) {
              forgetSemantics();
            }
          }
        };
    manager.addAccessibilityStateChangeListener(mAccessibilityListener);

    mTouchExplorationListener =
        new AccessibilityManager.TouchExplorationStateChangeListener() {
          @Override
          public void onTouchExplorationStateChanged(boolean enabled) {
            if (!enabled) {
              sAccessibilityFocus = -1;
            }
          }
        };
    manager.addTouchExplorationStateChangeListener(mTouchExplorationListener);

    nativeSemanticsEnabled(manager.isEnabled());
  }

  /** Undoes watchAccessibility. Called once, from onDestroy. */
  private void stopWatchingAccessibility() {
    if (mAccessibilityManager == null) {
      return;
    }
    if (mTouchExplorationListener != null) {
      mAccessibilityManager.removeTouchExplorationStateChangeListener(mTouchExplorationListener);
      mTouchExplorationListener = null;
    }
    if (mAccessibilityListener != null) {
      mAccessibilityManager.removeAccessibilityStateChangeListener(mAccessibilityListener);
      mAccessibilityListener = null;
    }
  }

  private static void forgetSemantics() {
    synchronized (sSemantics) {
      sSemantics.clear();
      sSemanticsOrder = new int[0];
      sSemanticsChildren.clear();
    }
    sAccessibilityFocus = -1;
  }

  private AccessibilityManager mAccessibilityManager;
  private AccessibilityManager.AccessibilityStateChangeListener mAccessibilityListener;
  private AccessibilityManager.TouchExplorationStateChangeListener mTouchExplorationListener;

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

  private static native void nativeSurfaceDestroyed();

  private static native void nativeSurfaceRecreated(
      Surface surface, int width, int height, float devicePixelRatio);

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

  private static native void nativeComposingRegion(int start, int end);

  private static native void nativeSetSelection(int start, int end);

  private static native boolean nativeEditingKey(int keyCode, boolean shift);

  private static native void nativeEditorAction();

  private static native boolean nativeBackPressed();

  private static native void nativeSemanticsEnabled(boolean enabled);

  private static native void nativeSemanticsAction(int nodeId, int action);
}
