//! The words Gothic 3 makes clip names of. Script.dll composes the name of the clip it plays from the character's
//! state (0x1002f7b0, `sprintf("%s_%s_%s_%s_%s_%s_%s_%s_%s_%s_", …)`, the variant as `%02d` or `00`):
//!
//! ```text
//! <actor>_<stance>_<left>_<right>_<pose>_<action>_<phase>_<N|O|I>_<direction>_<variant>_   then the clip's own:  %_<n>_<P>_<ms>
//!  rig     STANCE   USE    USE     POSE   ACTION   PHASE   NIO     DIRECTION   00..99
//! ```
//! Each word is an entry of a table the game fills from string literals at start-up (Game.dll exports, Script.dll
//! arrays; read from their init code: `push <literal>; mov ecx, <slot>`). A name with a word outside them is never
//! composed, so the game never plays it. Every clip the game ships is named from these tables (test below).
use anyhow::{bail, Result};

/// Stances: Game.dll `g_pstrAniStateString` (gEAniState, RVA 0x7cc7c8).
pub const STANCE: &[&str] = &[
    "Dummy0", "Dummy1", "Stand", "Sneak", "Parade", "Kneel", "SitGround", "SitStool", "SitBench", "SitThrone",
    "SleepBed", "SleepGround", "TiltOrcBoulder", "HoldOrcBoulder", "LiftOrcBoulder", "SitKnockDown", "LieKnockDown",
    "LieKnockOut", "LieStraightDead", "LieDead", "LiePiercedKO", "LiePiercedDead", "TalkStand", "TalkSitGround",
    "TalkSitStool", "TalkSitBench", "TalkSitThrone", "Wade", "Swim", "Dive",
];
/// What a hand holds (left, right): Game.dll `g_ppszUseTypes` (gEUseType, RVA 0x6aafd0), items and interactions alike.
pub const USE: &[&str] = &[
    "None", "Action", "1H", "2H", "Arrow", "Bow", "CrossBow", "Bolt", "Fist", "Shield", "Armor", "Helmet", "Staff",
    "Amulet", "Ring", "Cast", "Potion", "Bread", "Meat", "Fruit", "Bread", "Potion", "Cup", "Bowl", "Torch",
    "Alarmhorn", "Staff", "Block", "Lute", "Staff", "TrophyTeeth", "Valuable", "Smoke", "OrcPipe", "Tool", "Tool",
    "Staff", "Tool", "Staff", "Tool", "Tool", "TrophySkin", "Letter", "Letter", "Letter", "Key", "Key", "CarryFront",
    "CarryShoulder", "2H", "TrophyFur", "Staff", "2H", "ITEM_E", "Modify", "Fist", "ITEM_H", "Anvil", "Forge",
    "GrindStone", "Cauldron", "Barbecue", "Alchemy", "Bookshelf", "Bookstand", "TakeStone", "DropStone", "PickOre",
    "PickGround", "DigGround", "Field", "Repair", "SawLog", "Lumberjack", "Bed", "SleepGround", "CleanFloor",
    "Dance", "FanBoss", "Boss", "Throne", "Pace", "Bard", "Stool", "Bench", "Waterpipe", "WaterBarrel", "Stove",
    "Campfire", "SitCampfire", "SitGround", "Smalltalk", "Preach", "Spectator", "Stand", "Guard", "Trader",
    "Listener", "OrcDance", "Stoneplate", "OrcDrum", "Door", "OrcBoulder", "EatGround", "DrinkWater", "Pee", "Chest",
    "Shrine", "AttackPoint", "Roam", "BODY_A", "Beard", "Hair", "Head", "Body", "Flee", "Talk",
];
/// Poses: Game.dll `g_pstrPoseString` (RVA 0x7cc758, up to `g_pstrFaceAni` at 0x7cc7bc).
pub const POSE: &[&str] = &[
    "None", "P0", "P1", "P2", "P3", "P00", "P01", "P02", "P03", "P10", "P11", "P12", "P13", "P20", "P21", "P22",
    "P23", "P30", "P31", "P32", "P33", "P40", "P50", "P60", "P70",
];
/// Actions: Game.dll `g_pstrActionString` (gEAction, RVA 0x7cc4c0).
pub const ACTION: &[&str] = &[
    "None", "Attack", "PowerAttack", "QuickAttack", "QuickAttackR", "QuickAttackL", "WhirlAttack", "WhirlAttack",
    "PierceAttack", "JumpAttack", "RamAttack", "FinishingAttack", "FinishingAttack", "Parade", "ParadeR", "ParadeL",
    "ExitParade", "QuickParadeStumble", "ParadeStumble", "ParadeStumbleR", "ParadeStumbleL", "HeavyParadeStumble",
    "QuickStumble", "Stumble", "StumbleR", "StumbleL", "SitKnockDown", "GetUpAttack", "GetUpParade", "LieKnockDown",
    "LieKnockOut", "PierceStumble", "Die", "LieDead", "LiePiercedKO", "LiePiercedDead", "AbortAttack", "Aim",
    "Shoot", "Reload", "Cock", "Cast", "PowerCast", "MagicParade", "QuickCast", "Summon", "Heal", "Wait", "JumpBack",
    "Fwd", "Back", "Left", "Right", "Move", "Jump", "Evade", "Slide", "Fall", "Dive", "", "Use", "Equip", "Reach",
    "Take", "Drop", "Hold", "HoldLeft", "HoldRight", "Transfer", "FlameSword", "Fry", "Eat", "Drink", "Stand",
    "Sneak", "Kneel", "SitGround", "SitStool", "SitBench", "SitThrone", "SleepBed", "SleepGround", "OrcDrum",
    "Waterpipe", "Anvil", "OrcAnvil", "Forge", "CoolWeapon", "DrinkWaterBarrel", "GrindStone", "Cauldron",
    "Barbecue", "Alchemy", "Bookstand", "Stoneplate", "PickOre", "OpenChest", "Lockpick", "OpenDoor", "ExitInteract",
    "PickGround", "DigGround", "Repair", "SawLog", "Lumberjack", "RakeField", "CleanFloor", "TiltOrcBoulder",
    "HoldOrcBoulder", "LiftOrcBoulder", "Fan", "Dance", "OrcDance", "FieldWork", "EatGround", "DrinkWater",
    "ArmsCrossed", "HandsOnHips", "Warn", "Ambient", "Think", "Pee", "Say", "Listen", "WatchFight", "Cheer",
    "SuperCheer", "Applaud", "QuickUse0", "QuickUse1", "QuickUse2", "QuickUse3", "QuickUse4", "QuickUse5",
    "QuickUse6", "QuickUse7", "QuickUse8", "QuickUse9",
];
/// Phases: Game.dll `g_pstrPhaseString` (gEPhase, RVA 0x7cc6e8).
pub const PHASE: &[&str] = &[
    "Raise", "Hit", "Aim", "Recover", "Begin", "Loop", "End", "Full", "Walk", "Run", "Sprint", "Far", "Stand",
    "StrafeL", "StrafeR", "Turn90", "Ambient", "Proud", "Approve", "Thoughtful", "Angry", "Threaten", "Nervous",
    "Point", "Preach", "Beg", "Sad",
];
/// The 8th word: Script.dll array at 0x100b3a80, filled at start-up from "N", "O", "I".
pub const NIO: &[&str] = &[
    "N", "O", "I",
];
/// Directions: Script.dll array at 0x100b3a8c; aiming adds `Up` and `Down` (Script.dll literals `Up_00_`, `Down_00_`).
pub const DIRECTION: &[&str] = &[
    "None", "Fwd", "Back", "Left", "Right", "FwdLeft", "FwdRight", "BackLeft", "BackRight", "Up", "Down",
];

/// The tables in field order, with what the add-on calls them.
pub const FIELDS: [(&str, &[&str]); 8] = [("stance", STANCE), ("left", USE), ("right", USE), ("pose", POSE), ("action", ACTION), ("phase", PHASE), ("nio", NIO), ("direction", DIRECTION)];

/// Each table's words, once each, for the add-on's lists.
pub fn words() -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (name, t) in FIELDS {
        let mut v: Vec<&str> = vec![];
        for w in t.iter().filter(|w| !w.is_empty()) { if !v.contains(w) { v.push(w); } }
        m.insert(name.to_string(), serde_json::json!(v));
    }
    serde_json::Value::Object(m)
}

/// A clip name's words: actor, the eight table words, the variant, and the clip's own part from `%`.
pub fn split(name: &str) -> Option<(String, [String; 8], u32, String)> {
    let (pre, v, tail) = crate::motion_write::variant_of(name)?;
    let p: Vec<&str> = pre.split('_').collect();
    if p.len() != 9 { return None; }
    Some((p[0].to_string(), std::array::from_fn(|i| p[i + 1].to_string()), v, tail))
}

/// The name the game would compose from these words (each checked against its table) with the clip's own part `tail`.
pub fn compose(actor: &str, w: &[String; 8], variant: u32, tail: &str) -> Result<String> {
    if actor.is_empty() || !actor.chars().all(|c| c.is_ascii_alphanumeric()) { bail!("actor word {actor:?}"); }
    for ((name, t), x) in FIELDS.iter().zip(w) {
        if x.is_empty() || !t.iter().any(|y| y.eq_ignore_ascii_case(x)) { bail!("{name} {x:?} is not a word the game composes"); }
    }
    if variant > 99 { bail!("variant {variant}"); }
    if !tail.starts_with("%_") || !tail[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { bail!("clip part {tail:?}"); }
    Ok(format!("{actor}_{}_{variant:02}_{tail}", w.join("_")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every clip of the game named the composed way uses only words of these tables (actor aside; the game finds files
    /// whatever their case: `BookStand`, `LockPick`), except about a hundred whose words no code of the game holds (no
    /// literal in Game.dll, Script.dll or Engine.dll): the composer never makes those names. Some are named whole
    /// elsewhere (the seagull's flight in its template); the rest the game does not play.
    #[test]
    fn every_clip_of_the_game_is_named_from_the_tables() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let (mut n, mut bad) = (0, vec![]);
        for k in g.keys().into_iter().filter(|k| k.starts_with("_compiledanimation/") && k.ends_with(".xmot")) {
            let name = crate::stem_of(g.path_of(&k).unwrap());
            let Some((actor, w, v, tail)) = split(&name) else { continue };
            n += 1;
            match compose(&actor, &w, v, &tail) { Ok(c) if c == name => {}, Ok(c) => bad.push(format!("{name} -> {c}")), Err(e) => bad.push(format!("{name}: {e:#}")) }
        }
        assert!(n > 5000, "{n}");
        let mut words: std::collections::BTreeMap<String, usize> = Default::default();
        for b in &bad { if let Some(w) = b.split(": ").nth(1) { *words.entry(w.split(" is ").next().unwrap().to_string()).or_default() += 1; } }
        eprintln!("missing words: {words:?}");
        let never: std::collections::BTreeMap<String, usize> = [("action \"Fly\"", 1), ("action \"Glide\"", 1), ("action \"LightStumble\"", 52), ("action \"Lute\"", 2), ("action \"Manipulate\"", 8), ("action \"Perching\"", 1), ("action \"StartFly\"", 1), ("action \"SummonStrong\"", 8), ("action \"ThatHurt\"", 2), ("action \"Transform\"", 6), ("action \"TwirlAttack\"", 7), ("phase \"PowerRaise\"", 3), ("phase \"Sneak\"", 1), ("pose \"P61\"", 1), ("stance \"LietDead\"", 1)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(words, never, "{} of {n}", bad.len());
    }

    /// The stance, hand and action tables are as long as the game's own enums: Game.dll names every value of
    /// `gEAniState`, `gEUseType` and `gEAction` (`gEAniState_Stand`, …, `…_Count`), and the tables have one word per value
    /// (the same name, or the animation word the game puts for it: `Plant` → `Bread`, `Broom` → `Staff`).
    #[test]
    fn the_tables_are_as_long_as_the_games_enums() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let dll = std::fs::read(std::path::Path::new(&root).join("Game.dll")).unwrap();
        let names = |prefix: &str| -> Vec<String> {
            dll.split(|&b| b == 0).filter_map(|s| std::str::from_utf8(s).ok()).filter_map(|s| s.strip_prefix(prefix)).filter(|s| *s != "Count" && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).map(String::from).collect()
        };
        for (prefix, table, same) in [("gEAniState_", STANCE, 30), ("gEUseType_", USE, 97), ("gEAction_", ACTION, 135)] {
            let e = names(prefix);
            assert_eq!(e.len(), table.len(), "{prefix}");
            assert_eq!(e.iter().zip(table).filter(|(a, b)| a.eq_ignore_ascii_case(b)).count(), same, "{prefix}");
        }
    }

    /// A clip belongs to the actor its name's first word names: Engine.dll `eCWrapper_emfx2Motion::CleanUpMotions`
    /// ("Can't extract actor prefix from name %s!") cuts the name at its first `_` and takes `G3_<prefix>_Skeleton.FXA`,
    /// else `G3_<prefix>_Body_01.FXA`. Every clip of the game but the few named otherwise finds its actor so.
    #[test]
    fn every_clip_finds_its_actor_by_its_first_word() {
        let Ok(root) = std::env::var("G3_GAME") else { return };
        let g = crate::g3::G3Ctx::open_original(std::path::Path::new(&root)).unwrap();
        let (mut found, mut lost) = (0, std::collections::BTreeMap::<String, usize>::new());
        for k in g.keys().into_iter().filter(|k| k.starts_with("_compiledanimation/") && k.ends_with(".xmot")) {
            let name = crate::stem_of(g.path_of(&k).unwrap());
            let p = name.split('_').next().unwrap();
            let has = |end: &str| g.find("_compiledanimation", &format!("G3_{p}_{end}.xact")).is_some();
            if has("Skeleton") || has("Body_01") { found += 1 } else { *lost.entry(p.to_string()).or_default() += 1; }
        }
        eprintln!("{found} clips find their actor; not: {lost:?}");
        assert!(found > 5000 && lost.values().sum::<usize>() < 200, "{lost:?}");
    }

    #[test]
    fn names_are_composed_only_from_the_words() {
        let w = ["Stand", "None", "None", "P0", "Ambient", "Loop", "N", "Fwd"].map(String::from);
        assert_eq!(compose("Wolf", &w, 6, "%_00_P0_0").unwrap(), "Wolf_Stand_None_None_P0_Ambient_Loop_N_Fwd_06_%_00_P0_0");
        let mut x = w.clone();
        x[4] = "Breakdance".into();
        assert!(compose("Wolf", &x, 6, "%_00_P0_0").is_err());
    }
}
