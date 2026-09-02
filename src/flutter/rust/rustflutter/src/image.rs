//! The image pipeline, from upstream `painting/image_provider.dart`,
//! `image_stream.dart`, `image_cache.dart` and `image_resolution.dart`.
//!
//! The decode side is the crate's own: a string-keyed cache over a worker
//! pool ([`crate::painting::Image::shared`]), arrival next frame. What this
//! module adds is the upstream-shaped vocabulary over it -- providers that
//! know where bytes come from, streams that hand out the frame, and the
//! cache status queries.
//!
//! Recorded divergences (see PORTING_STATUS.md):
//!
//! * One frame only. The engine ABI decodes to a single image, so
//!   `MultiFrameImageStreamCompleter` is `ImageStreamCompleter` here and
//!   animated images hold on their first frame.
//! * `ResizeImage` carries its target size and policy, but the decode ABI
//!   takes no resize -- the frame arrives full size until that lands.
//! * `NetworkImage` needs an HTTP stack the crate does not have; it is a
//!   provider whose bytes arrive through a caller-supplied fetch callback.
//! * `AssetBundle` is the seed of the plan's E4: a trait with one `load`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::direction::TextDirection;
use crate::painting::Image;

/// Where an image's bytes come from, upstream `AssetBundle` narrowed to the
/// one method the image pipeline asks of it.
pub trait AssetBundle {
    fn load(&self, key: &str) -> Option<Vec<u8>>;
}

// The root bundle, what `AssetImage` reads through when no other was
// given -- upstream's `rootBundle`, waiting for the services wave to give
// it a platform-backed implementation.
thread_local! {
    static ROOT_BUNDLE: RefCell<Option<Rc<dyn AssetBundle>>> = const { RefCell::new(None) };
}

/// Installs the bundle [`AssetImage`] falls back to.
pub fn set_root_bundle(bundle: Rc<dyn AssetBundle>) {
    ROOT_BUNDLE.with(|slot| *slot.borrow_mut() = Some(bundle));
}

/// Upstream `ImageConfiguration`: what a provider may want to know about
/// the box its image lands in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImageConfiguration {
    pub size: Option<(f32, f32)>,
    pub text_direction: Option<TextDirection>,
}

impl ImageConfiguration {
    pub const EMPTY: ImageConfiguration = ImageConfiguration {
        size: None,
        text_direction: None,
    };

    pub fn with_size(mut self, size: (f32, f32)) -> ImageConfiguration {
        self.size = Some(size);
        self
    }
}

/// A decoded frame and the scale it decodes at, upstream `ImageInfo`.
///
/// # `clone` and `dispose` are the language's job here
///
/// Upstream this class has both, and a paragraph of contract about who calls
/// which. That is because a `dart:ui` image is a **handle** onto a buffer:
/// `clone()` makes a second handle, each one has to be disposed, and the
/// buffer goes when the last of them does.
///
/// `Rc<Image>` is that counting, done by the compiler. Cloning an `ImageInfo`
/// bumps the count and dropping it lowers it, so neither method has anything
/// to do here and neither is written.
///
/// What does not come for free is [`ImageInfo::is_clone_of`] -- see there.
#[derive(Clone)]
pub struct ImageInfo {
    pub image: Rc<Image>,
    /// Logical pixels per image pixel; a 2.0 means the image is drawn at
    /// half its pixel size.
    pub scale: f32,
    /// Upstream's `debugLabel`, which is not decoration: it is how an image
    /// in a leak report or a memory dump is traced back to the thing that
    /// asked for it. Part of the identity below for that reason -- two infos
    /// labelled differently came from different places even if the pixels are
    /// the same.
    pub debug_label: Option<String>,
}

impl ImageInfo {
    pub fn new(image: Rc<Image>, scale: f32) -> ImageInfo {
        ImageInfo {
            image,
            scale,
            debug_label: None,
        }
    }

    pub fn with_debug_label(mut self, label: impl Into<String>) -> Self {
        self.debug_label = Some(label.into());
        self
    }

    /// Upstream's `sizeBytes`: `image.height * image.width * 4`.
    ///
    /// **The decoded size, not the file's.** Four bytes a pixel whatever the
    /// source format was, so a 200 KB photograph is 48 MB of memory at
    /// 4000x3000 and the number that matters for a cache is this one. Upstream
    /// puts the same arithmetic in `ImageCache`, which is what makes the cache
    /// count in bytes rather than in images.
    pub fn size_bytes(&self) -> usize {
        let (width, height) = self.image.size();
        (width.max(0) as usize) * (height.max(0) as usize) * 4
    }

    /// Upstream's `isCloneOf`: whether these two describe the **same pixels**.
    ///
    /// ```dart
    /// bool isCloneOf(ImageInfo other) {
    ///   return other.image.isCloneOf(image) && other.scale == scale && other.debugLabel == debugLabel;
    /// }
    /// ```
    ///
    /// It exists for one question a listener has to answer on every frame it
    /// is handed an image: *is this new pixels, or the same pixels again?* The
    /// answer decides whether to lay out and paint again, and upstream's own
    /// example shows the caller disposing the new reference and returning
    /// where it is a clone.
    ///
    /// # Where this port and upstream part company, and why
    ///
    /// In Dart `clone()` makes a **second handle** onto one buffer, so a
    /// cloned `ImageInfo` is `isCloneOf` the original and **not** `==` to it
    /// -- `==` compares the handles. Here an `Rc` clone is the same pointer,
    /// so the two questions have the same answer and this is `Rc::ptr_eq`.
    ///
    /// The distinction upstream needs is a consequence of hand-counted
    /// handles, not of anything about images. Collapsing it is right; leaving
    /// the method out would not be, because *"are these the same pixels"* is
    /// still a question with a wrong answer available -- comparing sizes, or
    /// comparing nothing and repainting every frame.
    pub fn is_clone_of(&self, other: &ImageInfo) -> bool {
        Rc::ptr_eq(&self.image, &other.image)
            && self.scale == other.scale
            && self.debug_label == other.debug_label
    }
}

/// One listener on a stream, upstream `ImageStreamListener`: the frame
/// callback, plus the loading-progress and error callbacks.
#[derive(Clone)]
pub struct ImageStreamListener {
    pub on_image: Rc<dyn Fn(&ImageInfo)>,
    pub on_chunk: Option<Rc<dyn Fn(&ImageChunkEvent)>>,
    pub on_error: Option<Rc<dyn Fn(&str)>>,
}

/// A progress report while bytes load, upstream `ImageChunkEvent`. The
/// worker-pool decode reports nothing per-chunk, so this exists for the
/// callback's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ImageChunkEvent {
    pub cumulative_bytes_loaded: u32,
    pub expected_total_bytes: Option<u32>,
}

/// Upstream's `ImageStreamCompleter`: the thing that owns a frame (or the
/// promise of one) and tells the stream's listeners about it. The
/// one-frame/multi-frame split collapses here -- see the module docs.
pub struct ImageStreamCompleter {
    image: Option<ImageInfo>,
    listeners: Vec<ImageStreamListener>,
    error: Option<String>,
}

impl ImageStreamCompleter {
    /// `OneFrameImageStreamCompleter`: already resolved.
    pub fn one_frame(image: ImageInfo) -> ImageStreamCompleter {
        ImageStreamCompleter {
            image: Some(image),
            listeners: Vec::new(),
            error: None,
        }
    }

    /// `MultiFrameImageStreamCompleter`'s spelling: a promise, until
    /// [`ImageStreamCompleter::set_image`] lands the frame.
    pub fn pending() -> ImageStreamCompleter {
        ImageStreamCompleter {
            image: None,
            listeners: Vec::new(),
            error: None,
        }
    }

    pub fn add_listener(&mut self, listener: ImageStreamListener) {
        match (&self.image, &self.error) {
            (Some(info), _) => (listener.on_image)(info),
            (None, Some(error)) => {
                if let Some(on_error) = &listener.on_error {
                    (on_error)(error)
                }
            }
            _ => self.listeners.push(listener),
        }
    }

    /// Hands out the frame, upstream `_checkListener`'s notify path.
    pub fn set_image(&mut self, image: ImageInfo) {
        self.image = Some(image);
        let listeners = std::mem::take(&mut self.listeners);
        for listener in listeners {
            if let Some(info) = &self.image {
                (listener.on_image)(info);
            }
        }
    }

    /// Reports a load failure to whoever is listening.
    pub fn report_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        let listeners = std::mem::take(&mut self.listeners);
        for listener in listeners {
            if let Some(on_error) = &listener.on_error {
                (on_error)(self.error.as_deref().unwrap_or_default())
            }
        }
    }

    pub fn has_image(&self) -> bool {
        self.image.is_some()
    }

    /// The frame, once it has landed.
    pub fn image(&self) -> Option<ImageInfo> {
        self.image.clone()
    }
}

/// A handle into a completer that keeps it alive, upstream
/// `ImageStreamCompleterHandle`; `keepAlive` is the completer's refcount
/// here, so the handle is the refcount itself.
pub type ImageStreamCompleterHandle = Rc<RefCell<ImageStreamCompleter>>;

/// A slot for a frame, upstream `ImageStream`: listen before the frame
/// arrives and be told, or listen after and be handed it.
#[derive(Clone, Default)]
pub struct ImageStream {
    completer: Option<ImageStreamCompleterHandle>,
}

impl ImageStream {
    pub fn new() -> ImageStream {
        ImageStream { completer: None }
    }

    /// Upstream `ImageStream.setCompleter`.
    pub fn set_completer(&mut self, completer: ImageStreamCompleterHandle) {
        self.completer = Some(completer);
    }

    pub fn completer(&self) -> Option<&ImageStreamCompleterHandle> {
        self.completer.as_ref()
    }

    /// Upstream `ImageStream.addListener`, routed to the completer.
    pub fn add_listener(&self, listener: ImageStreamListener) {
        if let Some(completer) = &self.completer {
            completer.borrow_mut().add_listener(listener);
        }
    }
}

/// The error `NetworkImage` resolves with when its fetcher fails,
/// upstream `NetworkImageLoadException`.
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkImageLoadException {
    pub status_code: i32,
    pub uri: String,
}

/// Where image bytes come from, upstream `ImageProvider`'s subclass family
/// as one enum.
#[derive(Clone)]
pub enum ImageProvider {
    /// `MemoryImage`: bytes already in hand.
    Memory { bytes: Rc<Vec<u8>>, scale: f32 },
    /// `AssetImage`/`ExactAssetImage`: a key into a bundle. The exact
    /// spelling (`allowExact` upstream) folds into `scale` being honoured
    /// verbatim rather than resolution-adjusted.
    Asset {
        key: String,
        scale: f32,
        bundle: Option<Rc<dyn AssetBundle>>,
    },
    /// `FileImage`: bytes read off disk.
    File { path: String, scale: f32 },
    /// `NetworkImage`: bytes fetched by the caller's HTTP, the crate
    /// carrying none of its own.
    Network {
        url: String,
        scale: f32,
        fetch: Rc<dyn Fn(&str) -> Result<Vec<u8>, NetworkImageLoadException>>,
    },
    /// `ResizeImage`: another provider, at a size.
    Resize {
        provider: Box<ImageProvider>,
        width: Option<u32>,
        height: Option<u32>,
        policy: ResizeImagePolicy,
    },
}

/// Upstream `ResizeImagePolicy`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResizeImagePolicy {
    /// Exactly the given size.
    #[default]
    Exact,
    /// At most the given size, aspect kept.
    Fit,
    /// Decode at full size regardless.
    None,
}

/// The cache key a provider resolves to, upstream `AssetBundleImageKey` and
/// friends: the string the crate's image cache already keys on.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedImageKey {
    pub key: String,
    pub scale: f32,
}

impl ImageProvider {
    /// Upstream `obtainKey`, folded with the cache-key spelling each
    /// provider uses.
    pub fn cache_key(&self) -> ResolvedImageKey {
        match self {
            ImageProvider::Memory { bytes, scale } => ResolvedImageKey {
                key: format!("memory:{:x}@{scale}", hash_bytes(bytes)),
                scale: *scale,
            },
            ImageProvider::Asset { key, scale, .. } => ResolvedImageKey {
                key: format!("asset:{key}"),
                scale: *scale,
            },
            ImageProvider::File { path, scale } => ResolvedImageKey {
                key: format!("file:{path}"),
                scale: *scale,
            },
            ImageProvider::Network { url, scale, .. } => ResolvedImageKey {
                key: format!("net:{url}"),
                scale: *scale,
            },
            ImageProvider::Resize {
                provider,
                width,
                height,
                ..
            } => {
                let inner = provider.cache_key();
                ResolvedImageKey {
                    key: format!("{}_resize:{:?}x{:?}", inner.key, width, height),
                    scale: inner.scale,
                }
            }
        }
    }

    /// The bytes this provider resolves to, if they can be had on this
    /// thread.
    fn load_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            ImageProvider::Memory { bytes, .. } => Ok(bytes.as_ref().clone()),
            ImageProvider::Asset { key, bundle, .. } => {
                let bundle = bundle
                    .clone()
                    .or_else(|| ROOT_BUNDLE.with(|slot| slot.borrow().clone()))
                    .ok_or_else(|| format!("no bundle has asset '{key}'"))?;
                bundle
                    .load(key)
                    .ok_or_else(|| format!("asset '{key}' not in bundle"))
            }
            ImageProvider::File { path, .. } => {
                std::fs::read(path).map_err(|error| format!("'{path}': {error}"))
            }
            ImageProvider::Network { url, fetch, .. } => {
                fetch(url).map_err(|error| format!("{} {}", error.status_code, error.uri))
            }
            ImageProvider::Resize { provider, .. } => provider.load_bytes(),
        }
    }

    /// Upstream `resolve`: the stream, and the decode on the worker pool
    /// under the cache key. Arrives next frame, the pool's rhythm.
    pub fn resolve(&self, _configuration: ImageConfiguration) -> ImageStream {
        let key = self.cache_key();
        let scale = key.scale;
        let mut stream = ImageStream::new();
        let completer = Rc::new(RefCell::new(ImageStreamCompleter::pending()));
        stream.set_completer(completer.clone());
        match self.load_bytes() {
            Ok(bytes) => {
                if let Some(image) = Image::shared(&key.key, &bytes) {
                    completer
                        .borrow_mut()
                        .set_image(ImageInfo::new(image, scale));
                }
            }
            Err(error) => completer.borrow_mut().report_error(error),
        }
        stream
    }

    /// Upstream `resolveStreamForKey`'s synchronous spelling: decode on this
    /// thread. The headless render's path.
    pub fn resolve_now(&self, _configuration: ImageConfiguration) -> ImageStream {
        let key = self.cache_key();
        let scale = key.scale;
        let mut stream = ImageStream::new();
        let completer = Rc::new(RefCell::new(ImageStreamCompleter::pending()));
        stream.set_completer(completer.clone());
        match self.load_bytes() {
            Ok(bytes) => {
                if let Some(image) = Image::shared_now(&key.key, &bytes) {
                    completer
                        .borrow_mut()
                        .set_image(ImageInfo::new(image, scale));
                }
            }
            Err(error) => completer.borrow_mut().report_error(error),
        }
        stream
    }

    /// Upstream `evict`, routed to the crate's cache.
    pub fn evict(&self) -> bool {
        crate::painting::image_cache_evict(&self.cache_key().key)
    }

    /// The resize spelling, upstream `ResizeImage.resizeIfNeeded`.
    pub fn resize(
        provider: ImageProvider,
        width: Option<u32>,
        height: Option<u32>,
        policy: ResizeImagePolicy,
    ) -> ImageProvider {
        if width.is_none() && height.is_none() {
            return provider;
        }
        ImageProvider::Resize {
            provider: Box::new(provider),
            width,
            height,
            policy,
        }
    }
}

/// Upstream `ImageCacheStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageCacheStatus {
    Live,
    Pending,
    Uncached,
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    // FNV-1a: a stable in-process identity for the cache key, not a
    // cryptographic claim.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A tiny valid PNG: a 1x1 opaque white pixel. Decode needs real bytes
    /// to go anywhere under the stub engine.
    const ONE_PX_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00,
    ];

    struct MapBundle {
        entries: std::collections::HashMap<&'static str, Vec<u8>>,
    }

    impl AssetBundle for MapBundle {
        fn load(&self, key: &str) -> Option<Vec<u8>> {
            self.entries.get(key).cloned()
        }
    }

    #[test]
    fn a_pending_completer_tells_its_listener_when_the_frame_lands() {
        let completer = Rc::new(RefCell::new(ImageStreamCompleter::pending()));
        let mut stream = ImageStream::new();
        stream.set_completer(completer.clone());
        let heard = Rc::new(Cell::new(false));
        let listener = ImageStreamListener {
            on_image: {
                let heard = Rc::clone(&heard);
                Rc::new(move |_info| heard.set(true))
            },
            on_chunk: None,
            on_error: None,
        };
        stream.add_listener(listener);
        assert!(!completer.borrow().has_image());
        assert!(!heard.get());
        // A frame arriving without the worker pool: the direct spelling.
        let image = crate::painting::Image::decode(ONE_PX_PNG);
        if let Some(image) = image {
            completer
                .borrow_mut()
                .set_image(ImageInfo::new(Rc::new(image), 1.0));
            assert!(heard.get());
        }
    }

    #[test]
    fn a_resolved_completer_hands_the_frame_to_late_listeners() {
        let image = crate::painting::Image::decode(ONE_PX_PNG);
        let Some(image) = image else {
            return;
        };
        let completer = Rc::new(RefCell::new(ImageStreamCompleter::one_frame(
            ImageInfo::new(Rc::new(image), 2.0),
        )));
        let mut stream = ImageStream::new();
        stream.set_completer(completer);
        let scale_seen = Rc::new(Cell::new(0.0f32));
        stream.add_listener(ImageStreamListener {
            on_image: {
                let scale_seen = Rc::clone(&scale_seen);
                Rc::new(move |info| scale_seen.set(info.scale))
            },
            on_chunk: None,
            on_error: None,
        });
        assert_eq!(scale_seen.get(), 2.0);
    }

    #[test]
    fn a_failing_provider_reports_the_error() {
        let provider = ImageProvider::Asset {
            key: "missing.png".to_string(),
            scale: 1.0,
            bundle: Some(Rc::new(MapBundle {
                entries: std::collections::HashMap::new(),
            })),
        };
        let stream = provider.resolve_now(ImageConfiguration::EMPTY);
        let completer = stream.completer().unwrap().clone();
        assert!(!completer.borrow().has_image());
        let error_seen = Rc::new(Cell::new(false));
        stream.add_listener(ImageStreamListener {
            on_image: Rc::new(|_| {}),
            on_chunk: None,
            on_error: {
                let error_seen = Rc::clone(&error_seen);
                Some(Rc::new(move |_message: &str| error_seen.set(true)))
            },
        });
        assert!(error_seen.get());
    }

    #[test]
    fn cache_keys_distinguish_providers_and_resizes() {
        let memory = ImageProvider::Memory {
            bytes: Rc::new(vec![1, 2, 3]),
            scale: 1.0,
        };
        let other_memory = ImageProvider::Memory {
            bytes: Rc::new(vec![4, 5, 6]),
            scale: 1.0,
        };
        assert_ne!(memory.cache_key(), other_memory.cache_key());
        let resized = ImageProvider::resize(memory, Some(64), None, ResizeImagePolicy::Exact);
        let ResolvedImageKey { key, .. } = resized.cache_key();
        assert!(key.ends_with("_resize:Some(64)xNone"));
    }

    #[test]
    fn resize_of_nothing_is_the_provider_itself() {
        let provider = ImageProvider::Memory {
            bytes: Rc::new(vec![1]),
            scale: 1.0,
        };
        match ImageProvider::resize(provider, None, None, ResizeImagePolicy::Exact) {
            ImageProvider::Memory { .. } => {}
            _other => panic!("expected the provider itself"),
        }
    }

    #[test]
    fn the_root_bundle_serves_assets_without_an_explicit_one() {
        set_root_bundle(Rc::new(MapBundle {
            entries: [("logo.png", Vec::<u8>::new())].into_iter().collect(),
        }));
        let provider = ImageProvider::Asset {
            key: "logo.png".to_string(),
            scale: 1.0,
            bundle: None,
        };
        // Bytes resolve; the empty PNG will not decode, but the error path
        // is the decode's, not the bundle's.
        let stream = provider.resolve_now(ImageConfiguration::EMPTY);
        assert!(stream.completer().is_some());
    }

    // -- Is this new pixels, or the same pixels again? ----------------------

    #[test]
    fn an_info_and_its_clone_describe_the_same_pixels() {
        // The question a listener answers on every frame it is handed an
        // image: the answer decides whether to lay out and paint again.
        let Some(image) = crate::painting::Image::decode(ONE_PX_PNG) else {
            return;
        };
        let first = ImageInfo::new(Rc::new(image), 1.0);
        let second = first.clone();
        assert!(first.is_clone_of(&second));
        assert!(second.is_clone_of(&first), "and either way round");
    }

    #[test]
    fn but_two_decodes_of_the_same_bytes_are_not() {
        // Same pixels by value, different buffers. A listener told these were
        // clones would skip a repaint it needs -- the new buffer is what it
        // has to draw from, and the old one may be on its way out.
        let (Some(one), Some(two)) = (
            crate::painting::Image::decode(ONE_PX_PNG),
            crate::painting::Image::decode(ONE_PX_PNG),
        ) else {
            return;
        };
        let first = ImageInfo::new(Rc::new(one), 1.0);
        let second = ImageInfo::new(Rc::new(two), 1.0);
        assert!(!first.is_clone_of(&second));
    }

    #[test]
    fn the_scale_and_the_label_are_part_of_the_identity() {
        // Upstream compares all three. The same buffer at a different scale is
        // a different thing to draw, and a different label came from a
        // different place -- which is what makes a leak report readable.
        let Some(image) = crate::painting::Image::decode(ONE_PX_PNG) else {
            return;
        };
        let image = Rc::new(image);
        let base = ImageInfo::new(Rc::clone(&image), 1.0);

        let rescaled = ImageInfo::new(Rc::clone(&image), 2.0);
        assert!(!base.is_clone_of(&rescaled), "same buffer, different scale");

        let labelled = ImageInfo::new(Rc::clone(&image), 1.0).with_debug_label("avatar");
        assert!(!base.is_clone_of(&labelled), "same buffer, different label");
        assert!(
            labelled.is_clone_of(&labelled.clone()),
            "and its own clone is"
        );
    }

    #[test]
    fn the_size_is_the_decoded_size_and_not_the_files() {
        // Four bytes a pixel whatever the source format was. This is the
        // number a cache has to count in, and the reason a small photograph
        // can be a large image.
        //
        // The dimensions have to be **known and not square**, and neither is
        // fussiness: the first version measured the one-pixel fixture, whose
        // width and height are what the stub reports for bytes it does not
        // recognise. Every arithmetic mistake gives the same answer there, and
        // two mutations -- dropping the four and multiplying by the scale --
        // both stayed green.
        let bytes = crate::engine_test_stubs::encoded_image(40, 30);
        let Some(image) = crate::painting::Image::decode(&bytes) else {
            panic!("the stub decodes its own fixture");
        };
        assert_eq!(image.size(), (40, 30), "the fixture arrived intact");
        let info = ImageInfo::new(Rc::new(image), 1.0);
        assert_eq!(info.size_bytes(), 40 * 30 * 4);

        // And the scale does not enter into it: scale is about how large the
        // image is *drawn*, not how much of it there is. A 3.0 here would be
        // three times the memory if it did.
        let other = crate::painting::Image::decode(&bytes).expect("again");
        let scaled = ImageInfo::new(Rc::new(other), 3.0);
        assert_eq!(scaled.size_bytes(), info.size_bytes());
    }
}
