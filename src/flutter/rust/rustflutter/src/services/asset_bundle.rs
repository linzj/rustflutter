// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Where an application's own files come from (upstream
//! `services/asset_bundle.dart`, `services/font_loader.dart`,
//! `services/flutter_version.dart`).
//!
//! An asset is a file the build packed into the application: an image, a
//! font, a translation table. The bundle is what turns a key -- the path the
//! author wrote in their manifest -- into the bytes.
//!
//! The [`AssetBundle`] trait itself lives in
//! [`image`](crate::image::AssetBundle), where the image pipeline needed it
//! first; the two bundles upstream defines are here.
//!
//! # Recorded divergences
//!
//! * Every load upstream is a `Future`. The trait here is synchronous,
//!   because [`AssetImage`](crate::image::AssetImage) reads it while
//!   building and cannot wait. [`PlatformAssetBundle`] therefore separates
//!   the two halves upstream's futures fuse together: `prefetch` asks the
//!   platform and fills the cache, and `load` answers out of it. A key that
//!   has not been prefetched is a miss rather than a wait.
//! * `NetworkAssetBundle` is not ported. It is upstream's `dart:io`
//!   `HttpClient` behind the same interface; there is no HTTP client in this
//!   crate, and writing one is not porting a framework. Ledgered.
//! * Upstream's `loadStructuredData` caches the *parsed* value, and goes to
//!   some length to stay synchronous when the load happened to be. Caching
//!   the bytes is what is here; a caller that wants the parsed value keeps
//!   it, and the synchronous/asynchronous dance has nothing to reconcile
//!   because there is only one of them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::engine::register_font;
use crate::image::AssetBundle;

/// Upstream `CachingAssetBundle`: a bundle that remembers what it has
/// already loaded.
///
/// Upstream is abstract -- it caches and leaves `load` to a subclass. Here it
/// wraps another bundle, which is the same arrangement with the inheritance
/// turned into a field.
pub struct CachingAssetBundle {
    inner: Box<dyn AssetBundle>,
    cache: RefCell<HashMap<String, Option<Vec<u8>>>>,
}

impl CachingAssetBundle {
    pub fn new(inner: Box<dyn AssetBundle>) -> CachingAssetBundle {
        CachingAssetBundle {
            inner,
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Upstream `evict`: forget one key, so the next load asks again.
    ///
    /// Upstream's reason is hot reload -- an asset that changed on disk is
    /// still in the cache, and nothing else would ever ask for it again.
    pub fn evict(&self, key: &str) {
        self.cache.borrow_mut().remove(key);
    }

    /// Upstream `clear`: forget all of it.
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }

    /// Whether this key has been looked up before, whether or not it was
    /// found. A miss is cached too: upstream caches the future, and a future
    /// that completed with an error is still a completed future.
    pub fn is_cached(&self, key: &str) -> bool {
        self.cache.borrow().contains_key(key)
    }
}

impl AssetBundle for CachingAssetBundle {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(cached) = self.cache.borrow().get(key) {
            return cached.clone();
        }
        let loaded = self.inner.load(key);
        self.cache
            .borrow_mut()
            .insert(key.to_string(), loaded.clone());
        loaded
    }
}

/// Upstream `PlatformAssetBundle`: the assets the engine packed with the
/// application, fetched over the `flutter/assets` channel.
pub struct PlatformAssetBundle {
    /// Shared with the reply closures, which outlive the call that made
    /// them and have to be able to fill it in when the platform answers.
    cache: Rc<RefCell<HashMap<String, Option<Vec<u8>>>>>,
}

impl Default for PlatformAssetBundle {
    fn default() -> PlatformAssetBundle {
        PlatformAssetBundle::new()
    }
}

impl PlatformAssetBundle {
    /// The channel the engine answers asset requests on.
    pub const CHANNEL: &'static str = "flutter/assets";

    pub fn new() -> PlatformAssetBundle {
        PlatformAssetBundle {
            cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Asks the platform for `key` and keeps the answer, so that a later
    /// [`load`](AssetBundle::load) can be synchronous.
    ///
    /// `done` is told whether the asset was there. Upstream throws for a
    /// missing asset; there is nothing to throw to from a callback, and the
    /// caller is the one who knows whether a missing asset is fatal.
    pub fn prefetch(&self, key: &str, done: impl FnOnce(bool) + 'static) {
        // Upstream percent-encodes the key before sending it: an asset path
        // is a URI to the engine, and one with a space in it does not survive
        // otherwise.
        let encoded = encode_asset_path(key);
        let cache = Rc::clone(&self.cache);
        let key = key.to_string();
        crate::services::send_with_reply(
            PlatformAssetBundle::CHANNEL,
            encoded.as_bytes(),
            Box::new(move |reply| {
                let bytes = reply.map(|bytes| bytes.to_vec());
                let found = bytes.is_some();
                cache.borrow_mut().insert(key, bytes);
                done(found);
            }),
        );
    }
}

impl AssetBundle for PlatformAssetBundle {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.cache.borrow().get(key).cloned().flatten()
    }
}

/// Upstream's `Uri.encodeFull` over an asset key.
///
/// Everything unreserved passes through; anything else becomes a percent
/// escape. A path with a space is the case that actually happens, and the
/// one that fails silently -- the engine looks for a file whose name has a
/// literal space in the request and finds nothing.
pub fn encode_asset_path(key: &str) -> String {
    let mut encoded = String::with_capacity(key.len());
    for byte in key.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')'
            | b';'
            | b'/'
            | b'?'
            | b':'
            | b'@'
            | b'&'
            | b'='
            | b'+'
            | b'$'
            | b','
            | b'#' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Upstream `FontLoader`: registers a font family the application shipped or
/// downloaded.
///
/// Upstream collects futures and awaits them in `load`; here the bytes are
/// already bytes, so `add_font` takes them and `load` hands them to the
/// engine. What survives is the part that matters: a loader can be loaded
/// once, and adding to one that has been is an error rather than a font that
/// silently never appears.
pub struct FontLoader {
    pub family: String,
    fonts: Vec<Vec<u8>>,
    loaded: bool,
}

impl FontLoader {
    pub fn new(family: impl Into<String>) -> FontLoader {
        FontLoader {
            family: family.into(),
            fonts: Vec::new(),
            loaded: false,
        }
    }

    /// Upstream `addFont`, which throws a `StateError` once loaded.
    pub fn add_font(&mut self, bytes: Vec<u8>) -> Result<(), FontLoaderError> {
        if self.loaded {
            return Err(FontLoaderError::AlreadyLoaded);
        }
        self.fonts.push(bytes);
        Ok(())
    }

    /// Upstream `load`. Every face added goes to the engine under the same
    /// family, which is how a family gets its bold and its italic.
    pub fn load(&mut self) -> Result<(), FontLoaderError> {
        if self.loaded {
            return Err(FontLoaderError::AlreadyLoaded);
        }
        self.loaded = true;
        for bytes in &self.fonts {
            if !register_font(bytes, &self.family) {
                return Err(FontLoaderError::EngineRefused);
            }
        }
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }
}

/// What can go wrong loading a font. Upstream's is a `StateError` and an
/// exception out of the engine; both are outcomes a caller can do something
/// about, so both are returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontLoaderError {
    /// Upstream's `StateError('FontLoader is already loaded')`.
    AlreadyLoaded,
    /// The engine would not take the bytes -- not a font, or a format it does
    /// not read.
    EngineRefused,
}

/// Upstream `FlutterVersion`: what the tool stamped into the build.
///
/// Every one is absent unless the build defined it, which is upstream's
/// `bool.hasEnvironment` test. `option_env!` is the same question asked at
/// compile time.
pub struct FlutterVersion;

impl FlutterVersion {
    pub const VERSION: Option<&'static str> = option_env!("FLUTTER_VERSION");
    pub const CHANNEL: Option<&'static str> = option_env!("FLUTTER_CHANNEL");
    pub const GIT_URL: Option<&'static str> = option_env!("FLUTTER_GIT_URL");
    pub const FRAMEWORK_REVISION: Option<&'static str> = option_env!("FLUTTER_FRAMEWORK_REVISION");
    pub const ENGINE_REVISION: Option<&'static str> = option_env!("FLUTTER_ENGINE_REVISION");
    /// Upstream carries this because `Platform.version` does not exist on the
    /// web. There is no Dart here at all, so it is absent for a second
    /// reason as well as the first.
    pub const DART_VERSION: Option<&'static str> = option_env!("FLUTTER_DART_VERSION");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle that counts how many times it was actually asked.
    struct Counting {
        answers: HashMap<String, Vec<u8>>,
        asked: RefCell<usize>,
    }

    impl Counting {
        fn new(answers: &[(&str, &[u8])]) -> Counting {
            Counting {
                answers: answers
                    .iter()
                    .map(|(key, bytes)| (key.to_string(), bytes.to_vec()))
                    .collect(),
                asked: RefCell::new(0),
            }
        }
    }

    impl AssetBundle for Rc<Counting> {
        fn load(&self, key: &str) -> Option<Vec<u8>> {
            *self.asked.borrow_mut() += 1;
            self.answers.get(key).cloned()
        }
    }

    #[test]
    fn a_cached_bundle_asks_the_one_underneath_it_once() {
        let inner = Rc::new(Counting::new(&[("a.png", &[1, 2, 3])]));
        let bundle = CachingAssetBundle::new(Box::new(Rc::clone(&inner)));
        assert_eq!(bundle.load("a.png"), Some(vec![1, 2, 3]));
        assert_eq!(bundle.load("a.png"), Some(vec![1, 2, 3]));
        assert_eq!(*inner.asked.borrow(), 1);
    }

    #[test]
    fn a_miss_is_cached_too() {
        // Upstream caches the future, and a future that completed with an
        // error is still completed -- so a key that was not there is not
        // asked for again either. A port that cached only the hits turns
        // every missing asset into a lookup on every build.
        let inner = Rc::new(Counting::new(&[]));
        let bundle = CachingAssetBundle::new(Box::new(Rc::clone(&inner)));
        assert_eq!(bundle.load("missing.png"), None);
        assert_eq!(bundle.load("missing.png"), None);
        assert_eq!(*inner.asked.borrow(), 1);
        assert!(bundle.is_cached("missing.png"));
    }

    #[test]
    fn evicting_a_key_makes_the_next_load_ask_again() {
        // Upstream's reason is hot reload: an asset that changed on disk is
        // still in the cache and nothing else would ever ask for it again.
        let inner = Rc::new(Counting::new(&[("a.png", &[1])]));
        let bundle = CachingAssetBundle::new(Box::new(Rc::clone(&inner)));
        bundle.load("a.png");
        bundle.evict("a.png");
        bundle.load("a.png");
        assert_eq!(*inner.asked.borrow(), 2);
        // And clearing forgets everything rather than one thing.
        bundle.clear();
        assert!(!bundle.is_cached("a.png"));
    }

    #[test]
    fn an_asset_path_is_percent_encoded_before_it_is_sent() {
        // A path with a space is the case that actually happens and the one
        // that fails silently: the engine looks for a file whose name has a
        // literal space in the request, and finds nothing.
        assert_eq!(
            encode_asset_path("assets/my image.png"),
            "assets/my%20image.png"
        );
        // The separators a path is made of stay themselves -- encoding the
        // slashes would ask for one file with slashes in its name.
        assert_eq!(encode_asset_path("packages/x/a.png"), "packages/x/a.png");
        // Non-ASCII goes out as its UTF-8 bytes, one escape each.
        assert_eq!(encode_asset_path("图.png"), "%E5%9B%BE.png");
    }

    #[test]
    fn a_platform_bundle_has_nothing_until_it_has_been_prefetched() {
        // The divergence, asserted: upstream's load awaits, and this one
        // cannot, so a key nobody prefetched is a miss rather than a wait.
        let bundle = PlatformAssetBundle::new();
        assert_eq!(bundle.load("a.png"), None);
        assert_eq!(PlatformAssetBundle::CHANNEL, "flutter/assets");
    }

    #[test]
    fn a_font_loader_loads_once_and_says_so_the_second_time() {
        // Upstream throws a StateError both for adding after loading and for
        // loading twice. Silently ignoring either gives a font that never
        // appears and nothing to explain why.
        let mut loader = FontLoader::new("Inter");
        assert!(loader.add_font(vec![0, 1, 2]).is_ok());
        assert!(!loader.is_loaded());
        // The engine stub these tests run against refuses every font,
        // so what this shows is that the refusal is reported rather
        // than swallowed -- and that the loader counts as loaded
        // either way, which is upstream's order: `_loaded = true`
        // before the first face is handed over.
        assert_eq!(loader.load(), Err(FontLoaderError::EngineRefused));
        assert!(loader.is_loaded());
        assert_eq!(loader.load(), Err(FontLoaderError::AlreadyLoaded));
        assert_eq!(
            loader.add_font(vec![3]),
            Err(FontLoaderError::AlreadyLoaded)
        );
    }

    #[test]
    fn several_faces_go_under_the_one_family() {
        // That is how a family gets its bold and its italic: one loader, one
        // name, several files.
        let mut loader = FontLoader::new("Inter");
        loader.add_font(vec![0]).expect("added");
        loader.add_font(vec![1]).expect("added");
        assert_eq!(loader.family, "Inter");
        // Both go to the engine under the one name; the stub refuses
        // the first, which is how far this can be checked without one.
        assert_eq!(loader.load(), Err(FontLoaderError::EngineRefused));
    }

    #[test]
    fn the_version_constants_are_absent_unless_the_build_defined_them() {
        // Upstream's `bool.hasEnvironment` test, which is `option_env!` here.
        // Nothing defines these in this build, and that is the answer -- not
        // an empty string, which would read as a version of "".
        assert_eq!(FlutterVersion::VERSION, option_env!("FLUTTER_VERSION"));
        assert_eq!(FlutterVersion::CHANNEL, option_env!("FLUTTER_CHANNEL"));
        assert!(FlutterVersion::VERSION.is_none_or(|version| !version.is_empty()));
    }
}
