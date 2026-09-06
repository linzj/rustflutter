//! The runtime's first layer: a headless engine.
//!
//! The translated program sees only the Dart side of `dart:ui`; every
//! `external` member reaches the prelude's native boundary (`dart_native`)
//! by its `@Native` symbol. This crate answers as an engine would when
//! there is no window: a root isolate, one implicit view of a fixed size,
//! frames scheduled and driven, platform messages answered "no plugin",
//! and (later) a scene recorded rather than drawn. What it does not
//! answer falls back to the absent engine's value, recorded.
//!
//! `generated.rs` is written by `bin/workspace.py` and names the crate
//! holding `dart:ui`'s translation, whose upward hooks (`_addView`,
//! `_updateWindowMetrics`, `_beginFrame`, `_drawFrame`, ..) this crate
//! calls the way the engine's `PlatformConfiguration` does.
#![allow(warnings)]

use dart_prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

mod generated;
use generated::dart_ui;
use generated::services_message_codecs::{
    JSONMethodCodec, StandardMessageCodec, StandardMethodCodec,
};

const VIEW_ID: i64 = 0;
const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

thread_local! {
    static FRAMES: RefCell<i64> = RefCell::new(0);
    static FRAME_PENDING: RefCell<bool> = RefCell::new(false);
    static MESSAGES: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

/// Installs the headless engine: the native host, then the view the
/// engine would have announced before `main` ran.
pub fn install() {
    set_native_host(Box::new(answer));
    if let Err(e) = announce_view() {
        eprintln!(
            "dart2rust runtime: announcing the view threw: {}",
            dart_error_text(&e)
        );
    }
}

/// What the run did, for the ruler: frames drawn, platform messages seen.
pub fn report() {
    let frames = FRAMES.with(|f| *f.borrow());
    let messages = MESSAGES.with(|m| m.borrow().clone());
    eprintln!(
        "dart2rust runtime: {} frame(s) drawn; {} platform message(s): {}",
        frames,
        messages.len(),
        messages.join(", ")
    );
}

fn announce_view() -> Result<(), DartError> {
    // The engine tells the isolate its lifecycle state before `main`: the
    // framework reads it as a `late` field (run478).
    dart_ui::_update_initial_lifecycle_state("AppLifecycleState.resumed".to_string())?;
    dart_ui::_update_locales(vec![
        "en".to_string(),
        "US".to_string(),
        "".to_string(),
        "".to_string(),
    ])?;
    dart_ui::_update_user_settings_data(
        "{\"textScaleFactor\":1.0,\"alwaysUse24HourFormat\":false,\"platformBrightness\":\"light\"}".to_string(),
    )?;
    dart_ui::_add_view(
        VIEW_ID,
        1.0,
        WIDTH,
        HEIGHT,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        18.0,
        vec![],
        vec![],
        vec![],
        0,
        WIDTH,
        WIDTH,
        HEIGHT,
        HEIGHT,
        0.0,
        0.0,
        0.0,
        0.0,
    )?;
    Ok(())
}

fn int(value: i64) -> Rc<dyn Object> {
    Rc::new(value) as Rc<dyn Object>
}

fn string(value: &str) -> Rc<dyn Object> {
    Rc::new(value.to_string()) as Rc<dyn Object>
}

fn null() -> Rc<dyn Object> {
    Rc::new(Null) as Rc<dyn Object>
}

/// A frame: the engine's vsync, as a timer that fires once the program
/// yields, then `_beginFrame` and `_drawFrame` as the engine would call
/// them.
fn schedule_frame() {
    let already = FRAME_PENDING.with(|p| std::mem::replace(&mut *p.borrow_mut(), true));
    if already {
        return;
    }
    Timer::run(Rc::new(|| {
        FRAME_PENDING.with(|p| *p.borrow_mut() = false);
        let number = FRAMES.with(|f| {
            *f.borrow_mut() += 1;
            *f.borrow()
        });
        let micros = Timeline::now();
        dart_ui::_begin_frame(micros, number)?;
        dart_ui::_draw_frame()?;
        Ok(())
    }));
}

type MessageCallback = Rc<dyn Fn(Option<ByteData>) -> Result<(), DartError>>;

/// The reply callback a platform message came with: the typed closure, or
/// Dart's `Function` object (a closure handed across a `dynamic` slot goes
/// behind one), called through the prelude's dynamic call.
fn message_callback(object: &Rc<dyn Object>) -> Option<MessageCallback> {
    let any = object.as_any();
    if let Some(callback) = any.downcast_ref::<MessageCallback>() {
        return Some(callback.clone());
    }
    if let Some(callback) = any.downcast_ref::<Option<MessageCallback>>() {
        return callback.clone();
    }
    if any.downcast_ref::<DartFunction>().is_some() {
        let function = object.clone();
        return Some(Rc::new(move |bytes: Option<ByteData>| {
            let reply: Rc<dyn Object> = match bytes {
                Some(bytes) => Rc::new(bytes) as Rc<dyn Object>,
                None => dart_null_object(),
            };
            dart_call_function(function.clone(), vec![reply]).map(|_| ())
        }));
    }
    None
}

/// A platform message: answered by the plugin this runtime hosts for the
/// channel, else by Dart's null ("no plugin"), delivered on the next turn
/// as the engine's would be.
fn send_platform_message(args: &[Rc<dyn Object>]) {
    let name = args
        .get(0)
        .and_then(|a| a.as_any().downcast_ref::<String>().cloned())
        .unwrap_or_default();
    MESSAGES.with(|m| m.borrow_mut().push(name.clone()));
    if std::env::var_os("DART2RUST_TRACE_MESSAGES").is_some() {
        eprintln!("dart2rust runtime: platform message on {}", name);
    }
    let callback = args.get(1).and_then(message_callback);
    let data = args.get(2).and_then(|a| {
        let any = a.as_any();
        any.downcast_ref::<ByteData>()
            .cloned()
            .or_else(|| any.downcast_ref::<Option<ByteData>>().cloned().flatten())
    });
    let reply = match plugin_reply(&name, data) {
        Ok(reply) => reply,
        Err(error) => {
            eprintln!(
                "dart2rust runtime: the plugin for {} threw: {}",
                name,
                dart_error_text(&error)
            );
            None
        }
    };
    if std::env::var_os("DART2RUST_TRACE_MESSAGES").is_some() {
        match &reply {
            Some(bytes) => {
                let hex: Vec<String> = bytes
                    .dart_bytes()
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect();
                eprintln!("dart2rust runtime: {} reply: {}", name, hex.join(" "));
            }
            None => eprintln!("dart2rust runtime: {} reply: none", name),
        }
    }
    if let Some(callback) = callback {
        Timer::run(Rc::new(move || callback(reply.clone())));
    }
}

/// The plugins an embedder registers, hosted here: `path_provider`'s
/// method channel, answered with directories under the XDG data home
/// (what `path_provider_linux` does on the Dart side).
fn standard_codec() -> StandardMethodCodec {
    StandardMethodCodec {
        message_codec: dart_rc(StandardMessageCodec {
            __self: DartSelf::new(),
        }),
    }
}

fn plugin_reply(channel: &str, data: Option<ByteData>) -> Result<Option<ByteData>, DartError> {
    if std::env::var_os("DART2RUST_TRACE_MESSAGES").is_some() {
        if let Some(bytes) = &data {
            let hex: Vec<String> = bytes
                .dart_bytes()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            eprintln!("dart2rust runtime: {} bytes: {}", channel, hex.join(" "));
        }
    }
    match channel {
        // The embedder's own channels (`SystemChannels`), by name and codec:
        // answered with success and nothing, as an embedder with nothing to
        // say does -- a plain `MethodChannel` throws `MissingPluginException`
        // on no reply, and `Title` sets the switcher description while the
        // first frame builds (run507).
        "flutter/platform"
        | "flutter/textinput"
        | "flutter/navigation"
        | "flutter/undomanager"
        | "flutter/localization"
        | "flutter/spellcheck"
        | "flutter/scribe" => Ok(Some(
            JSONMethodCodec {}.encode_success_envelope(dart_null_object())?,
        )),
        "flutter/keyboard" => {
            // `getKeyboardState`: no key is down.
            let state: Map<Rc<dyn Object>, Rc<dyn Object>> = Map::new();
            Ok(Some(standard_codec().encode_success_envelope(
                Rc::new(state) as Rc<dyn Object>,
            )?))
        }
        "flutter/menu"
        | "flutter/mousecursor"
        | "flutter/backgesture"
        | "flutter/platform_views"
        | "flutter/processtext"
        | "flutter/contextmenu"
        | "flutter/restoration" => Ok(Some(
            standard_codec().encode_success_envelope(dart_null_object())?,
        )),
        "plugins.flutter.io/path_provider" => {
            let codec = standard_codec();
            let call = codec.decode_method_call(data)?;
            let directory = match call.method.as_str() {
                "getApplicationDocumentsDirectory" => Some(app_dir("documents")),
                "getApplicationSupportDirectory" => Some(app_dir("support")),
                "getLibraryDirectory" => Some(app_dir("library")),
                "getApplicationCachePath" | "getApplicationCacheDirectory" => {
                    Some(app_dir("cache"))
                }
                "getDownloadsDirectory" => Some(app_dir("downloads")),
                "getTemporaryDirectory" => Some(temp_dir()),
                _ => return Ok(None),
            };
            // The envelope's `Object?` result is a `dynamic` here, whose
            // null is the `Null` object.
            let result = match directory {
                Some(path) => Rc::new(path) as Rc<dyn Object>,
                None => dart_null_object(),
            };
            Ok(Some(codec.encode_success_envelope(result)?))
        }
        _ => Ok(None),
    }
}

fn app_dir(kind: &str) -> String {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}/.local/share",
                std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
            )
        });
    let path = format!("{}/dart2rust_app/{}", base, kind);
    let _ = std::fs::create_dir_all(&path);
    path
}

fn temp_dir() -> String {
    let path = std::env::temp_dir().join("dart2rust_app");
    let _ = std::fs::create_dir_all(&path);
    path.to_string_lossy().into_owned()
}

fn answer(symbol: &str, args: Vec<Rc<dyn Object>>) -> Result<Option<Rc<dyn Object>>, DartError> {
    let answer: Option<Rc<dyn Object>> = match symbol {
        "PlatformConfigurationNativeApi::GetRootIsolateToken" => Some(int(1)),
        "PlatformConfigurationNativeApi::DefaultRouteName" => Some(string("/")),
        "PlatformConfigurationNativeApi::GetPersistentIsolateData" => Some(null()),
        "PlatformConfigurationNativeApi::ScheduleFrame" => {
            schedule_frame();
            Some(null())
        }
        "PlatformConfigurationNativeApi::SendPlatformMessage" => {
            send_platform_message(&args);
            Some(null())
        }
        "PlatformConfigurationNativeApi::RespondToPlatformMessage"
        | "PlatformConfigurationNativeApi::SetNeedsReportTimings"
        | "PlatformConfigurationNativeApi::SetIsolateDebugName"
        | "PlatformConfigurationNativeApi::Render"
        | "PlatformConfigurationNativeApi::UpdateSemantics" => Some(null()),
        _ => None,
    };
    Ok(answer)
}
