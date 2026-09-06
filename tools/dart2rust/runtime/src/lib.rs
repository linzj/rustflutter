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

/// A platform message: no plugin is here, so the reply is Dart's null,
/// delivered on the next turn as the engine's would be.
fn send_platform_message(args: &[Rc<dyn Object>]) {
    let name = args
        .get(0)
        .and_then(|a| a.as_any().downcast_ref::<String>().cloned())
        .unwrap_or_default();
    MESSAGES.with(|m| m.borrow_mut().push(name));
    let callback = args.get(1).and_then(|a| {
        let any = a.as_any();
        any.downcast_ref::<MessageCallback>().cloned().or_else(|| {
            any.downcast_ref::<Option<MessageCallback>>()
                .cloned()
                .flatten()
        })
    });
    if let Some(callback) = callback {
        Timer::run(Rc::new(move || callback(None)));
    }
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
