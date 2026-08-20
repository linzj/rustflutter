// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! rustflutter -- a Rust UI framework on the Flutter engine.
//!
//! The engine's rendering, compositing, text-layout and threading stack is used
//! unmodified; the Dart VM, `dart:ui` and `packages/flutter` are replaced by
//! this crate.
//!
//! ```no_run
//! use rustflutter::prelude::*;
//!
//! let ui = Center::new(Text::new("Hello, World!").with_size(48.0));
//! App::new(800, 600).with_background(Color::WHITE).render_to_png(ui, "hello.png").unwrap();
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

// Compiled for this crate's own `#[test]` binary, and for anything built with
// `--cfg rustflutter_stubs` -- which is how a crate that depends on this one
// gets its tests linked without the whole C++ engine behind them.
#[cfg(any(test, rustflutter_stubs))]
mod engine_test_stubs;

pub mod about;
pub mod actions;
pub mod animated_scroll_view;
pub mod animation;
pub mod app;
pub mod arc;
pub mod assertions;
pub mod r#async;
pub mod autocomplete;
pub mod borders;
pub mod color_scheme;
pub mod colors;
pub mod component_themes;
pub mod components;
pub mod controls;
pub mod cupertino;
pub mod cupertino_route;
pub mod decoration;
pub mod diagnostics;
pub mod direction;
pub mod display_feature;
pub mod drag_boundary;
pub mod drag_target;
pub mod draggable_sheet;
pub mod drawer;
pub mod dual_transition_builder;
pub mod editable;
pub mod editable_text;
pub mod elevation_overlay;
pub mod engine;
pub mod expansion_panel;
pub mod fab_location;
pub mod flexible_space_bar;
pub mod focus;
pub mod foundation;
pub mod framework;
pub mod gesture_details;
pub mod gestures;
pub mod grid;
pub mod icon_data;
pub mod image;
pub mod implicit;
pub mod ink;
pub mod ink_well;
pub mod interactive_viewer;
pub mod keyboard;
pub mod licenses;
pub mod list_wheel;
pub mod magnifier;
pub mod material;
pub mod media_query;
pub mod menu;
pub mod menu_anchor;
pub mod mergeable_material;
pub mod motion;
pub mod multidrag;
pub mod multitap;
pub mod navigation;
pub mod navigation_destinations;
pub mod navigator;
pub mod nested_scroll_view;
pub mod overflow_bar;
pub mod page_storage;
pub mod painting;
pub mod physics;
pub mod pickers;
pub mod platform;
pub mod platform_menu_bar;
pub mod preferred_size;
pub mod range_slider_parts;
pub mod recognizers;
pub mod render;
pub mod reorderable_list;
pub mod resampler;
pub mod router;
pub mod routes;
pub mod scaffold_messenger;
pub mod scrollable_helpers;
pub mod scrollbar;
pub mod scrolling;
pub mod selectable_region;
pub mod selection;
pub mod selection_container;
pub mod semantics;
pub mod services;
pub mod shortcuts;
pub mod slider_theme;
pub mod sliver;
pub mod snack_bar;
pub mod stack_frame;
pub mod tap_and_drag;
pub mod tap_region;
pub mod text_editing_intents;
pub mod text_selection;
pub mod text_selection_controls;
pub mod theme;
pub mod ticker;
pub mod transitions;
pub mod two_dimensional;
pub mod undo_history;
pub mod widget_inspector;
pub mod widget_state;
pub mod widgets;

pub use actions::{Action, ActionDispatcher, Intent};
pub use animation::{
    AlwaysStoppedAnimation, Animatable, Animation, AnimationListener, AnimationMax, AnimationMean,
    AnimationMin, AnimationStatus, AnimationStyle, Animations, ConstantTween, Controller,
    CurveTween, CurvedAnimation, Direction, FlippedTweenSequence, FloatTween, IntTween,
    OffsetTween, ProxyAnimation, RectTween, Repeat, ReverseAnimation, ReverseTween, SizeTween,
    StepTween, Tween, TweenSequence, TweenSequenceItem,
};
#[cfg(not(test))]
pub use app::run;
pub use app::{
    Application, ApplicationFactory, BuildContext, FrameContext, FrameScheduler, RunOptions,
    ViewMetrics, WidgetApplication, WidgetHost, register_application,
};
pub use r#async::{AsyncSnapshot, ConnectionState, async_builder};
pub use borders::{
    BeveledRectangleBorder, Border, BorderDirectional, BorderRadius, BorderRadiusDirectional,
    BorderRadiusGeometry, BorderSide, BorderStyle, BoxBorder, BoxShape, CircleBorder,
    ContinuousRectangleBorder, EdgeInsetsGeometry, LinearBorder, LinearBorderEdge, NotchedShape,
    OvalBorder, Radius, RoundedRectangleBorder, RoundedSuperellipseBorder, STROKE_ALIGN_CENTER,
    STROKE_ALIGN_INSIDE, STROKE_ALIGN_OUTSIDE, ShapeBorder, ShapeDecoration, StadiumBorder,
    StarBorder, TableBorder,
};
pub use components::{
    AppBar, Badge, Button, ButtonGroupState, ButtonVariant, Card, Divider, IdSource, Label,
    LabelStyle, ListTile, ProgressBar, Scaffold, Slider, Switch, Theme, theme_of,
};
pub use controls::{
    Banner, BottomNavigation, BottomSheet, Checkbox, Chip, ChipStyle, DataTable, Destination,
    Dialog, GridList, NavigationRail, Radio, Scrim, Section, Snackbar, Spinner, TabBar, Tooltip,
    TooltipTrigger, TooltipTriggerMode,
};
pub use cupertino::{
    CupertinoActivityIndicator, CupertinoAlertAction, CupertinoAlertDialog, CupertinoButton,
    CupertinoButtonSize, CupertinoColors, CupertinoContextMenu, CupertinoContextMenuAction,
    CupertinoContextMenuSheet, CupertinoDynamicColor, CupertinoNavigationBar,
    CupertinoPageScaffold, CupertinoPicker, CupertinoScrollbar, CupertinoSearchTextField,
    CupertinoSegmentedControl, CupertinoSlider, CupertinoSwitch, CupertinoTabBar, CupertinoTabItem,
    CupertinoTabScaffold, CupertinoTheme, cupertino_theme_of,
};
pub use decoration::{BoxDecoration, Decoration, FlutterLogoDecoration, FlutterLogoStyle};
pub use direction::{TextDirection, current_direction, direction_of, directionality};
pub use drawer::{Drawer, DrawerAlignment};
pub use editable::{RenderEditable, TextField, TextFieldState};
pub use engine::{
    Canvas, Color, DisplayList, LayerTree, Paint, Paragraph, Rect, RenderError, Style, TextAlign,
    TextStyle,
};
pub use focus::{Focus, KeyResult, focusable};
pub use foundation::{
    ChangeNotifier, Listenable, ListenableMerge, ValueNotifier, keys as foundation_keys,
};
pub use framework::{
    AnyWidget, Component, ElementTree, GlobalKey, Key, RenderWidget, StateHandle,
    StatefulComponent, component, keyed_leaf, keyed_many, keyed_single, leaf, many, provide,
    single, stateful, with_global_key,
};
pub use grid::{
    GridView, RenderSliverGrid, SliverGridDelegate, SliverGridGeometry, SliverGridRegularTileLayout,
};
pub use image::{
    AssetBundle, ImageChunkEvent, ImageConfiguration, ImageInfo, ImageProvider, ImageStream,
    ImageStreamCompleter, ImageStreamListener, NetworkImageLoadException, ResizeImagePolicy,
    set_root_bundle,
};
pub use implicit::{Animated, Lerp, animated};
pub use ink::{Ink, ink};
pub use interactive_viewer::{
    Affine2D, InteractiveViewer, InteractiveViewerState, PanAxis, TransformationController,
    interactive_viewer,
};
pub use keyboard::{KeyChange, KeyEvent, Keyboard, LogicalKey, PhysicalKey};
pub use media_query::{
    MediaQuery, MediaQueryData, SafeArea, current_text_scale, media_query_of, safe_area,
};
pub use menu::{
    CheckedPopupMenuItem, PopupMenu, PopupMenuButton, PopupMenuDivider, PopupMenuEntry,
    PopupMenuItem, PopupMenuPosition, popup_menu_offset,
};
pub use navigation::{
    Motion, Navigator, Presentation, Route, RouteArgs, Transition, TransitionOffsets,
};
pub use painting::{
    Accumulator, Affine, BlendMode, ClipBehavior, ClipOp, ColorSwatch, DecorationImage, FillType,
    Gradient, GradientTransform, HSLColor, HSVColor, Image, InlineSpanSemanticsInformation,
    LinearGradient, Matrix4, PlaceholderAlignment, PlaceholderDimensions, RadialGradient,
    RenderPath, ShaderGradient, StrokeCap, StrokeJoin, StrutStyle, SweepGradient, TextBaseline,
    TextPainter, TextScaler, TileMode, WordBoundary, matrix_utils,
};
pub use pickers::{
    CalendarDatePicker, CalendarDatePickerState, Date, DatePickerDialog, DatePickerDialogState,
    DatePickerEntryMode, DatePickerMode, DateRangePickerDialog, DateRangePickerDialogState,
    DateTimeRange, DayPeriod, InputDatePickerFormField, InputDatePickerState, Orientation,
    SelectableDayForRangePredicate, SelectableDayPredicate, TimeOfDay, TimePickerDialog,
    TimePickerDialogState, TimePickerEntryMode, YearPicker, YearPickerState, add_days_to_date,
    add_months_to_month_date, days_in_month, first_day_offset, format_compact_date,
    format_full_date, format_medium_date, format_month_year, is_same_day, is_same_month,
    month_delta, parse_compact_date, show_date_picker, show_date_range_picker, show_time_picker,
};
pub use scrollbar::{Scrollbar, scrollbar};
pub use scrolling::{ExtentBook, ItemWindow, LazyList, Scroll, VariableExtentList, item_window};
pub use services::{
    BasicMessageChannel, EventChannel, EventSink, JsonMessageCodec, JsonMethodCodec, MethodCall,
    MethodChannel, MethodError, MethodResult, StandardMessageCodec, StandardMethodCodec, Value,
};
pub use shortcuts::{CallbackShortcuts, LogicalKeySet, ShortcutActivator, ShortcutRegistry};
pub use widgets::{
    Baseline, BoxedWidget, Center, Column, Constraints, Container, EdgeInsets, FittedBox,
    FractionallySizedBox, IndexedStack, LimitedBox, Offset, OverflowBox, Size, SizedOverflowBox,
    Text, TextSpan, Widget, boxed, repaint_boundary,
};

/// Everything a typical app needs in one import.
pub mod prelude {
    pub use crate::App;
    pub use crate::animation::{Animations, ColorTween, Controller, Curve, FloatTween, Tween};
    #[cfg(not(test))]
    pub use crate::app::run;
    pub use crate::app::{
        Application, BuildContext, FrameContext, FrameScheduler, RunOptions, WidgetApplication,
        WidgetHost, register_application,
    };
    pub use crate::components::{
        AppBar, Badge, Button, ButtonGroupState, ButtonVariant, Card, Divider, IdSource, Label,
        ListTile, ProgressBar, Scaffold, Slider, Switch, Theme, gap, stack_column, stack_row,
        theme_of,
    };
    pub use crate::controls::{
        Banner, BottomNavigation, BottomSheet, Checkbox, Chip, ChipStyle, DataTable, Destination,
        Dialog, GridList, NavigationRail, Radio, Scrim, Section, Snackbar, Spinner, TabBar,
        Tooltip, TooltipTrigger, TooltipTriggerMode,
    };
    pub use crate::cupertino::{
        CupertinoActivityIndicator, CupertinoAlertAction, CupertinoAlertDialog, CupertinoButton,
        CupertinoButtonSize, CupertinoColors, CupertinoContextMenu, CupertinoContextMenuAction,
        CupertinoContextMenuSheet, CupertinoDynamicColor, CupertinoNavigationBar,
        CupertinoPageScaffold, CupertinoPicker, CupertinoScrollbar, CupertinoSearchTextField,
        CupertinoSegmentedControl, CupertinoSlider, CupertinoSwitch, CupertinoTabBar,
        CupertinoTabItem, CupertinoTabScaffold, CupertinoTheme, cupertino_theme_of,
    };
    pub use crate::direction::{TextDirection, current_direction, directionality};
    pub use crate::drawer::{Drawer, DrawerAlignment};
    pub use crate::editable::{TextField, TextFieldState};
    pub use crate::engine::{Color, Paint, Rect, Style, TextAlign, TextStyle};
    pub use crate::focus::{Focus, KeyResult, focusable};
    pub use crate::framework::{
        AnyWidget, Component, GlobalKey, StateHandle, StatefulComponent, component, keyed_leaf,
        keyed_many, keyed_single, leaf, many, provide, single, stateful, with_global_key,
    };
    pub use crate::grid::{GridView, SliverGridDelegate};
    pub use crate::implicit::{Animated, animated};
    pub use crate::ink::{Ink, ink};
    pub use crate::interactive_viewer::{
        InteractiveViewer, PanAxis, TransformationController, interactive_viewer,
    };
    pub use crate::keyboard::{KeyChange, KeyEvent, Keyboard, LogicalKey, PhysicalKey};
    pub use crate::media_query::{
        MediaQuery, MediaQueryData, SafeArea, current_text_scale, media_query_of, safe_area,
    };
    pub use crate::menu::{
        CheckedPopupMenuItem, PopupMenu, PopupMenuButton, PopupMenuDivider, PopupMenuEntry,
        PopupMenuItem, PopupMenuPosition, popup_menu_offset,
    };
    pub use crate::navigation::{Navigator, Route, RouteArgs, Transition};
    pub use crate::painting::{
        BlendMode, ClipBehavior, ClipOp, FillType, Gradient, Image, RenderPath, StrokeCap,
        StrokeJoin, TileMode,
    };
    pub use crate::pickers::{
        CalendarDatePicker, Date, DatePickerDialog, DatePickerEntryMode, DatePickerMode,
        DateRangePickerDialog, DateTimeRange, DayPeriod, InputDatePickerFormField, Orientation,
        TimeOfDay, TimePickerDialog, TimePickerEntryMode, YearPicker, show_date_picker,
        show_date_range_picker, show_time_picker,
    };
    pub use crate::platform::{Brightness, Locale, UserSettings};
    pub use crate::scrollbar::{Scrollbar, scrollbar};
    pub use crate::scrolling::{ExtentBook, LazyList, Scroll, SliverListView, VariableExtentList};
    pub use crate::services::system::{
        AppExitResponse, AppExitType, AppLifecycleState, Clipboard, HapticFeedback, SystemChrome,
        SystemMouseCursor, SystemNavigator, SystemSound, SystemSoundType,
    };
    pub use crate::services::{MethodCall, MethodChannel, MethodError, Value};
    pub use crate::widgets::{
        Baseline, BoxedWidget, Center, Column, Constraints, Container, EdgeInsets, FittedBox,
        FractionallySizedBox, IndexedStack, LimitedBox, Offset, OverflowBox, Size,
        SizedOverflowBox, Text, TextSpan, Widget, boxed,
    };
}

/// Drives one frame: lays the widget tree out against the window size, paints
/// it into a `DisplayList`, wraps that in a `LayerTree` and hands it to the
/// engine.
///
/// Upstream this is what `RenderView` + `SchedulerBinding.drawFrame` do before
/// calling `PlatformDispatcher.render()`; the handoff object is the same
/// `flow::LayerTree`.
pub struct App {
    width: i32,
    height: i32,
    background: Color,
}

impl App {
    pub fn new(width: i32, height: i32) -> App {
        // The text stack needs ICU data before the first Paragraph is built.
        // Idempotent, so doing it here keeps app entry points boilerplate-free.
        engine::initialize();
        App {
            width,
            height,
            background: Color::WHITE,
        }
    }

    pub fn with_background(mut self, color: Color) -> App {
        self.background = color;
        self
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Runs layout + paint and returns the layer tree the engine rasterizes.
    pub fn build_frame(&self, mut root: impl Widget) -> LayerTree {
        let width = self.width as f32;
        let height = self.height as f32;

        root.layout(Constraints::tight(width, height));

        crate::app::compose_frame(
            self.width,
            self.height,
            1.0,
            render::Size::new(width, height),
            self.background,
            |context| root.paint(context, Offset::ZERO),
        )
    }

    /// Renders one frame to a PNG. Headless -- no window or GPU context.
    pub fn render_to_png(
        &self,
        root: impl Widget,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), RenderError> {
        self.build_frame(root).write_png(path.as_ref())
    }

    /// Renders one frame into a BGRA8888 buffer, for blitting into a window.
    pub fn render_to_bgra(&self, root: impl Widget) -> Result<Vec<u8>, RenderError> {
        self.build_frame(root).rasterize_bgra()
    }

    /// Renders one frame and shows it in a window, blocking until it closes.
    pub fn show(&self, root: impl Widget, title: &str) -> Result<(), RenderError> {
        self.build_frame(root).show(title)
    }
}
