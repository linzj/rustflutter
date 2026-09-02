// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/profile_avatar.dart` (flutter/gallery @
//! d12640d): the circular sender photo.
//!
//! Upstream is a `CircleAvatar` clipping a 42x42 `BoxFit.cover` image with
//! `ClipOval`. The framework has no oval clip, so the clip is a `ClipRRect`
//! whose radius is half the (square) box -- a rounded rect with that radius
//! on a square IS the circle. Divergence, visually nil.

use rustflutter::framework::{AnyWidget, BuildContext, Component, leaf};
use rustflutter::painting::Image;
use rustflutter::render::BoxFit;
use rustflutter::widgets::{ClipRRect, Container, ImageView};

use super::model::email_model::Avatar;

/// Upstream's `ProfileAvatar`.
pub struct ProfileAvatar {
    pub avatar: Avatar,
    /// Upstream's `radius`, default 20.
    pub radius: f32,
}

impl ProfileAvatar {
    pub fn new(avatar: Avatar) -> ProfileAvatar {
        ProfileAvatar {
            avatar,
            radius: 20.0,
        }
    }
}

impl Component for ProfileAvatar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // Upstream's `backgroundColor: Theme.of(context).cardColor`, read from
        // the study's own theme rather than the gallery's. `cardColor` is the
        // `surface` slot here -- see `reply/theme.rs`'s mapping table.
        let card = super::app::reply_theme_of(context).surface;
        let avatar = self.avatar;
        let radius = self.radius;
        // Keyed by the upstream asset path, so the photograph decodes once
        // for the life of the process rather than once per frame.
        let photo = Image::shared(avatar.key, avatar.bytes);

        leaf(move || {
            let mut circle = Container::new()
                .with_size(radius * 2.0, radius * 2.0)
                .with_color(card)
                .with_corner_radius(radius);
            if let Some(photo) = photo.clone() {
                circle = circle.with_child(ClipRRect::new(
                    radius,
                    ImageView::with_fit(photo, BoxFit::Cover),
                ));
            }
            circle
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_radius_is_upstreams() {
        let avatar = Avatar {
            key: "k",
            bytes: &[],
        };
        assert_eq!(ProfileAvatar::new(avatar).radius, 20.0);
    }
}
