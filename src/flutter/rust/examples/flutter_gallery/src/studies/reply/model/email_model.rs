// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/model/email_model.dart` (flutter/gallery @
//! d12640d): the `Email` record and the two enums.
//!
//! Upstream's `InboxEmail` is a subclass of `Email` adding `inboxType`; Rust
//! has no inheritance, so the field is flattened onto [`Email`] and only the
//! inbox rows set it to anything but [`InboxType::Normal`] -- the same
//! information, one struct instead of two. The divergence is structural only.
//!
//! Upstream's `avatar` is an asset path into the `flutter_gallery_assets`
//! package (`reply/avatars/avatar_N.jpg`). There is no asset bundle here, so
//! the bytes are compiled in and the upstream path is kept as the image cache
//! key -- [`Avatar::key`] names the upstream file for provenance.

/// One avatar photograph: the decoded-once bytes and the upstream asset path
/// they came from, which doubles as the `Image::shared` cache key.
#[derive(Clone, Copy, Debug)]
pub struct Avatar {
    /// The upstream asset path, e.g. `reply/avatars/avatar_5.jpg`.
    pub key: &'static str,
    pub bytes: &'static [u8],
}

/// Upstream's `Email` (with `InboxEmail.inboxType` flattened in, see the
/// module header).
#[derive(Clone, Copy, Debug)]
pub struct Email {
    pub id: i32,
    pub sender: &'static str,
    pub time: &'static str,
    pub subject: &'static str,
    pub message: &'static str,
    pub avatar: Avatar,
    pub recipients: &'static str,
    pub contains_pictures: bool,
    /// Upstream: `InboxEmail.inboxType`, present only on inbox rows.
    pub inbox_type: InboxType,
}

/// The different mailbox pages that the Reply app contains.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MailboxPageType {
    #[default]
    Inbox,
    Starred,
    Sent,
    Trash,
    Spam,
    Drafts,
}

impl MailboxPageType {
    /// The lowercased name upstream derives with
    /// `destination.toString().substring(...)` for the empty-mailbox message
    /// ("Empty in inbox").
    pub fn name(self) -> &'static str {
        match self {
            MailboxPageType::Inbox => "inbox",
            MailboxPageType::Starred => "starred",
            MailboxPageType::Sent => "sent",
            MailboxPageType::Trash => "trash",
            MailboxPageType::Spam => "spam",
            MailboxPageType::Drafts => "drafts",
        }
    }

    /// The label the bottom app bar shows for this mailbox -- upstream's
    /// `_Destination.textLabel`, which comes from the localizations
    /// (`replyInboxLabel` and friends, `intl_en.arb`).
    pub fn label(self) -> &'static str {
        match self {
            MailboxPageType::Inbox => "Inbox",
            MailboxPageType::Starred => "Starred",
            MailboxPageType::Sent => "Sent",
            MailboxPageType::Trash => "Trash",
            MailboxPageType::Spam => "Spam",
            MailboxPageType::Drafts => "Drafts",
        }
    }

    /// All six, in upstream's declaration order -- the navigation destinations
    /// are built in this order.
    pub const ALL: [MailboxPageType; 6] = [
        MailboxPageType::Inbox,
        MailboxPageType::Starred,
        MailboxPageType::Sent,
        MailboxPageType::Trash,
        MailboxPageType::Spam,
        MailboxPageType::Drafts,
    ];
}

/// Different types of mail that can be sent to the inbox.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InboxType {
    #[default]
    Normal,
    Spam,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_names_match_upstreams_lowercased_enum_names() {
        let names: Vec<&str> = MailboxPageType::ALL.iter().map(|p| p.name()).collect();
        assert_eq!(
            names,
            ["inbox", "starred", "sent", "trash", "spam", "drafts"]
        );
    }

    #[test]
    fn the_labels_are_upstreams_english_strings() {
        // intl_en.arb: replyInboxLabel .. replyDraftsLabel.
        let labels: Vec<&str> = MailboxPageType::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(
            labels,
            ["Inbox", "Starred", "Sent", "Trash", "Spam", "Drafts"]
        );
    }

    #[test]
    fn inbox_type_defaults_to_normal() {
        assert_eq!(InboxType::default(), InboxType::Normal);
    }
}
