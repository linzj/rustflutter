// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Which assets are in the bundle, and at which resolutions (upstream
//! `services/asset_manifest.dart`).
//!
//! An application that ships one image at three densities ships three files,
//! and the build writes down which is which. The manifest is that record:
//! for each asset the reader asked for, the variants that exist and what
//! device pixel ratio each is for. It is what lets `AssetImage` pick the
//! 2x file on a 2x screen without the caller naming it.
//!
//! # Recorded divergences
//!
//! * Upstream's `loadFromAssetBundle` is a `Future` and picks between
//!   `AssetManifest.bin` and, on the web, `AssetManifest.bin.json` --
//!   base64 inside JSON, because a browser cannot be relied on to fetch
//!   binary. There is no web target here, so
//!   [`load_from_asset_bundle`](AssetManifest::load_from_asset_bundle) reads
//!   the binary file directly and the web filename is recorded as a constant
//!   rather than a code path.
//! * Upstream's `_AssetManifestBin` decodes lazily, one key at a time, so
//!   that a large manifest does not cost anything at the first asset load.
//!   This decodes the whole map once, because the decode already happened
//!   when the bytes were turned into a [`Value`] -- the laziness upstream
//!   wants is in the *type-casting*, which Rust did on the way in.

use crate::image::AssetBundle;
use crate::services::codec::{MessageCodec, StandardMessageCodec, Value};

/// Upstream `AssetMetadata`: one file that could serve an asset key.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetMetadata {
    /// The path to load this variant from.
    pub key: String,
    /// The device pixel ratio this file is drawn for. Absent when the build
    /// did not say, which is the case for anything that is not an image.
    pub target_device_pixel_ratio: Option<f64>,
    /// Whether this is the asset the caller asked for rather than one of its
    /// variants.
    pub main: bool,
}

impl AssetMetadata {
    pub fn new(
        key: impl Into<String>,
        target_device_pixel_ratio: Option<f64>,
        main: bool,
    ) -> AssetMetadata {
        AssetMetadata {
            key: key.into(),
            target_device_pixel_ratio,
            main,
        }
    }
}

/// Upstream `AssetManifest`.
///
/// Upstream is an abstract class with one implementation, `_AssetManifestBin`;
/// here it is the one implementation, for the reason recorded on
/// [`TapRegionRegistry`](crate::tap_region::TapRegionRegistry) -- an
/// interface with a single implementation and no second one in prospect buys
/// nothing.
#[derive(Clone, Debug, Default)]
pub struct AssetManifest {
    entries: Vec<(String, Vec<AssetMetadata>)>,
}

impl AssetManifest {
    /// The file the build writes the manifest to.
    pub const FILENAME: &'static str = "AssetManifest.bin";
    /// What the web build writes instead: the same bytes, base64-encoded
    /// inside a JSON string. Recorded because upstream reads it and this
    /// does not; see the module's divergences.
    pub const WEB_FILENAME: &'static str = "AssetManifest.bin.json";

    /// Upstream `_AssetManifestBin.fromStandardMessageCodecMessage`.
    pub fn from_standard_message(bytes: &[u8]) -> Option<AssetManifest> {
        let decoded = StandardMessageCodec.decode(bytes).ok()?;
        AssetManifest::from_value(&decoded)
    }

    /// The manifest a decoded message describes.
    pub fn from_value(value: &Value) -> Option<AssetManifest> {
        let Value::Map(pairs) = value else {
            return None;
        };
        let mut entries = Vec::with_capacity(pairs.len());
        for (key, variants) in pairs {
            let Value::String(key) = key else {
                continue;
            };
            entries.push((key.clone(), AssetManifest::variants_from(key, variants)));
        }
        Some(AssetManifest { entries })
    }

    /// Upstream `AssetManifest.loadFromAssetBundle`, without the web branch.
    pub fn load_from_asset_bundle(bundle: &dyn AssetBundle) -> Option<AssetManifest> {
        AssetManifest::from_standard_message(&bundle.load(AssetManifest::FILENAME)?)
    }

    /// Upstream's per-key decoding. Upstream's schema is a list of maps with
    /// an `asset` and a `dpr`; a variant whose `asset` is the key itself is
    /// the main one.
    fn variants_from(key: &str, variants: &Value) -> Vec<AssetMetadata> {
        let Value::List(items) = variants else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|item| {
                let Value::Map(fields) = item else {
                    return None;
                };
                let get = |name: &str| {
                    fields
                        .iter()
                        .find(|(field, _)| matches!(field, Value::String(field) if field == name))
                        .map(|(_, value)| value)
                };
                let Some(Value::String(asset)) = get("asset") else {
                    return None;
                };
                let ratio = match get("dpr") {
                    Some(Value::F64(number)) => Some(*number),
                    Some(Value::I32(number)) => Some(*number as f64),
                    Some(Value::I64(number)) => Some(*number as f64),
                    _ => None,
                };
                Some(AssetMetadata::new(asset.clone(), ratio, key == asset))
            })
            .collect()
    }

    /// Upstream `listAssets`.
    pub fn list_assets(&self) -> Vec<String> {
        self.entries.iter().map(|(key, _)| key.clone()).collect()
    }

    /// Upstream `getAssetVariants`: nothing at all for a key the manifest
    /// does not mention, which is different from a key with no variants.
    pub fn asset_variants(&self, key: &str) -> Option<&[AssetMetadata]> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, variants)| variants.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(asset: &str, dpr: Option<f64>) -> Value {
        let mut fields = vec![(
            Value::String("asset".to_string()),
            Value::String(asset.to_string()),
        )];
        if let Some(dpr) = dpr {
            fields.push((Value::String("dpr".to_string()), Value::F64(dpr)));
        }
        Value::Map(fields)
    }

    /// One image at three densities, which is the case the manifest exists
    /// for.
    fn manifest() -> AssetManifest {
        AssetManifest::from_value(&Value::Map(vec![(
            Value::String("images/logo.png".to_string()),
            Value::List(vec![
                variant("images/logo.png", Some(1.0)),
                variant("images/2.0x/logo.png", Some(2.0)),
                variant("images/3.0x/logo.png", Some(3.0)),
            ]),
        )]))
        .expect("a manifest")
    }

    #[test]
    fn the_variant_named_by_the_key_is_the_main_one() {
        // That is the whole of upstream's `main` rule, and it is what tells a
        // 1x file apart from the 2x file beside it: the key is the path the
        // caller wrote, and exactly one variant matches it.
        let manifest = manifest();
        let variants = manifest
            .asset_variants("images/logo.png")
            .expect("variants");
        assert_eq!(variants.len(), 3);
        assert_eq!(
            variants
                .iter()
                .filter(|variant| variant.main)
                .map(|variant| variant.key.as_str())
                .collect::<Vec<_>>(),
            vec!["images/logo.png"]
        );
        assert_eq!(variants[1].target_device_pixel_ratio, Some(2.0));
    }

    #[test]
    fn a_key_the_manifest_does_not_mention_has_no_variants_at_all() {
        // Different from a key with an empty list: one means "not shipped",
        // the other means "shipped with nothing to choose between".
        let manifest = manifest();
        assert_eq!(manifest.asset_variants("images/missing.png"), None);

        let empty = AssetManifest::from_value(&Value::Map(vec![(
            Value::String("data/blob".to_string()),
            Value::List(vec![]),
        )]))
        .expect("a manifest");
        assert_eq!(empty.asset_variants("data/blob"), Some(&[][..]));
    }

    #[test]
    fn an_asset_with_no_density_still_gets_an_entry() {
        // Anything that is not an image has no `dpr`, and it is still an
        // asset the manifest lists.
        let manifest = AssetManifest::from_value(&Value::Map(vec![(
            Value::String("fonts/sans.ttf".to_string()),
            Value::List(vec![variant("fonts/sans.ttf", None)]),
        )]))
        .expect("a manifest");
        let variants = manifest.asset_variants("fonts/sans.ttf").expect("variants");
        assert_eq!(variants[0].target_device_pixel_ratio, None);
        assert!(variants[0].main);
    }

    #[test]
    fn the_asset_list_is_the_keys_and_not_the_files() {
        // Three files, one asset. A caller asks for the key, and picking
        // between the files is what the variants are for.
        assert_eq!(
            manifest().list_assets(),
            vec!["images/logo.png".to_string()]
        );
    }

    #[test]
    fn a_message_that_is_not_a_map_is_not_a_manifest() {
        assert!(AssetManifest::from_value(&Value::Null).is_none());
        assert!(AssetManifest::from_value(&Value::List(vec![])).is_none());
        // And a manifest with nothing in it is a manifest.
        assert_eq!(
            AssetManifest::from_value(&Value::Map(vec![]))
                .expect("a manifest")
                .list_assets(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_manifest_survives_the_trip_through_the_standard_codec() {
        // The bytes on disk are what the build wrote with this codec, so
        // decoding them is the only way the manifest is ever read.
        let value = Value::Map(vec![(
            Value::String("images/logo.png".to_string()),
            Value::List(vec![
                variant("images/logo.png", Some(1.0)),
                variant("images/2.0x/logo.png", Some(2.0)),
            ]),
        )]);
        let bytes = StandardMessageCodec.encode(&value).expect("encodes");
        let manifest = AssetManifest::from_standard_message(&bytes).expect("a manifest");
        assert_eq!(manifest.list_assets(), vec!["images/logo.png".to_string()]);
        assert_eq!(manifest.asset_variants("images/logo.png").unwrap().len(), 2);
    }

    #[test]
    fn the_two_filenames_are_the_ones_the_build_writes() {
        // The web one is recorded rather than read: base64 inside JSON,
        // because a browser cannot be relied on to fetch binary.
        assert_eq!(AssetManifest::FILENAME, "AssetManifest.bin");
        assert_eq!(AssetManifest::WEB_FILENAME, "AssetManifest.bin.json");
    }
}
