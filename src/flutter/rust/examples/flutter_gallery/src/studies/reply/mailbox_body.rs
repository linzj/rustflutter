// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/mailbox_body.dart` (flutter/gallery @
//! d12640d): the selected mailbox as a list of [`MailPreviewCard`]s.
//!
//! Upstream is a `Consumer<EmailStore>` that switches on
//! `selectedMailboxPage` for the list, then a `ListView.separated` of cards
//! with a 4dp separator. The switch is [`EmailStore::selected_mailbox_emails`]
//! -- it was already ported, and this is its first reader.
//!
//! The mobile paddings are upstream's: 4 at the start and end, nothing at the
//! top, and `kToolbarHeight` at the bottom so the last card clears the bottom
//! app bar the body extends under (`extendBody: true`).
//!
//! Divergences:
//!
//! * **The desktop arm is not here.** Upstream pads 120/60 (or 60/30 on a
//!   small desktop) and puts a search icon column beside the list. See
//!   `adaptive_nav.rs` on why only the mobile arm is rendered.
//! * **The list is eager.** Upstream's `ListView.separated` builds on demand;
//!   this pushes all of them into a `ListView`, which for six to twelve
//!   emails is the whole list either way.

use rustflutter::framework::{AnyWidget, BuildContext, Component, StateHandle, many};
use rustflutter::prelude::*;
use rustflutter::render::{
    CrossAxisAlignment, EdgeInsets, MainAxisSize, RenderFlex, RenderPadding,
};
use rustflutter::widgets::{Center, Container, ListView, Pointer};

use crate::app::{GalleryState, ids};

use super::mail_card_preview::MailPreviewCard;

/// Upstream's mobile `startPadding` / `endPadding`.
const HORIZONTAL_PADDING: f32 = 4.0;

/// Upstream's `separatorBuilder`: `SizedBox(height: 4)`.
const CARD_GAP: f32 = 4.0;

/// Upstream's `bottom: kToolbarHeight` -- the body draws under the bottom app
/// bar, so the list ends a bar's height above its own bottom.
const K_TOOLBAR_HEIGHT: f32 = 56.0;

/// Upstream's `MailboxBody`.
///
/// Upstream reads the store through a `Consumer<EmailStore>`; there is no
/// provider tree carrying mutable models here, so the rows and the mailbox's
/// name are handed in by whoever already holds the gallery's state -- the
/// same way every other study screen is given it.
pub struct MailboxBody {
    /// [`EmailStore::selected_mailbox_emails`], already switched on.
    pub emails: Vec<&'static super::model::email_model::Email>,
    /// The mailbox's lowercased name, for the empty label.
    pub destination: &'static str,
    /// Where the list is scrolled to, and how far it can go.
    pub offset: f32,
    pub link: std::rc::Rc<rustflutter::scrolling::ScrollLink>,
    pub handle: StateHandle<GalleryState>,
}

impl Component for MailboxBody {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = super::app::reply_theme_of(context);
        let destination = self.destination;

        if self.emails.is_empty() {
            // Upstream's `Center(child: Text('Empty in $destinationString'))`,
            // in the default body style.
            let style = TextStyle {
                font_size: 14.0,
                color: theme.text,
                font_family: theme.font_family.map(str::to_string),
                ..TextStyle::default()
            };
            let label = format!("Empty in {destination}");
            return rustflutter::framework::leaf(move || {
                Center::new(Text::new(label.clone()).with_style(style.clone()))
            });
        }

        let handle = self.handle.clone();
        let cards: Vec<AnyWidget> = self
            .emails
            .iter()
            .map(|email| {
                rustflutter::framework::component(MailPreviewCard {
                    email,
                    handle: handle.clone(),
                })
            })
            .collect();

        let offset = self.offset;
        let link = std::rc::Rc::clone(&self.link);
        let handlers = crate::app::scroll_handlers(
            self.handle.clone(),
            |state| &mut state.study.reply_scroll,
            rustflutter::render::Axis::Vertical,
        );

        many(cards, move |rendered| {
            let mut column = RenderFlex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_main_axis_size(MainAxisSize::Min);
            for (index, card) in rendered.into_iter().enumerate() {
                if index > 0 {
                    column = column.push(Container::new().with_size(1.0, CARD_GAP));
                }
                column = column.push(card);
            }
            let padded = RenderPadding::new(
                EdgeInsets::only(
                    HORIZONTAL_PADDING,
                    0.0,
                    HORIZONTAL_PADDING,
                    K_TOOLBAR_HEIGHT,
                ),
                column,
            );
            Box::new(
                Pointer::new(
                    ids::STUDY_LOCAL,
                    ListView::new()
                        .with_offset(offset)
                        .with_link(std::rc::Rc::clone(&link))
                        .push(padded),
                )
                .with_handlers(handlers.clone()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paddings_are_upstreams_mobile_arm() {
        // mailbox_body.dart: start/end 4 on mobile, top 0, bottom
        // kToolbarHeight, and a 4dp separator between cards.
        assert_eq!(HORIZONTAL_PADDING, 4.0);
        assert_eq!(CARD_GAP, 4.0);
        assert_eq!(K_TOOLBAR_HEIGHT, 56.0);
    }

    #[test]
    fn the_empty_label_is_upstreams_wording() {
        // Upstream lowercases the enum name into "Empty in inbox".
        use super::super::model::email_model::MailboxPageType;
        assert_eq!(MailboxPageType::Inbox.name(), "inbox");
        assert_eq!(
            format!("Empty in {}", MailboxPageType::Starred.name()),
            "Empty in starred"
        );
    }
}
