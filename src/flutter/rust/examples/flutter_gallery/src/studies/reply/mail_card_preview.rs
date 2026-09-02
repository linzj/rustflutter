// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/mail_card_preview.dart` (flutter/gallery @
//! d12640d): one email as a row in a mailbox.
//!
//! Upstream is three widgets. `MailPreviewCard` is an `OpenContainer` that
//! grows into the `MailViewPage`, wrapping a `Dismissible` whose two
//! backgrounds are the delete and star actions; `_MailPreview` is the card's
//! own content; `_PicturePreview` is the horizontal strip of attachments.
//!
//! The content is ported whole -- every padding, every text style. What is
//! not, and why:
//!
//! * **The container does not open.** Upstream's `OpenContainer` is the
//!   container-transform: the card grows into the full mail view. That needs
//!   `mail_view_page.rs`, which is still a skeleton, so a tap sets the store's
//!   `selected_email_id` and nothing has been built to show for it yet. The
//!   tap is wired rather than omitted because the store is what the next batch
//!   reads.
//! * **The swipes are not wired.** Upstream's `Dismissible` deletes on a
//!   start-to-end swipe past 80% and stars on an end-to-start swipe past 40%,
//!   over two coloured backgrounds bearing `twotone_delete` and
//!   `twotone_star`. The framework has a `Dismissible`, but the two icons are
//!   not among the vendored assets (`assets/reply/` has the avatars, the
//!   attachments and the logo, and no `icons/`), so the backgrounds would be
//!   bare colour. Deferred with the icons rather than half-drawn.
//! * **Desktop is not here.** Upstream's `_MailPreviewActionBar` shows star,
//!   delete and more-vert buttons beside the avatar on a wide window and
//!   *only the avatar* on a narrow one. This renders the narrow arm, which is
//!   the one the phone shows; see `adaptive_nav.rs` on the width branch.

use rustflutter::framework::{AnyWidget, BuildContext, Component, StateHandle, leaf, many};
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    BoxFit, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisSize, RenderClipRect, RenderFlex,
    RenderPadding, RenderRef, TextOverflow,
};
use rustflutter::widgets::{Container, ImageView, Pointer};

use crate::app::{GalleryState, ids};

use super::model::email_model::Email;
use super::profile_avatar::ProfileAvatar;

/// Upstream's `EdgeInsets.all(20)` on `_MailPreview`'s content.
const CARD_PADDING: f32 = 20.0;

/// The gap between the sender line and the subject.
const SENDER_TO_SUBJECT: f32 = 4.0;

/// The gap between the subject and the body preview.
const SUBJECT_TO_MESSAGE: f32 = 16.0;

/// Upstream's `EdgeInsetsDirectional.only(end: 20)` on the body preview, so
/// the one line ellipsises before it reaches the avatar rather than under it.
const MESSAGE_END_INSET: f32 = 20.0;

/// Upstream's `_PicturePreview`: `SizedBox(height: 96)`.
const PICTURE_STRIP_HEIGHT: f32 = 96.0;

/// The gap before the attachment strip.
const MESSAGE_TO_PICTURES: f32 = 20.0;

/// The gap between two attachments, upstream's
/// `EdgeInsetsDirectional.only(end: 4)`.
const PICTURE_GAP: f32 = 4.0;

/// Upstream's four `reply/attachments/paris_N.jpg`, compiled in; there is no
/// asset bundle here (see `assets/README.md`).
const ATTACHMENTS: [(&str, &[u8]); 4] = [
    (
        "reply/attachments/paris_1.jpg",
        include_bytes!("../../../assets/reply/attachments/paris_1.jpg"),
    ),
    (
        "reply/attachments/paris_2.jpg",
        include_bytes!("../../../assets/reply/attachments/paris_2.jpg"),
    ),
    (
        "reply/attachments/paris_3.jpg",
        include_bytes!("../../../assets/reply/attachments/paris_3.jpg"),
    ),
    (
        "reply/attachments/paris_4.jpg",
        include_bytes!("../../../assets/reply/attachments/paris_4.jpg"),
    ),
];

/// Upstream's `MailPreviewCard`.
pub struct MailPreviewCard {
    pub email: &'static Email,
    pub handle: StateHandle<GalleryState>,
}

impl Component for MailPreviewCard {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = super::app::reply_theme_of(context);
        let email = self.email;
        let handle = self.handle.clone();

        // Upstream's three text styles, asked for at the site because a
        // `Theme` carries two sizes and this card uses three:
        //   bodySmall    12 w400  -- "<sender> - <time>"
        //   headlineSmall 24 bold -- the subject
        //   bodyMedium   14 w400  -- the one-line body preview
        let sender_style = TextStyle {
            font_size: 12.0,
            color: theme.text,
            font_family: theme.font_family.map(str::to_string),
            ..TextStyle::default()
        };
        let subject_style = TextStyle {
            font_size: 24.0,
            font_weight: 700,
            color: theme.text,
            font_family: theme.font_family.map(str::to_string),
            ..TextStyle::default()
        };
        let message_style = TextStyle {
            font_size: 14.0,
            color: theme.text,
            font_family: theme.font_family.map(str::to_string),
            ..TextStyle::default()
        };

        // The avatar is a component of its own; the rest of the card is a
        // render tree, so it is built as a child and placed by `many`.
        let mut children: Vec<AnyWidget> = vec![rustflutter::framework::component(
            ProfileAvatar::new(email.avatar),
        )];
        if email.contains_pictures {
            children.push(picture_strip());
        }
        let has_pictures = email.contains_pictures;
        let card = theme.surface;

        // Upstream sets `selectedEmailId` on tap; the view it then opens into
        // is the next batch's, see the module header.
        let id = email.id;
        let tapped = handle;

        many(children, move |mut rendered| {
            let pictures = if has_pictures { rendered.pop() } else { None };
            let avatar = rendered.pop().expect("the avatar");

            // The head: sender/time over subject, with the avatar beside them.
            let text_column = RenderFlex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_main_axis_size(MainAxisSize::Min)
                .push(
                    Text::new(format!("{} - {}", email.sender, email.time))
                        .with_style(sender_style.clone()),
                )
                .push(Container::new().with_size(1.0, SENDER_TO_SUBJECT))
                .push(Text::new(email.subject).with_style(subject_style.clone()))
                .push(Container::new().with_size(1.0, SUBJECT_TO_MESSAGE));

            let head = RenderFlex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .push_flex(FlexChild::expanded(RenderRef::new(text_column), 1))
                .push(avatar);

            let mut column = RenderFlex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_main_axis_size(MainAxisSize::Min)
                .push(head)
                // The body preview: one line, ellipsised, held off the
                // avatar's column by the end inset.
                .push(RenderPadding::new(
                    EdgeInsets::only(0.0, 0.0, MESSAGE_END_INSET, 0.0),
                    Text::new(email.message)
                        .with_style(message_style.clone())
                        .with_max_lines(1)
                        .with_overflow(TextOverflow::Ellipsis),
                ));
            if let Some(pictures) = pictures {
                column = column
                    .push(Container::new().with_size(1.0, MESSAGE_TO_PICTURES))
                    .push(pictures);
            }

            Box::new(
                Pointer::new(
                    ids::STUDY_LOCAL + id as u64,
                    Container::new()
                        .with_color(card)
                        .with_padding(EdgeInsets::all(CARD_PADDING))
                        .with_child(column),
                )
                .with_handlers(
                    rustflutter::gestures::PointerHandlers::new().with_tap({
                        let tapped = tapped.clone();
                        move |_| {
                            tapped.set_state(move |state| {
                                state.study.reply.selected_email_id = id;
                            });
                        }
                    }),
                ),
            )
        })
    }
}

/// Upstream's `_PicturePreview`: a fixed-height horizontal strip of the four
/// Paris photographs at their natural aspect.
///
/// Upstream is a horizontally scrolling `ListView.builder`; this is a row.
/// The four are what upstream hard-codes (`itemCount: 4`), and at the
/// aspect ratios they have they overflow a phone's width -- upstream scrolls
/// to reach the fourth, and here the row is clipped. Recorded rather than
/// faked: a scroll view inside a list row needs the row to give it a bounded
/// cross-axis, which is the sliver-in-sliver case the port does not have.
fn picture_strip() -> AnyWidget {
    leaf(move || {
        let mut row = RenderFlex::row().with_cross_axis_alignment(CrossAxisAlignment::Start);
        for (key, bytes) in ATTACHMENTS {
            let mut cell = Container::new().with_size(PICTURE_STRIP_HEIGHT, PICTURE_STRIP_HEIGHT);
            if let Some(photo) = Image::shared(key, bytes) {
                cell = cell.with_child(ImageView::with_fit(photo, BoxFit::Cover));
            }
            row = row
                .push(cell)
                .push(Container::new().with_size(PICTURE_GAP, 1.0));
        }
        RenderClipRect::new(
            Container::new()
                .with_size(f32::INFINITY, PICTURE_STRIP_HEIGHT)
                .with_child(row),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paddings_are_upstreams() {
        // mail_card_preview.dart: EdgeInsets.all(20) around the content,
        // 4 between sender and subject, 16 between subject and message,
        // 20 before the pictures, and a 96-high strip.
        assert_eq!(CARD_PADDING, 20.0);
        assert_eq!(SENDER_TO_SUBJECT, 4.0);
        assert_eq!(SUBJECT_TO_MESSAGE, 16.0);
        assert_eq!(MESSAGE_END_INSET, 20.0);
        assert_eq!(MESSAGE_TO_PICTURES, 20.0);
        assert_eq!(PICTURE_STRIP_HEIGHT, 96.0);
        assert_eq!(PICTURE_GAP, 4.0);
    }

    #[test]
    fn the_strip_is_upstreams_four_photographs() {
        // `itemCount: 4`, `paris_${index + 1}.jpg`.
        assert_eq!(ATTACHMENTS.len(), 4);
        for (index, (key, bytes)) in ATTACHMENTS.iter().enumerate() {
            assert_eq!(*key, format!("reply/attachments/paris_{}.jpg", index + 1));
            assert!(!bytes.is_empty(), "the photograph is compiled in");
        }
    }
}
