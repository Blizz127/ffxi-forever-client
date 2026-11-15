//! UNWIRED preparation for owner122dd30e. No parser, World integration, config,
//! commands, account state or privilege grants. Server remains authoritative.
//! A future adapter must authenticate the receiving game session, accept ERA1
//! only from the system feed, and bind it to that session's current character.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub generation: u64,
    pub character: u32,
    pub zone: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Typed output of a future strict parser, not a local preference or GM badge.
    Capability {
        gm: bool,
        stage: u32,
    },
    InvalidOrUnsupported,
}

#[derive(Default, Debug)]
pub struct Visibility {
    context: Option<Context>,
    confirmed: bool,
    stage: Option<u32>,
}

impl Visibility {
    /// Entering/re-entering any zone starts restricted, even for the same char.
    /// The adapter allocates a new generation at EVERY zone/session boundary.
    pub fn enter(&mut self, context: Context) {
        *self = Self { context: Some(context), ..Self::default() };
    }

    pub fn disconnect(&mut self) {
        *self = Self::default();
    }

    /// Only a message in the exact current receiving context may update state.
    /// Stage is informational: it never grants browsing or server permissions.
    pub fn receive(&mut self, context: Context, forever: bool, system_feed: bool, notice: Notice) {
        if self.context != Some(context) || !forever || !system_feed {
            return;
        }
        match notice {
            Notice::Capability { gm, stage } => {
                self.confirmed = gm;
                self.stage = Some(stage);
            }
            Notice::InvalidOrUnsupported => {
                self.confirmed = false;
                self.stage = None;
            }
        }
    }

    /// Presentation only. Never use this result to approve a game action.
    pub fn show_later_content(&self, context: Context) -> bool {
        self.context == Some(context) && self.confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GM: Context = Context { generation: 1, character: 41, zone: 240 };
    // Both fixture chars could share an account; account identity is deliberately
    // absent from this capability model, so it cannot grant to siblings.
    const SIBLING: Context = Context { generation: 2, character: 42, zone: 240 };
    fn grant(v: &mut Visibility, c: Context) {
        v.receive(c, true, true, Notice::Capability { gm: true, stage: 0 });
    }
    #[test]
    fn absent_capability_is_restricted() {
        let mut v = Visibility::default();
        assert!(!v.show_later_content(GM));
        v.enter(GM);
        assert!(!v.show_later_content(GM));
    }
    #[test]
    fn exact_server_context_enables_presentation_only() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        assert!(v.show_later_content(GM));
        assert!(!v.show_later_content(SIBLING));
    }
    #[test]
    fn sibling_does_not_inherit_or_accept_old_event() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        v.enter(SIBLING);
        grant(&mut v, GM);
        assert!(!v.show_later_content(SIBLING));
    }
    #[test]
    fn relog_requires_new_server_confirmation() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        v.disconnect();
        let next = Context { generation: 2, ..GM };
        v.enter(next);
        grant(&mut v, GM);
        assert!(!v.show_later_content(next));
        grant(&mut v, next);
        assert!(v.show_later_content(next));
    }
    #[test]
    fn same_zone_reentry_clears_confirmation() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        let next = Context { generation: 2, ..GM };
        v.enter(next);
        assert!(!v.show_later_content(next));
    }
    #[test]
    fn zone_change_rejects_late_old_zone_message() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        let next = Context { generation: 2, zone: 241, ..GM };
        v.enter(next);
        grant(&mut v, GM);
        assert!(!v.show_later_content(next));
    }
    #[test]
    fn explicit_revocation_and_invalid_feed_fail_closed() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        v.receive(GM, true, true, Notice::Capability { gm: false, stage: 0 });
        assert!(!v.show_later_content(GM));
        grant(&mut v, GM);
        v.receive(GM, true, true, Notice::InvalidOrUnsupported);
        assert!(!v.show_later_content(GM));
    }
    #[test]
    fn classic_and_player_chat_cannot_grant() {
        for (forever, system) in [(false, true), (true, false), (false, false)] {
            let mut v = Visibility::default();
            v.enter(GM);
            v.receive(GM, forever, system, Notice::Capability { gm: true, stage: 0 });
            assert!(!v.show_later_content(GM));
        }
    }
    #[test]
    fn stage_alone_never_unlocks_content() {
        let mut v = Visibility::default();
        v.enter(GM);
        v.receive(GM, true, true, Notice::Capability { gm: false, stage: u32::MAX });
        assert!(!v.show_later_content(GM));
    }
    #[test]
    fn disconnect_removes_all_confirmation() {
        let mut v = Visibility::default();
        v.enter(GM);
        grant(&mut v, GM);
        v.disconnect();
        grant(&mut v, GM);
        assert!(!v.show_later_content(GM));
        assert_eq!(v.stage, None);
    }
}
