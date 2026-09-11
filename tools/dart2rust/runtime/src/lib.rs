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
    /// Frames that panicked part-way (see `schedule_frame`).
    static FRAME_PANICS: RefCell<i64> = RefCell::new(0);
    static FRAME_PENDING: RefCell<bool> = RefCell::new(false);
    static MESSAGES: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

/// Installs the headless engine: the native host, then the view the
/// engine would have announced before `main` ran.
pub fn install() {
    install_panic_report();
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
    // `DART2RUST_DUMP_RENDER_TREE=1`: the render tree as the translated
    // program describes it (`toStringDeep`), for the ruler against
    // Flutter's own `debugDumpRenderTree` of the same page.
    if std::env::var_os("DART2RUST_DUMP_RENDER_TREE").is_some() {
        match dump_render_tree() {
            Ok(text) => {
                eprintln!("=== DART2RUST RENDER TREE BEGIN ===");
                eprintln!("{}", text);
                eprintln!("=== DART2RUST RENDER TREE END ===");
            }
            Err(e) => eprintln!(
                "dart2rust runtime: dumping the render tree threw: {}",
                dart_error_text(&e)
            ),
        }
    }
    // `DART2RUST_DUMP_APP=1`: the element tree (`Element.toStringDeep`),
    // for where the widget tree got to when the render tree is empty.
    if std::env::var_os("DART2RUST_DUMP_APP").is_some() {
        match dump_app() {
            Ok(text) => {
                eprintln!("=== DART2RUST ELEMENT TREE BEGIN ===");
                eprintln!("{}", text);
                eprintln!("=== DART2RUST ELEMENT TREE END ===");
            }
            Err(e) => eprintln!(
                "dart2rust runtime: dumping the element tree threw: {}",
                dart_error_text(&e)
            ),
        }
    }
    // `DART2RUST_COUNT_COPIES=1`: what the `Iterable` handle costs in
    // whole-collection copies (`copy_counts_report`). Empty when off.
    let copies = dart_prelude::copy_counts_report();
    if !copies.is_empty() {
        eprint!("{}", copies);
    }
    // `DART2RUST_TRACE_CAST=1`: every `dart_cast_to` that answered nothing,
    // with what it was asked for and what the object was. Empty when off.
    let misses = dart_prelude::cast_miss_report();
    if !misses.is_empty() {
        eprint!("{}", misses);
    }
    let frames = FRAMES.with(|f| *f.borrow());
    let panicked = FRAME_PANICS.with(|f| *f.borrow());
    let messages = MESSAGES.with(|m| m.borrow().clone());
    eprintln!(
        "dart2rust runtime: {} frame(s) drawn ({} panicked); {} platform message(s): {}",
        frames,
        panicked,
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
    // The engine names the implicit view before adding it: `dart:ui`'s
    // top-level `_implicitViewId`, written from C++ (`runApp` renders into
    // it; "the platform did not provide one", run519).
    // Through `DartLazy::get`, which is what a lazy static is now: Dart
    // re-runs an initialiser that threw, so the read is fallible (ws1072).
    *dart_ui::_IMPLICIT_VIEW_ID.get()?.borrow_mut() = Some(VIEW_ID);
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

fn int(value: i64) -> Rc<dyn DartAny> {
    Rc::new(value) as Rc<dyn DartAny>
}

fn string(value: &str) -> Rc<dyn DartAny> {
    Rc::new(value.to_string()) as Rc<dyn DartAny>
}

fn null() -> Rc<dyn DartAny> {
    Rc::new(Null) as Rc<dyn DartAny>
}

/// Under `panic = "abort"` (the workspace's profile: unwind landing pads
/// cost the widget-tree functions half an hour of codegen) nothing can
/// catch a panic, so the report is made *in* the panic hook: the message,
/// then the tree dumps the run asked for (`DART2RUST_DUMP_*`), which is
/// what a stub on the compositing path was hiding (run554). A borrow the
/// panicking frame still holds makes the dump panic in turn, which aborts
/// exactly as the first panic was about to.
pub fn install_panic_report() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        eprintln!("dart2rust host: reporting from the panic hook");
        report();
    }));
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
        // `DART2RUST_TRACE_FRAMES=1`: one line per frame as it starts and
        // as it ends, with the wall clock, so a run that produces nothing
        // says whether it is drawing slowly or stuck inside one frame.
        let traced = std::env::var("DART2RUST_TRACE_FRAMES").as_deref() == Ok("1");
        // `DART2RUST_TREE_EVERY=n`: the size of the render tree every n
        // frames. The ruler reads that tree at the end of a wall-clock
        // budget and gets 707 nodes or 2 depending on when it lands, and
        // 45s/60s/90s budgets do not order the two -- so the tree is
        // emptied and rebuilt on some cycle, and this says on which.
        if let Ok(every) = std::env::var("DART2RUST_TREE_EVERY") {
            if let Ok(every) = every.parse::<i64>() {
                if every > 0 && number % every == 0 {
                    let size = match dump_render_tree() {
                        Ok(text) => text.lines().filter(|l| !l.is_empty()).count(),
                        Err(_) => 0,
                    };
                    eprintln!("dart2rust tree: frame {} nodes {}", number, size);
                }
            }
        }
        let started = std::time::Instant::now();
        if traced {
            eprintln!("dart2rust frame {} begin at {}us", number, micros);
        }
        // A frame that panics -- a stub on the compositing path, say -- is
        // reported and the run goes on to `report()`, so the trees built
        // and laid out before the panic can still be dumped and compared
        // (run553: the render tree was there and never printed). The
        // unwinding drops every `RefMut` the frame held.
        let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dart_ui::_begin_frame(micros, number)?;
            dart_ui::_draw_frame()
        }));
        if traced {
            eprintln!(
                "dart2rust frame {} end after {:?}",
                number,
                started.elapsed()
            );
        }
        match frame {
            Ok(result) => result,
            Err(payload) => {
                let text = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "(no message)".to_string());
                eprintln!("dart2rust host: frame {} panicked: {}", number, text);
                FRAME_PANICS.with(|p| *p.borrow_mut() += 1);
                Ok(())
            }
        }
    }));
}

type MessageCallback = Rc<dyn Fn(Option<ByteData>) -> Result<(), DartError>>;

/// The reply callback a platform message came with: the typed closure, or
/// Dart's `Function` object (a closure handed across a `dynamic` slot goes
/// behind one), called through the prelude's dynamic call.
fn message_callback(object: &Rc<dyn DartAny>) -> Option<MessageCallback> {
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
            let reply: Rc<dyn DartAny> = match bytes {
                Some(bytes) => Rc::new(bytes) as Rc<dyn DartAny>,
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
fn send_platform_message(args: &[Rc<dyn DartAny>]) {
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
            let state: Map<Rc<dyn DartAny>, Rc<dyn DartAny>> = Map::new();
            Ok(Some(standard_codec().encode_success_envelope(
                Rc::new(state) as Rc<dyn DartAny>,
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
        // The asset bundle, as the engine's embedder serves it: the key
        // (UTF-8) is a path under the assets directory `DART2RUST_ASSETS`
        // (a `flutter build`'s `flutter_assets`), the reply is the file's
        // bytes, and no reply is "no such asset" -- the gallery's locale
        // names come from `packages/flutter_localized_locales/data/en.json`
        // (run601).
        "flutter/assets" => {
            let key = match data {
                Some(bytes) => String::from_utf8_lossy(&bytes.dart_bytes()).to_string(),
                None => return Ok(None),
            };
            let root = match std::env::var_os("DART2RUST_ASSETS") {
                Some(root) => std::path::PathBuf::from(root),
                None => return Ok(None),
            };
            if key.split('/').any(|part| part == "..") {
                return Ok(None);
            }
            match std::fs::read(root.join(&key)) {
                Ok(bytes) => Ok(Some(ByteData::view(bytes, 0, None))),
                Err(_) => Ok(None),
            }
        }
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
                Some(path) => Rc::new(path) as Rc<dyn DartAny>,
                None => dart_null_object(),
            };
            Ok(Some(codec.encode_success_envelope(result)?))
        }
        _ => Ok(None),
    }
}

/// The render tree as a structural walk -- one line per render object:
/// its runtime type, its size when it is a laid-out box, its offset in
/// its parent when its parent data is box parent data -- which is what
/// `~/gallery_upstream/test/dump_render_walk_test.dart` prints from
/// Flutter itself. Not `toStringDeep`: Flutter renders that inside an
/// `assert`, which the release dill drops, so it is empty here (run554).
fn dump_render_tree() -> Result<String, DartError> {
    use generated::rendering_box::{BoxParentData, RenderBox};
    use generated::rendering_object::RenderObject;
    let binding = generated::rendering_binding::renderer_binding_instance()?;
    let out = Rc::new(RefCell::new(String::new()));
    fn walk(
        node: Rc<dyn RenderObject>,
        depth: usize,
        out: Rc<RefCell<String>>,
    ) -> Result<(), DartError> {
        let mut line = format!(
            "{}{}",
            "  ".repeat(depth),
            (&*node).dart_runtime_type().name
        );
        if let Some(bx) = node.dart_cast_to::<dyn RenderBox>() {
            if bx.has_size()? {
                line.push_str(&format!(" size={}", bx.size()?.dart_to_string()));
            }
        }
        if let Some(data) = RenderObject::parent_data(&*node)? {
            if let Some(bpd) = data.dart_cast_to::<dyn BoxParentData>() {
                line.push_str(&format!(" offset={}", bpd.offset()?.dart_to_string()));
            }
        }
        line.push('\n');
        out.borrow_mut().push_str(&line);
        if depth > 200 {
            return Ok(());
        }
        let out_child = out.clone();
        RenderObject::visit_children(
            &*node,
            Rc::new(move |child: Rc<dyn RenderObject>| walk(child, depth + 1, out_child.clone())),
        )
    }
    // `renderViews` is a Dart `Iterable`, which is a `Rc<dyn DartIterable>`
    // here: its list is what a Rust `for` walks.
    for view in binding.render_views()?.dart_to_list() {
        walk(view as Rc<dyn RenderObject>, 0, out.clone())?;
    }
    let text = out.borrow().clone();
    Ok(text)
}

fn dump_app() -> Result<String, DartError> {
    use generated::widgets_framework::Element;
    let binding = generated::widgets_binding::widgets_binding_instance()?;
    let views = generated::rendering_binding::renderer_binding_instance()?
        .render_views()?
        .dart_to_list();
    eprintln!("dart2rust runtime: {} render view(s)", views.len());
    // The tree walked by hand -- `visitChildren` and each widget's runtime
    // type -- since `toStringDeep` overflowed the stack (run534).
    let out = Rc::new(RefCell::new(String::new()));
    fn walk(
        element: Rc<dyn Element>,
        depth: usize,
        out: Rc<RefCell<String>>,
    ) -> Result<(), DartError> {
        let widget = Element::widget(&*element)?;
        let line = format!(
            "{}{} ({})\n",
            "  ".repeat(depth),
            (&*widget).dart_runtime_type().name,
            (&*element).dart_runtime_type().name
        );
        out.borrow_mut().push_str(&line);
        if depth > 200 {
            return Ok(());
        }
        let out_child = out.clone();
        element.visit_children(Rc::new(move |child: Rc<dyn Element>| {
            walk(child, depth + 1, out_child.clone())
        }))
    }
    match binding.root_element()? {
        Some(root) => {
            walk(root, 0, out.clone())?;
            let text = out.borrow().clone();
            Ok(text)
        }
        None => Ok("<no root element>".to_string()),
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

fn answer(symbol: &str, args: Vec<Rc<dyn DartAny>>) -> Result<Option<Rc<dyn DartAny>>, DartError> {
    // `DART2RUST_TRACE_HOST=1`: every native the program reaches, by symbol.
    if std::env::var_os("DART2RUST_TRACE_HOST").is_some() {
        eprintln!("dart2rust host: native {}", symbol);
    }
    let answer: Option<Rc<dyn DartAny>> = match symbol {
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
