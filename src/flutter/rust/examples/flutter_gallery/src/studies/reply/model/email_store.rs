// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/model/email_store.dart` (flutter/gallery @
//! d12640d): the twelve sample emails and the `EmailStore` state machine.
//!
//! Every sender, time, subject, message and recipient is upstream's string,
//! verbatim, in upstream's order (inbox 1-9, outbox 10-11, drafts 12).
//!
//! Upstream's `EmailStore` is a `ChangeNotifier` provided at the app root;
//! here it is plain data inside `reply::app::ReplyState` (the study's
//! `StatefulComponent` state), mutated through the `StateHandle` the build is
//! given -- `notifyListeners()` is a rebuild, and the framework already
//! rebuilds after every `set_state`.
//!
//! The avatar bytes are upstream's `flutter_gallery_assets` files
//! (`reply/avatars/`), compiled in with `include_bytes!`; see
//! `assets/README.md` for provenance. The upstream path strings are kept as
//! cache keys.

use std::collections::BTreeSet;

use super::email_model::{Avatar, Email, InboxType, MailboxPageType};

const AVATARS: &str = "reply/avatars";

macro_rules! avatar {
    ($file:literal) => {
        Avatar {
            key: concat!("reply/avatars/", $file),
            bytes: include_bytes!(concat!("../../../../assets/reply/avatars/", $file)),
        }
    };
}

/// Upstream's `_avatarsLocation`, kept for provenance: the include paths and
/// cache keys above are built from the same string.
#[allow(dead_code)]
const AVATARS_LOCATION: &str = AVATARS;

/// Upstream's `_inbox`.
static INBOX: &[Email] = &[
    Email {
        id: 1,
        sender: "Google Express",
        time: "15 minutes ago",
        subject: "Package shipped!",
        message: "Cucumber Mask Facial has shipped.\n\nKeep an eye out for a package to arrive between this Thursday and next Tuesday. If for any reason you don't receive your package before the end of next week, please reach out to us for details on your shipment.\n\nAs always, thank you for shopping with us and we hope you love our specially formulated Cucumber Mask!",
        avatar: avatar!("avatar_express.png"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 2,
        sender: "Ali Connors",
        time: "4 hrs ago",
        subject: "Brunch this weekend?",
        message: "I'll be in your neighborhood doing errands and was hoping to catch you for a coffee this Saturday. If you don't have anything scheduled, it would be great to see you! It feels like its been forever.\n\nIf we do get a chance to get together, remind me to tell you about Kim. She stopped over at the house to say hey to the kids and told me all about her trip to Mexico.\n\nTalk to you soon,\n\nAli",
        avatar: avatar!("avatar_5.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 3,
        sender: "Allison Trabucco",
        time: "5 hrs ago",
        subject: "Bonjour from Paris",
        message: "Here are some great shots from my trip...",
        avatar: avatar!("avatar_3.jpg"),
        recipients: "Jeff",
        contains_pictures: true,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 4,
        sender: "Trevor Hansen",
        time: "9 hrs ago",
        subject: "Brazil trip",
        message: "Thought we might be able to go over some details about our upcoming vacation.\n\nI've been doing a bit of research and have come across a few paces in Northern Brazil that I think we should check out. One, the north has some of the most predictable wind on the planet. I'd love to get out on the ocean and kitesurf for a couple of days if we're going to be anywhere near or around Taiba. I hear it's beautiful there and if you're up for it, I'd love to go. Other than that, I haven't spent too much time looking into places along our road trip route. I'm assuming we can find places to stay and things to do as we drive and find places we think look interesting. But... I know you're more of a planner, so if you have ideas or places in mind, lets jot some ideas down!\n\nMaybe we can jump on the phone later today if you have a second.",
        avatar: avatar!("avatar_8.jpg"),
        recipients: "Allison, Kim, Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 5,
        sender: "Frank Hawkins",
        time: "10 hrs ago",
        subject: "Update to Your Itinerary",
        message: "",
        avatar: avatar!("avatar_4.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 6,
        sender: "Google Express",
        time: "12 hrs ago",
        subject: "Delivered",
        message: "Your shoes should be waiting for you at home!",
        avatar: avatar!("avatar_express.png"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 7,
        sender: "Frank Hawkins",
        time: "4 hrs ago",
        subject: "Your update on the Google Play Store is live!",
        message: "Your update is now live on the Play Store and available for your alpha users to start testing.\n\nYour alpha testers will be automatically notified. If you'd rather send them a link directly, go to your Google Play Console and follow the instructions for obtaining an open alpha testing link.",
        avatar: avatar!("avatar_4.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 8,
        sender: "Allison Trabucco",
        time: "6 hrs ago",
        subject: "Try a free TrailGo account",
        message: "Looking for the best hiking trails in your area? TrailGo gets you on the path to the outdoors faster than you can pack a sandwich.\n\nWhether you're an experienced hiker or just looking to get outside for the afternoon, there's a segment that suits you.",
        avatar: avatar!("avatar_3.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 9,
        sender: "Allison Trabucco",
        time: "4 hrs ago",
        subject: "Free money",
        message: "You've been selected as a winner in our latest raffle! To claim your prize, click on the link.",
        avatar: avatar!("avatar_3.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Spam,
    },
];

/// Upstream's `_outbox`.
static OUTBOX: &[Email] = &[
    Email {
        id: 10,
        sender: "Kim Alen",
        time: "4 hrs ago",
        subject: "High school reunion?",
        message: "Hi friends,\n\nI was at the grocery store on Sunday night.. when I ran into Genie Williams! I almost didn't recognize her afer 20 years!\n\nAnyway, it turns out she is on the organizing committee for the high school reunion this fall. I don't know if you were planning on going or not, but she could definitely use our help in trying to track down lots of missing alums. If you can make it, we're doing a little phone-tree party at her place next Saturday, hoping that if we can find one person, thee more will...",
        avatar: avatar!("avatar_7.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
    Email {
        id: 11,
        sender: "Sandra Adams",
        time: "7 hrs ago",
        subject: "Recipe to try",
        message: "Raspberry Pie: We should make this pie recipe tonight! The filling is very quick to put together.",
        avatar: avatar!("avatar_2.jpg"),
        recipients: "Jeff",
        contains_pictures: false,
        inbox_type: InboxType::Normal,
    },
];

/// Upstream's `_drafts`.
static DRAFTS: &[Email] = &[Email {
    id: 12,
    sender: "Sandra Adams",
    time: "2 hrs ago",
    subject: "(No subject)",
    message: "Hey,\n\nWanted to email and see what you thought of",
    avatar: avatar!("avatar_2.jpg"),
    recipients: "Jeff",
    contains_pictures: false,
    inbox_type: InboxType::Normal,
}];

/// Upstream's `_allEmails`: inbox, then outbox, then drafts.
pub fn all_emails() -> impl Iterator<Item = &'static Email> {
    INBOX.iter().chain(OUTBOX).chain(DRAFTS)
}

/// Upstream's `EmailStore` -- everything the Reply app remembers, minus the
/// framework's notifyListeners (see the module header).
#[derive(Clone, Debug)]
pub struct EmailStore {
    /// Upstream's `starredEmailIds`.
    pub starred_email_ids: BTreeSet<i32>,
    /// Upstream's `trashEmailIds`, which starts holding 7 and 8.
    pub trash_email_ids: BTreeSet<i32>,
    /// Upstream's `_selectedEmailId`; -1 is "no mail open".
    pub selected_email_id: i32,
    /// Upstream's `_selectedMailboxPage`.
    pub selected_mailbox_page: MailboxPageType,
    /// Upstream's `_onSearchPage`.
    pub on_search_page: bool,
}

impl Default for EmailStore {
    fn default() -> EmailStore {
        EmailStore {
            starred_email_ids: BTreeSet::new(),
            trash_email_ids: BTreeSet::from([7, 8]),
            selected_email_id: -1,
            selected_mailbox_page: MailboxPageType::Inbox,
            on_search_page: false,
        }
    }
}

impl EmailStore {
    /// Upstream's `inboxEmails`: normal inbox mail that has not been trashed.
    pub fn inbox_emails(&self) -> Vec<&'static Email> {
        INBOX
            .iter()
            .filter(|email| {
                email.inbox_type == InboxType::Normal && !self.trash_email_ids.contains(&email.id)
            })
            .collect()
    }

    /// Upstream's `spamEmails`.
    pub fn spam_emails(&self) -> Vec<&'static Email> {
        INBOX
            .iter()
            .filter(|email| {
                email.inbox_type == InboxType::Spam && !self.trash_email_ids.contains(&email.id)
            })
            .collect()
    }

    /// Upstream's `outboxEmails`.
    pub fn outbox_emails(&self) -> Vec<&'static Email> {
        OUTBOX
            .iter()
            .filter(|email| !self.trash_email_ids.contains(&email.id))
            .collect()
    }

    /// Upstream's `draftEmails`.
    pub fn draft_emails(&self) -> Vec<&'static Email> {
        DRAFTS
            .iter()
            .filter(|email| !self.trash_email_ids.contains(&email.id))
            .collect()
    }

    /// Upstream's `trashEmails`: everything whose id has been deleted, in
    /// `_allEmails` order.
    pub fn trash_emails(&self) -> Vec<&'static Email> {
        all_emails()
            .filter(|email| self.trash_email_ids.contains(&email.id))
            .collect()
    }

    /// Upstream's `starredEmails`.
    pub fn starred_emails(&self) -> Vec<&'static Email> {
        all_emails()
            .filter(|email| self.starred_email_ids.contains(&email.id))
            .collect()
    }

    /// Upstream's `isEmailStarred`: the id has to exist AND be starred.
    pub fn is_email_starred(&self, id: i32) -> bool {
        all_emails().any(|email| email.id == id) && self.starred_email_ids.contains(&id)
    }

    /// Upstream's `currentEmail`. Valid only while `on_mail_view()` holds;
    /// upstream's `firstWhere` throws otherwise, so this expects.
    pub fn current_email(&self) -> &'static Email {
        all_emails()
            .find(|email| email.id == self.selected_email_id)
            .expect("currentEmail with no selection")
    }

    /// Upstream's `isCurrentEmailStarred`.
    pub fn is_current_email_starred(&self) -> bool {
        self.starred_email_ids.contains(&self.current_email().id)
    }

    /// Upstream's `starEmail`.
    pub fn star_email(&mut self, id: i32) {
        self.starred_email_ids.insert(id);
    }

    /// Upstream's `unstarEmail`.
    pub fn unstar_email(&mut self, id: i32) {
        self.starred_email_ids.remove(&id);
    }

    /// Upstream's `deleteEmail`: deletion is moving to trash.
    pub fn delete_email(&mut self, id: i32) {
        self.trash_email_ids.insert(id);
    }

    /// Upstream's `onMailView`.
    pub fn on_mail_view(&self) -> bool {
        self.selected_email_id > -1
    }

    /// Upstream's `selectedMailboxPage` setter, plus what
    /// `_onDestinationSelected` does alongside it in `adaptive_nav.dart`:
    /// leaving a mailbox leaves any open mail too.
    pub fn select_mailbox_page(&mut self, page: MailboxPageType) {
        self.selected_mailbox_page = page;
        if self.on_mail_view() {
            self.selected_email_id = -1;
        }
    }

    /// The emails the currently selected mailbox page lists, upstream's
    /// `switch (destination)` in `mailbox_body.dart`'s `MailboxBody.build`.
    pub fn selected_mailbox_emails(&self) -> Vec<&'static Email> {
        match self.selected_mailbox_page {
            MailboxPageType::Inbox => self.inbox_emails(),
            MailboxPageType::Sent => self.outbox_emails(),
            MailboxPageType::Starred => self.starred_emails(),
            MailboxPageType::Trash => self.trash_emails(),
            MailboxPageType::Spam => self.spam_emails(),
            MailboxPageType::Drafts => self.draft_emails(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_is_upstreams_twelve_emails() {
        assert_eq!(INBOX.len(), 9);
        assert_eq!(OUTBOX.len(), 2);
        assert_eq!(DRAFTS.len(), 1);
        assert_eq!(all_emails().count(), 12);
        // Ids are contiguous 1..=12 in upstream's declaration order.
        for (index, email) in all_emails().enumerate() {
            assert_eq!(email.id, index as i32 + 1);
        }
    }

    #[test]
    fn the_strings_are_upstreams_verbatim() {
        let first = &INBOX[0];
        assert_eq!(first.sender, "Google Express");
        assert_eq!(first.time, "15 minutes ago");
        assert_eq!(first.subject, "Package shipped!");
        assert!(
            first
                .message
                .starts_with("Cucumber Mask Facial has shipped.")
        );
        assert!(first.message.ends_with("Cucumber Mask!"));
        assert_eq!(first.recipients, "Jeff");
        assert_eq!(first.avatar.key, "reply/avatars/avatar_express.png");

        // The spam email, upstream's only InboxType.spam row.
        assert_eq!(INBOX[8].subject, "Free money");
        assert_eq!(INBOX[8].inbox_type, InboxType::Spam);

        // Upstream's typos are upstream's: "afer", "thee more will".
        assert!(OUTBOX[0].message.contains("afer 20 years"));
        assert!(OUTBOX[0].message.contains("thee more will..."));
        assert_eq!(DRAFTS[0].subject, "(No subject)");

        // The one email with attachments.
        let paris = &INBOX[2];
        assert_eq!(paris.subject, "Bonjour from Paris");
        assert!(paris.contains_pictures);
        assert!(all_emails().filter(|e| e.contains_pictures).count() == 1);
    }

    #[test]
    fn trash_starts_with_seven_and_eight() {
        let store = EmailStore::default();
        assert!(store.trash_email_ids.contains(&7));
        assert!(store.trash_email_ids.contains(&8));
        assert_eq!(store.trash_emails().len(), 2);
        // So the inbox shows six of its eight normal rows, and the spam row
        // -- id 9, not trashed -- is still in the spam box.
        assert_eq!(store.inbox_emails().len(), 6);
        assert_eq!(store.spam_emails().len(), 1);
        assert_eq!(store.outbox_emails().len(), 2);
        assert_eq!(store.draft_emails().len(), 1);
    }

    #[test]
    fn starring_and_unstarring() {
        let mut store = EmailStore::default();
        assert!(store.starred_emails().is_empty());
        store.star_email(2);
        store.star_email(10);
        assert_eq!(store.starred_emails().len(), 2);
        assert!(store.is_email_starred(2));
        // Upstream: an id that names no email is never "starred".
        assert!(!store.is_email_starred(42));
        store.unstar_email(2);
        assert!(!store.is_email_starred(2));
        assert_eq!(store.starred_emails()[0].id, 10);
    }

    #[test]
    fn deleting_moves_mail_to_trash() {
        let mut store = EmailStore::default();
        store.delete_email(1);
        assert_eq!(store.inbox_emails().len(), 5);
        assert_eq!(store.trash_emails().len(), 3);
        assert_eq!(
            store.trash_emails()[0].id,
            1,
            "trash lists in _allEmails order"
        );
    }

    #[test]
    fn selection_drives_the_mail_view() {
        let mut store = EmailStore::default();
        assert!(!store.on_mail_view());
        store.selected_email_id = 4;
        assert!(store.on_mail_view());
        assert_eq!(store.current_email().subject, "Brazil trip");
        assert!(!store.is_current_email_starred());
        store.star_email(4);
        assert!(store.is_current_email_starred());
    }

    #[test]
    fn switching_mailboxes_clears_the_open_mail() {
        // Upstream's `_onDestinationSelected`: a destination tap pops any open
        // mail view and resets the selection.
        let mut store = EmailStore::default();
        store.selected_email_id = 2;
        store.select_mailbox_page(MailboxPageType::Starred);
        assert_eq!(store.selected_email_id, -1);
        assert_eq!(store.selected_mailbox_page, MailboxPageType::Starred);
        assert_eq!(store.selected_mailbox_emails().len(), 0);
    }

    #[test]
    fn the_mailbox_switch_matches_upstreams() {
        let mut store = EmailStore::default();
        store.star_email(9); // star the spam row: it shows in Starred, not Inbox.
        let counts: Vec<(MailboxPageType, usize)> = MailboxPageType::ALL
            .iter()
            .map(|&page| {
                store.selected_mailbox_page = page;
                (page, store.selected_mailbox_emails().len())
            })
            .collect();
        assert_eq!(
            counts,
            [
                (MailboxPageType::Inbox, 6),
                (MailboxPageType::Starred, 1),
                (MailboxPageType::Sent, 2),
                (MailboxPageType::Trash, 2),
                (MailboxPageType::Spam, 1),
                (MailboxPageType::Drafts, 1),
            ]
        );
    }
}
