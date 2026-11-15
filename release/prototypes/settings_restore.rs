//! Unwired restore precondition model. No I/O, network, authentication or settings adapter.
//! Revisions stand for immutable validated snapshots; production must bind confirmation
//! and durable preimage to the exact content and perform the final CAS atomically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Account,
    Character(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    realm: u32,
    account: u32,
    scope: Scope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Preferences,
    Layout,
    Macros,
    JobActions,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Preview {
    identity: Identity,
    session: u64,
    schema: u32,
    section: Section,
    snapshot: u64,
    server_revision: u64,
    local_revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    Scope,
    Schema,
    Session,
    Changed,
    Confirmation,
    Preimage,
    Busy,
}
fn check(
    p: Preview,
    now: Preview,
    confirmation: Option<Preview>,
    preimage_revision: Option<u64>,
    idle: bool,
) -> Result<(), Refusal> {
    if p.identity != now.identity {
        return Err(Refusal::Scope);
    }
    if matches!(p.section, Section::Macros | Section::JobActions) && p.identity.scope == Scope::Account {
        return Err(Refusal::Scope);
    }
    if p.schema != 1 || now.schema != 1 {
        return Err(Refusal::Schema);
    }
    if p.session != now.session {
        return Err(Refusal::Session);
    }
    if p != now {
        return Err(Refusal::Changed);
    }
    if confirmation != Some(p) {
        return Err(Refusal::Confirmation);
    }
    if preimage_revision != Some(p.local_revision) {
        return Err(Refusal::Preimage);
    }
    if !idle {
        return Err(Refusal::Busy);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Preview {
        Preview {
            identity: Identity { realm: 1, account: 10, scope: Scope::Character(42) },
            session: 7,
            schema: 1,
            section: Section::Macros,
            snapshot: 20,
            server_revision: 21,
            local_revision: 5,
        }
    }
    fn valid(p: Preview, n: Preview) -> Result<(), Refusal> {
        check(p, n, Some(p), Some(p.local_revision), true)
    }
    #[test]
    fn explicit_exact_restore() {
        let p = sample();
        assert_eq!(valid(p, p), Ok(()));
    }
    #[test]
    fn realm_account_and_sibling_isolation() {
        let p = sample();
        for identity in [
            Identity { realm: 2, ..p.identity },
            Identity { account: 11, ..p.identity },
            Identity { scope: Scope::Character(43), ..p.identity },
        ] {
            assert_eq!(valid(p, Preview { identity, ..p }), Err(Refusal::Scope));
        }
    }
    #[test]
    fn account_templates_exclude_character_actions() {
        let p = Preview { identity: Identity { scope: Scope::Account, ..sample().identity }, ..sample() };
        for section in [Section::Macros, Section::JobActions] {
            let p = Preview { section, ..p };
            assert_eq!(valid(p, p), Err(Refusal::Scope));
        }
        for section in [Section::Preferences, Section::Layout] {
            let p = Preview { section, ..p };
            assert_eq!(valid(p, p), Ok(()));
        }
    }
    #[test]
    fn incompatible_schema_refused() {
        let p = Preview { schema: 2, ..sample() };
        assert_eq!(valid(p, p), Err(Refusal::Schema));
    }
    #[test]
    fn relog_invalidates_preview() {
        let p = sample();
        assert_eq!(valid(p, Preview { session: 8, ..p }), Err(Refusal::Session));
    }
    #[test]
    fn edits_uploads_and_different_snapshot_require_new_preview() {
        let p = sample();
        for n in [
            Preview { local_revision: 6, ..p },
            Preview { server_revision: 22, ..p },
            Preview { snapshot: 19, ..p },
            Preview { section: Section::Layout, ..p },
        ] {
            assert_eq!(valid(p, n), Err(Refusal::Changed));
        }
    }
    #[test]
    fn cancel_or_stale_confirmation() {
        let p = sample();
        assert_eq!(check(p, p, None, Some(5), true), Err(Refusal::Confirmation));
        assert_eq!(check(p, p, Some(Preview { snapshot: 19, ..p }), Some(5), true), Err(Refusal::Confirmation));
    }
    #[test]
    fn durable_matching_preimage_required() {
        let p = sample();
        for preimage in [None, Some(4)] {
            assert_eq!(check(p, p, Some(p), preimage, true), Err(Refusal::Preimage));
        }
    }
    #[test]
    fn no_apply_during_editor_macro_execution_or_transition() {
        let p = sample();
        assert_eq!(check(p, p, Some(p), Some(5), false), Err(Refusal::Busy));
    }
    #[test]
    fn repeated_apply_conflicts_after_revision_increment() {
        let p = sample();
        assert_eq!(valid(p, p), Ok(()));
        assert_eq!(valid(p, Preview { local_revision: 6, ..p }), Err(Refusal::Changed));
    }
}
