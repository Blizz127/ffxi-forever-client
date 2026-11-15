//! Pure comparison core for the queued Forever gear feature. Not yet wired to
//! UI or equip commands. Adapters must establish complete retail/instance facts;
//! an incomplete description, augment or conditional modifier requires review.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stat {
    Defense,
    Hp,
    Mp,
    Strength,
    Dexterity,
    Vitality,
    Agility,
    Intelligence,
    Mind,
    Charisma,
    Attack,
    Accuracy,
    Evasion,
    Enmity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Physical,
    Caster,
    Tank,
    ReviewOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Armor,
    Shield { size: u16 },
    Weapon { skill: u8, delay: u16, damage: u16 },
}

#[derive(Clone, Debug)]
pub struct Facts {
    pub job_mask: u32,
    pub race_mask: u16,
    pub level: u16,
    pub slots: u16,
    pub superior: u8,
    pub family: Family,
    /// False for unknown augments, unparsed/conditional effects or restrictions.
    pub complete: bool,
    pub stats: BTreeMap<Stat, i32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Wearer {
    pub job: u8,
    pub level: u16,
    pub race: u8,
    pub superior: u8,
    pub slot: u8,
    pub encumbered: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    Ineligible(&'static str),
    Review(&'static str),
    Equivalent,
    /// Higher modeled stats for the explicitly selected role, not a universal score.
    Improvement(Vec<String>),
    Tradeoff(Vec<String>),
    Lower(Vec<String>),
}

/// Recognize only complete, plain numeric retail description fields. Any
/// remaining text, percentage, conditional, duplicate, or conflicting weapon
/// header makes the whole comparison unknown. Instance/augment validity is a
/// separate required input; this parser alone cannot establish Facts::complete.
pub fn plain_stats(mut text: &str, family: Family) -> Option<BTreeMap<Stat, i32>> {
    let mut out = BTreeMap::new();
    let (mut damage, mut delay, mut fields) = (false, false, 0);
    while !text.trim().is_empty() {
        text = text.trim_start();
        let keys = [
            ("DEF", Some(Stat::Defense)),
            ("HP", Some(Stat::Hp)),
            ("MP", Some(Stat::Mp)),
            ("STR", Some(Stat::Strength)),
            ("DEX", Some(Stat::Dexterity)),
            ("VIT", Some(Stat::Vitality)),
            ("AGI", Some(Stat::Agility)),
            ("INT", Some(Stat::Intelligence)),
            ("MND", Some(Stat::Mind)),
            ("CHR", Some(Stat::Charisma)),
            ("Attack", Some(Stat::Attack)),
            ("Accuracy", Some(Stat::Accuracy)),
            ("Evasion", Some(Stat::Evasion)),
            ("Enmity", Some(Stat::Enmity)),
            ("DMG", None),
            ("Delay", None),
        ];
        let (name, stat) = keys.into_iter().find(|(key, _)| text.starts_with(key))?;
        text = text.strip_prefix(name)?.trim_start();
        let colon = text.starts_with(':');
        if colon {
            text = text.strip_prefix(':')?.trim_start();
        }
        if !colon && !text.starts_with(['+', '-']) {
            return None;
        }
        let sign = usize::from(text.starts_with(['+', '-']));
        let len = text.as_bytes().iter().skip(sign).take_while(|b| b.is_ascii_digit()).count() + sign;
        if len == sign {
            return None;
        }
        let value = text[..len].parse::<i32>().ok()?;
        text = &text[len..];
        if !text.is_empty() && !text.starts_with(char::is_whitespace) {
            return None;
        }
        if let Some(stat) = stat {
            if out.insert(stat, value).is_some() {
                return None;
            }
        } else {
            let Family::Weapon { damage: d, delay: t, .. } = family else {
                return None;
            };
            match name {
                "DMG" if !damage && value == i32::from(d) => damage = true,
                "Delay" if !delay && value == i32::from(t) => delay = true,
                _ => return None,
            }
        }
        fields += 1;
    }
    if fields == 0 || (matches!(family, Family::Weapon { .. }) && !(damage && delay)) {
        return None;
    }
    Some(out)
}

pub fn compare(current: &Facts, candidate: &Facts, who: Wearer, role: Role) -> Comparison {
    let Some(slot) = 1u16.checked_shl(who.slot.into()) else {
        return Comparison::Ineligible("Unknown slot");
    };
    if candidate.slots & slot == 0 || who.encumbered & candidate.slots != 0 {
        return Comparison::Ineligible("Slot unavailable");
    }
    if candidate.level > who.level || candidate.superior > who.superior {
        return Comparison::Ineligible("Level requirement");
    }
    if candidate.job_mask & 1u32.checked_shl(who.job.into()).unwrap_or(0) == 0 {
        return Comparison::Ineligible("Job requirement");
    }
    if candidate.race_mask & 1u16.checked_shl(who.race.into()).unwrap_or(0) == 0 {
        return Comparison::Ineligible("Race requirement");
    }
    if !current.complete || !candidate.complete {
        return Comparison::Review("Unknown modifiers, augments or restrictions");
    }
    if role == Role::ReviewOnly {
        return Comparison::Review("Choose a comparison role");
    }
    if current.slots != candidate.slots {
        return Comparison::Review("Different slot coverage");
    }
    let (mut gains, mut losses, mut relevant) = (false, false, false);
    let mut changes = Vec::new();
    match (current.family, candidate.family) {
        (Family::Armor, Family::Armor) => {}
        (Family::Shield { size: a }, Family::Shield { size: b }) if a == b => {}
        (Family::Weapon { skill: a, delay: ad, damage: av }, Family::Weapon { skill: b, delay: bd, damage: bv })
            if a == b && ad == bd =>
        {
            // Delay changes affect TP/weapon skills; never reduce this to damage/second.
            if av != bv {
                gains |= bv > av;
                losses |= bv < av;
                relevant |= matches!(role, Role::Physical | Role::Tank) && bv > av;
                changes.push(format!("Damage: {av} -> {bv}"));
            }
        }
        _ => {
            return Comparison::Review("Different weapon skill, delay, shield type or equipment family");
        }
    }
    for stat in [
        Stat::Defense,
        Stat::Hp,
        Stat::Mp,
        Stat::Strength,
        Stat::Dexterity,
        Stat::Vitality,
        Stat::Agility,
        Stat::Intelligence,
        Stat::Mind,
        Stat::Charisma,
        Stat::Attack,
        Stat::Accuracy,
        Stat::Evasion,
        Stat::Enmity,
    ] {
        let a = current.stats.get(&stat).copied().unwrap_or(0);
        let b = candidate.stats.get(&stat).copied().unwrap_or(0);
        if a == b {
            continue;
        }
        if stat == Stat::Enmity && role == Role::Physical {
            return Comparison::Review("Enmity preference is not established for this role");
        }
        let higher = if stat == Stat::Enmity && role == Role::Caster { b < a } else { b > a };
        gains |= higher;
        losses |= !higher;
        relevant |= higher
            && match stat {
                Stat::Defense | Stat::Hp | Stat::Vitality | Stat::Agility | Stat::Evasion => true,
                Stat::Strength | Stat::Dexterity | Stat::Attack | Stat::Accuracy => {
                    matches!(role, Role::Physical | Role::Tank)
                }
                Stat::Mp | Stat::Intelligence | Stat::Mind | Stat::Charisma => role == Role::Caster,
                Stat::Enmity => true,
            };
        changes.push(format!("{stat:?}: {a} -> {b}"));
    }
    match (gains, losses, relevant) {
        (false, false, _) => Comparison::Equivalent,
        (true, false, true) => Comparison::Improvement(changes),
        (false, true, _) => Comparison::Lower(changes),
        _ => Comparison::Tradeoff(changes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gear(stats: &[(Stat, i32)]) -> Facts {
        Facts {
            job_mask: 1 << 1 | 1 << 3,
            race_mask: 1 << 1,
            level: 1,
            slots: 1 << 5,
            superior: 0,
            family: Family::Armor,
            complete: true,
            stats: stats.iter().copied().collect(),
        }
    }
    fn wearer() -> Wearer {
        Wearer { job: 1, level: 40, race: 1, superior: 0, slot: 5, encumbered: 0 }
    }
    #[test]
    fn eligibility_precedes_apparent_stats() {
        let a = gear(&[]);
        let mut b = gear(&[(Stat::Defense, 100)]);
        b.level = 41;
        assert_eq!(compare(&a, &b, wearer(), Role::Tank), Comparison::Ineligible("Level requirement"));
        b.level = 1;
        b.job_mask = 1 << 3;
        assert_eq!(compare(&a, &b, wearer(), Role::Tank), Comparison::Ineligible("Job requirement"));
        b.job_mask = 1 << 1;
        b.race_mask = 1 << 2;
        assert_eq!(compare(&a, &b, wearer(), Role::Tank), Comparison::Ineligible("Race requirement"));
        b.race_mask = 1 << 1;
        assert_eq!(
            compare(&a, &b, Wearer { encumbered: 1 << 5, ..wearer() }, Role::Tank),
            Comparison::Ineligible("Slot unavailable")
        );
        b.slots |= 1 << 4;
        assert_eq!(
            compare(&a, &b, Wearer { encumbered: 1 << 4, ..wearer() }, Role::Tank),
            Comparison::Ineligible("Slot unavailable")
        );
    }
    #[test]
    fn level_alone_is_not_quality() {
        let a = gear(&[(Stat::Defense, 10)]);
        let mut b = a.clone();
        b.level = 30;
        assert_eq!(compare(&a, &b, wearer(), Role::Tank), Comparison::Equivalent);
    }
    #[test]
    fn stat_loss_is_a_tradeoff_even_with_more_defense() {
        let a = gear(&[(Stat::Defense, 10), (Stat::Mp, 20)]);
        let b = gear(&[(Stat::Defense, 12), (Stat::Mp, 10)]);
        assert!(matches!(compare(&a, &b, wearer(), Role::Caster), Comparison::Tradeoff(_)));
        assert!(matches!(compare(&a, &b, wearer(), Role::Tank), Comparison::Tradeoff(_)));
    }
    #[test]
    fn gains_must_fit_explicit_role() {
        let a = gear(&[]);
        let b = gear(&[(Stat::Intelligence, 2)]);
        assert!(matches!(compare(&a, &b, wearer(), Role::Caster), Comparison::Improvement(_)));
        assert!(matches!(compare(&a, &b, wearer(), Role::Physical), Comparison::Tradeoff(_)));
        assert!(matches!(compare(&a, &b, wearer(), Role::ReviewOnly), Comparison::Review(_)));
    }
    #[test]
    fn enmity_is_role_dependent() {
        let a = gear(&[]);
        let b = gear(&[(Stat::Enmity, 2)]);
        assert!(matches!(compare(&a, &b, wearer(), Role::Tank), Comparison::Improvement(_)));
        assert!(matches!(compare(&a, &b, wearer(), Role::Caster), Comparison::Lower(_)));
        assert!(matches!(compare(&a, &b, wearer(), Role::Physical), Comparison::Review(_)));
    }
    #[test]
    fn unknown_instance_and_multi_slot_need_review() {
        let a = gear(&[]);
        let mut b = gear(&[(Stat::Defense, 20)]);
        b.complete = false;
        assert!(matches!(compare(&a, &b, wearer(), Role::Tank), Comparison::Review(_)));
        b.complete = true;
        b.slots |= 1 << 4;
        assert!(matches!(compare(&a, &b, wearer(), Role::Tank), Comparison::Review(_)));
    }
    #[test]
    fn weapons_do_not_use_universal_dps_or_switch_skill() {
        let mut a = gear(&[]);
        a.family = Family::Weapon { skill: 3, delay: 231, damage: 6 };
        let mut b = a.clone();
        b.family = Family::Weapon { skill: 3, delay: 231, damage: 14 };
        assert!(matches!(compare(&a, &b, wearer(), Role::Physical), Comparison::Improvement(_)));
        b.family = Family::Weapon { skill: 3, delay: 200, damage: 14 };
        assert!(matches!(compare(&a, &b, wearer(), Role::Physical), Comparison::Review(_)));
        b.family = Family::Weapon { skill: 2, delay: 231, damage: 14 };
        assert!(matches!(compare(&a, &b, wearer(), Role::Physical), Comparison::Review(_)));
    }
    #[test]
    fn plain_description_fields_are_whole_and_signed() {
        let stats = plain_stats("DEF:32\nHP+10 MP-2 STR +1", Family::Armor).unwrap();
        assert_eq!(stats.get(&Stat::Defense), Some(&32));
        assert_eq!(stats.get(&Stat::Mp), Some(&-2));
        assert_eq!(stats.len(), 4);
    }
    #[test]
    fn unknown_conditional_percentage_or_duplicate_never_partially_scores() {
        for description in [
            "",
            "DEF:32 Latent effect: STR+3",
            "HP+10%",
            "HP+1 HP+2",
            "MP+5 Enhances cure potency",
            "DEF:32 Additional effect",
            "HP+999999999999999999999",
        ] {
            assert_eq!(plain_stats(description, Family::Armor), None, "{description}");
        }
    }
    #[test]
    fn weapon_description_must_match_structured_retail_fields() {
        let weapon = Family::Weapon { skill: 3, delay: 231, damage: 6 };
        assert_eq!(plain_stats("DMG:6 Delay:231", weapon), Some(BTreeMap::new()));
        for description in ["DMG:14 Delay:231", "DMG:6 Delay:200", "STR+1", "DMG:6", "DMG:6 Delay:231 DMG:6"] {
            assert_eq!(plain_stats(description, weapon), None);
        }
    }
}
