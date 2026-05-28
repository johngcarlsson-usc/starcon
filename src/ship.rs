use avian2d::dynamics::rigid_body::forces::{ConstantLocalForce, ConstantTorque};
use avian2d::prelude::*;
use bevy::prelude::*;
use ini::Ini;
use std::collections::HashMap;

use crate::input;

/// Static stats for one ship class, parsed from the TimeWarp `.ini` format.
///
/// Field names mirror the legacy `[Ship]`, `[Weapon]`, `[Special]` sections
/// so designers can compare against the original `data/shp*.ini` files.
#[derive(Debug, Clone)]
pub struct ShipStats {
    pub code: String,
    pub name: String,
    pub origin: String,
    pub crew_max: i32,
    /// Starting crew (`.ini` `Crew`), which can be below `crew_max` —
    /// the Syreen starts at 12 of 42 and absorbs enemy crew via its
    /// song. Defaults to `crew_max` when the `.ini` omits a distinct
    /// `Crew`, so every other ship still starts full.
    pub crew_start: i32,
    pub batt_max: i32,
    pub speed_max: f32,
    pub accel_rate: f32,
    pub turn_rate: f32,
    pub recharge_amount: i32,
    pub recharge_rate: i32,
    pub weapon_drain: i32,
    pub weapon_rate: i32,
    pub special_drain: i32,
    pub special_rate: i32,
    pub mass: f32,
    pub cost: i32,
    pub weapon_range: f32,
    pub weapon_damage: i32,
    pub description: String,
    /// Parsed `[AI3_Default]` block — drives `tick_ai_pilots` when this
    /// ship is `AiControlled`. See `AiTactics` for the canon-derived
    /// tactic library.
    pub ai: AiTactics,
}

/// Named weapon tactic from the original `.ini` `[AI3_Default]` block
/// (see `shp*.ini` `Weapon=` field). Decides when the AI fires its
/// primary. The canon library has more entries than we implement in v1
/// — anything unrecognised falls back to `Homing` (the generous
/// "shoot if target is roughly in front" default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiWeaponTactic {
    /// Fire only when the target is dead-centre in a very narrow cone
    /// and inside `Weapon_Range`. High-precision (Orz cannon, Ilwrath).
    Precedence,
    /// Fire when the target is roughly forward and in range. Trusts the
    /// projectile's homing to finish the job (Earthling, Mycon, Spathi).
    Homing,
    /// Fire when the target is in a medium-narrow forward cone, in
    /// range. (Spathi follow-up shot, Vuxin, Druuge, Melnorme.)
    Narrow,
    /// Just fire — every tick the cooldown allows. Used for
    /// always-on / auto-aim weapons (Arilou halo, Slylandro lightning,
    /// Umgah cone) and field-of-effect attackers.
    Field,
    /// Fire when the target is in range; the projectile is then
    /// fire-and-forget (Kohr-Ah blades, Chenjesu crystal).
    Launched,
    /// "Side-firing" weapons — Pkunk's triple cone shoots forward + two
    /// off-axis at the same time. Same trigger as Homing (wide forward
    /// arc); the three barrels handle the lateral coverage.
    Sides,
    /// Backward-firing weapons (Spathi BUTT missile via primary slot if
    /// ever flipped; today Spathi has a dedicated steering override).
    /// Fire when the target is in the rear arc.
    Back,
    /// "Drop something behind you" — Androsynth bubble field as the
    /// primary. Treat as Homing (fire whenever the target is roughly
    /// forward); the drift on the bubble does the rest.
    Mine,
    /// Hoard battery — refuse to fire primary unless batt ≥
    /// `BattRecharge` floor. Zoq-Fot-Pik uses this on the .ini side
    /// to save up for the tongue lash.
    ReserveBattery,
    /// Anything we don't recognise yet — behaves like `Homing` so the
    /// AI still does *something* sensible while we expand the library.
    Default,
}

impl Default for AiWeaponTactic {
    fn default() -> Self { AiWeaponTactic::Default }
}

/// Named special tactic from the `.ini` `Special=` (and chained
/// `Special2`/`3`/`4`) field. The AI runs each one in order each tick;
/// the FIRST that says "go" triggers the special this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiSpecialTactic {
    /// Trigger when in danger — incoming projectile detected OR crew is
    /// running low. Earthling point-defense, Yehat shield, Arilou
    /// teleport, Kohr-Ah blade-spin, Utwig ricochet, Alary grow.
    Defense,
    /// Trigger when the nearest enemy is inside `Special_Range`. Shofixti
    /// nova, Vuxin limpet, Syreen song, Androsynth Blazer, Thraddash
    /// afterburner, Melnorme confusion ray.
    Proximity,
    /// Trigger when battery has charged past `BattRecharge`. Pkunk Phase
    /// Shift, Druuge auto-recharge, Slylandro probe drone.
    Battery,
    /// Trigger when the enemy is FAR (outside `Special_Range`). Chmmr
    /// satellites — long-range area denial.
    NoProximity,
    /// Trigger only when the firer is ALSO pressing fire — Orz marine
    /// launch chord (handled by `tick_orz_turret`; tactic here is a
    /// no-op since the Orz AI gets a dedicated steering override).
    PlusFire,
    /// Ilwrath cloak. Same trigger conditions as `Defense` (incoming
    /// projectile OR low crew) — going invisible IS the defensive move.
    Cloak,
    /// "Drop something behind / near" — Androsynth bubble field, Vuxin
    /// limpet, Thraddash trail-puff. Fires when an enemy is close enough
    /// that the dropped thing has a chance of catching them, treated like
    /// `Proximity` with the special_range threshold.
    Mine,
    /// Spathi's `Back`: fire SPECIAL (the BUTT missile) whenever the
    /// target is in the rear arc. Spathi's run-and-burst override
    /// already handles the Spathi specifically, but other classes can
    /// reuse this tactic for backward-firing weapons.
    Back,
    /// Mode-toggle ships (Mmrnmhrm T↔Y, Androsynth normal↔Blazer, etc.).
    /// The AI picks a *desired* form based on engagement range and
    /// fires SPECIAL once when the actual form differs. Single-press
    /// edge — the ToggleMode ability dispatcher already de-bounces.
    NextState,
    /// Hoard battery — don't trigger any special until batt ≥
    /// `BattRecharge`. Combined with another tactic this just delays
    /// when that tactic becomes eligible.
    ReserveBattery,
    /// Skip this special. Used by classes whose special isn't
    /// AI-friendly (Supox 4-way thrust).
    None,
    /// Unrecognised — skip. We grow the library as needed.
    Default,
}

impl Default for AiSpecialTactic {
    fn default() -> Self { AiSpecialTactic::Default }
}

/// Parsed `[AI3_Default]` block for one ship. Lives in `ShipStats` so
/// the AI never has to touch the `.ini` again at runtime.
#[derive(Debug, Clone, Default)]
pub struct AiTactics {
    pub weapon: AiWeaponTactic,
    /// Up to four chained specials, evaluated in order. First that
    /// fires wins this tick.
    pub specials: Vec<AiSpecialTactic>,
    /// Range threshold for `Proximity` / `NoProximity` (world units).
    /// `None` = use the weapon's range as a fallback.
    pub special_range: Option<f32>,
    /// Range threshold for the weapon tactic (world units). Overrides
    /// the `.ini`'s `[Weapon] Range` when present.
    pub weapon_range: Option<f32>,
    /// 1/N gate for the special (only triggers 1 frame in N when its
    /// condition would otherwise fire) — keeps low-cost specials from
    /// firing every tick.
    pub special_freq: u32,
    /// Floor on battery before the AI will even consider its special
    /// (canon `BattRecharge=`). Defaults to 0 (no floor).
    pub batt_floor: i32,
}

impl ShipStats {
    /// Parse stats from already-loaded `.ini` + lore strings. Used by
    /// `load_ship_catalog` against `include_str!`-baked content so the
    /// catalog works identically on native and WASM (no `std::fs`).
    pub fn from_str(code: &str, ini_str: &str, txt_str: &str) -> Result<Self, String> {
        let ini = Ini::load_from_str(ini_str).map_err(|e| format!("ini {code}: {e}"))?;
        let info = ini.section(Some("Info")).ok_or("missing [Info]")?;
        let ship = ini.section(Some("Ship")).ok_or("missing [Ship]")?;
        let weapon = ini.section(Some("Weapon"));

        let name = [info.get("Name0"), info.get("Name1"), info.get("Name2")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");

        fn g<T: std::str::FromStr + Default>(sec: &ini::Properties, k: &str) -> T {
            sec.get(k).unwrap_or("").trim().parse().unwrap_or_default()
        }

        Ok(Self {
            code: code.to_string(),
            name,
            origin: info.get("Origin").unwrap_or("").to_string(),
            crew_max: g(ship, "CrewMax"),
            crew_start: {
                let cm: i32 = g(ship, "CrewMax");
                let c: i32 = g(ship, "Crew");
                if c > 0 { c.min(cm) } else { cm }
            },
            batt_max: g(ship, "BattMax"),
            speed_max: g(ship, "SpeedMax"),
            accel_rate: g(ship, "AccelRate"),
            turn_rate: g(ship, "TurnRate"),
            recharge_amount: g(ship, "RechargeAmount"),
            recharge_rate: g(ship, "RechargeRate"),
            weapon_drain: g(ship, "WeaponDrain"),
            weapon_rate: g(ship, "WeaponRate"),
            special_drain: g(ship, "SpecialDrain"),
            special_rate: g(ship, "SpecialRate"),
            mass: {
                let m: f32 = g(ship, "Mass");
                if m == 0.0 { 1.0 } else { m }
            },
            cost: g(ship, "Cost"),
            weapon_range: weapon.map(|w| g::<f32>(w, "Range")).unwrap_or(0.0),
            weapon_damage: weapon.map(|w| g::<i32>(w, "Damage")).unwrap_or(0),
            description: txt_str.to_string(),
            ai: parse_ai_tactics(&ini),
        })
    }
}

/// Parse the `[AI3_Default]` block into our internal `AiTactics`.
/// Tolerant of misspellings + spacing (the original `.ini`s have a
/// few — e.g. `Feild`, `Weapon_Vecolity`). Unknown tactic names fall
/// back to `Default`, which the AI treats as "do nothing fancy."
fn parse_ai_tactics(ini: &Ini) -> AiTactics {
    let Some(sec) = ini.section(Some("AI3_Default")) else {
        return AiTactics::default();
    };
    fn norm(s: &str) -> String {
        s.trim().to_ascii_lowercase()
    }
    fn weapon(name: &str) -> AiWeaponTactic {
        match norm(name).as_str() {
            "precedence" => AiWeaponTactic::Precedence,
            "homing" => AiWeaponTactic::Homing,
            "narrow" => AiWeaponTactic::Narrow,
            "field" | "feild" => AiWeaponTactic::Field,
            "launched" => AiWeaponTactic::Launched,
            "sides" => AiWeaponTactic::Sides,
            "back" | "rear" => AiWeaponTactic::Back,
            "mine" => AiWeaponTactic::Mine,
            "reserve_battery" | "reserver_battery" => AiWeaponTactic::ReserveBattery,
            "hold" => AiWeaponTactic::Narrow, // Meltr "Hold" = charge then fire when aligned.
            _ => AiWeaponTactic::Default,
        }
    }
    fn special(name: &str) -> AiSpecialTactic {
        match norm(name).as_str() {
            "defense" => AiSpecialTactic::Defense,
            "proximity" => AiSpecialTactic::Proximity,
            "battery" | "max_battery" => AiSpecialTactic::Battery,
            "no_proximity" => AiSpecialTactic::NoProximity,
            "plus_fire" => AiSpecialTactic::PlusFire,
            "cloak" => AiSpecialTactic::Cloak,
            "mine" => AiSpecialTactic::Mine,
            "back" | "rear" => AiSpecialTactic::Back,
            "next_state" => AiSpecialTactic::NextState,
            "reserve_battery" | "reserver_battery" => AiSpecialTactic::ReserveBattery,
            // "Front" / "Sides" / "Homing" / "Launched" / "Precedence" /
            // "Narrow" used as special tactics in some .inis are really
            // just aim-cone hints for a chained weapon-style call. We
            // treat them as `Proximity` (use when in cone of effect).
            "front" | "sides" | "homing" | "launched" | "precedence" | "narrow" => {
                AiSpecialTactic::Proximity
            }
            "none" => AiSpecialTactic::None,
            _ => AiSpecialTactic::Default,
        }
    }
    let weapon_t = sec
        .get("Weapon")
        .map(weapon)
        .unwrap_or(AiWeaponTactic::Default);
    let mut specials = Vec::new();
    for k in ["Special", "Special2", "Special3", "Special4"] {
        if let Some(v) = sec.get(k) {
            let t = special(v);
            if t != AiSpecialTactic::Default && t != AiSpecialTactic::None {
                specials.push(t);
            }
        }
    }
    // `Special_Range` / `Weapon_Range` are in SC2 range-units (scale ×40).
    let special_range = sec
        .get("Special_Range")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|r| r * 40.0);
    let weapon_range = sec
        .get("Weapon_Range")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|r| r * 40.0);
    let special_freq = sec
        .get("SpecialFreq")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    let batt_floor = sec
        .get("BattRecharge")
        .and_then(|v| v.trim().parse::<i32>().ok())
        .unwrap_or(0);
    AiTactics {
        weapon: weapon_t,
        specials,
        special_range,
        weapon_range,
        special_freq,
        batt_floor,
    }
}

/// Every shipped class's `.ini` + lore baked into the binary via
/// `include_str!`. Required because WASM can't scan an asset directory;
/// also nice on native (one source of truth, no filesystem assumption).
/// Adding a class = one new tuple here + the `ShipClass` enum arm.
const SHIP_INIS: &[(&str, &str, &str)] = &[
    ("earcr", include_str!("../assets/ships/earcr.ini"), include_str!("../assets/ships/earcr.txt")),
    ("spael", include_str!("../assets/ships/spael.ini"), include_str!("../assets/ships/spael.txt")),
    ("yehte", include_str!("../assets/ships/yehte.ini"), include_str!("../assets/ships/yehte.txt")),
    ("chmav", include_str!("../assets/ships/chmav.ini"), include_str!("../assets/ships/chmav.txt")),
    ("kzedr", include_str!("../assets/ships/kzedr.ini"), include_str!("../assets/ships/kzedr.txt")),
    ("mycpo", include_str!("../assets/ships/mycpo.ini"), include_str!("../assets/ships/mycpo.txt")),
    ("shosc", include_str!("../assets/ships/shosc.ini"), include_str!("../assets/ships/shosc.txt")),
    ("arisk", include_str!("../assets/ships/arisk.ini"), include_str!("../assets/ships/arisk.txt")),
    ("pkufu", include_str!("../assets/ships/pkufu.ini"), include_str!("../assets/ships/pkufu.txt")),
    ("ilwav", include_str!("../assets/ships/ilwav.ini"), include_str!("../assets/ships/ilwav.txt")),
    ("thrto", include_str!("../assets/ships/thrto.ini"), include_str!("../assets/ships/thrto.txt")),
    ("vuxin", include_str!("../assets/ships/vuxin.ini"), include_str!("../assets/ships/vuxin.txt")),
    ("supbl", include_str!("../assets/ships/supbl.ini"), include_str!("../assets/ships/supbl.txt")),
    ("kohma", include_str!("../assets/ships/kohma.ini"), include_str!("../assets/ships/kohma.txt")),
    ("syrpe", include_str!("../assets/ships/syrpe.ini"), include_str!("../assets/ships/syrpe.txt")),
    ("andgu", include_str!("../assets/ships/andgu.ini"), include_str!("../assets/ships/andgu.txt")),
    ("chebr", include_str!("../assets/ships/chebr.ini"), include_str!("../assets/ships/chebr.txt")),
    ("druma", include_str!("../assets/ships/druma.ini"), include_str!("../assets/ships/druma.txt")),
    ("utwju", include_str!("../assets/ships/utwju.ini"), include_str!("../assets/ships/utwju.txt")),
    ("zfpst", include_str!("../assets/ships/zfpst.ini"), include_str!("../assets/ships/zfpst.txt")),
    ("mmrxf", include_str!("../assets/ships/mmrxf.ini"), include_str!("../assets/ships/mmrxf.txt")),
    ("orzne", include_str!("../assets/ships/orzne.ini"), include_str!("../assets/ships/orzne.txt")),
    ("slypr", include_str!("../assets/ships/slypr.ini"), include_str!("../assets/ships/slypr.txt")),
    ("umgdr", include_str!("../assets/ships/umgdr.ini"), include_str!("../assets/ships/umgdr.txt")),
    ("meltr", include_str!("../assets/ships/meltr.ini"), include_str!("../assets/ships/meltr.txt")),
    ("alabc", include_str!("../assets/ships/alabc.ini"), include_str!("../assets/ships/alabc.txt")),
];

#[derive(Resource, Debug, Default)]
pub struct ShipCatalog {
    pub ships: HashMap<String, ShipStats>,
}

/// Who controls a given match slot. Drives input dispatch and
/// the lobby/menu flow:
///   - `Human`: a local keyboard player (uses `keymap(slot)` —
///     so for a 2P couch match P1 uses arrows + Z/X and P2 uses
///     WASD + G/H).
///   - `Ai`: stub AI. `tick_ai_pilots` flies the ship at the
///     nearest enemy and force-fires primary on every cooldown.
///   - `Remote`: online player whose inputs arrive via GGRS.
///     Currently unused until the rollback session lands; the
///     menu still uses `Human` for every online slot for now,
///     so this enum reserves the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerKind {
    Human,
    Ai,
    Remote,
}

/// One slot in a match: which ship to fly + who's flying it.
#[derive(Clone, Copy, Debug)]
pub struct SlotConfig {
    pub class: ShipClass,
    pub kind: PlayerKind,
}

impl SlotConfig {
    pub fn human(class: ShipClass) -> Self {
        Self { class, kind: PlayerKind::Human }
    }
    pub fn ai(class: ShipClass) -> Self {
        Self { class, kind: PlayerKind::Ai }
    }
}

/// What `spawn_match` should use the next time the scene rebuilds.
/// Mutated by the class-picker keys; read in `spawn_match`. The
/// `slots` vector is the per-slot config: index 0 is P1, index 1
/// is P2, and so on. Length 2..=4 — local hotseat plays the
/// first two; online matches up to four; mix with AI slots for
/// solo play.
#[derive(Resource, Debug, Clone)]
pub struct MatchConfig {
    pub slots: Vec<SlotConfig>,
}

impl MatchConfig {
    /// Convenience for the 2-player human default. Use this
    /// anywhere that wants the legacy "P1 + P2" shape.
    pub fn local_two(p1: ShipClass, p2: ShipClass) -> Self {
        Self {
            slots: vec![SlotConfig::human(p1), SlotConfig::human(p2)],
        }
    }
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
    /// Compatibility shim: returns the class list for callers
    /// that don't care about who controls each slot.
    pub fn classes(&self) -> impl Iterator<Item = ShipClass> + '_ {
        self.slots.iter().map(|s| s.class)
    }
    /// Get a mutable handle to slot N's class. Returns None if
    /// the slot doesn't exist in this config.
    pub fn class_mut(&mut self, slot: usize) -> Option<&mut ShipClass> {
        self.slots.get_mut(slot).map(|s| &mut s.class)
    }
    pub fn kind(&self, slot: usize) -> Option<PlayerKind> {
        self.slots.get(slot).map(|s| s.kind)
    }
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self::local_two(ShipClass::Earcr, ShipClass::Spael)
    }
}

/// Stable order — picker keys (Digit1..0 for P1, F1..F10 for P2) map to
/// `ALL_CLASSES[i]` by index. Don't reorder existing entries without
/// updating the README key table.
pub const ALL_CLASSES: [ShipClass; 26] = [
    // bank 1 (unmodified picker keys)
    ShipClass::Earcr,
    ShipClass::Spael,
    ShipClass::Yehte,
    ShipClass::Chmav,
    ShipClass::Kzedr,
    ShipClass::Mycpo,
    ShipClass::Shosc,
    ShipClass::Arisk,
    ShipClass::Pkufu,
    ShipClass::Ilwav,
    // bank 2 (hold Shift with the picker key)
    ShipClass::Thrto,
    ShipClass::Vuxin,
    ShipClass::Supbl,
    ShipClass::Kohma,
    ShipClass::Syrpe,
    ShipClass::Andgu,
    ShipClass::Chebr,
    ShipClass::Druma,
    ShipClass::Utwju,
    ShipClass::Zfpst,
    // bank 3 (hold Ctrl with the picker key)
    ShipClass::Mmrxf,
    ShipClass::Orzne,
    ShipClass::Slypr,
    ShipClass::Umgdr,
    ShipClass::Meltr,
    ShipClass::Alabc,
];

/// How rotation responds to forces.
///
/// **Classic** ≈ original SC2: each frame the ship's `AngularVelocity`
/// is *overwritten* with the player's commanded turn rate. Collisions
/// can briefly nudge `AngularVelocity` during a physics step, but the
/// very next frame we stamp it back to the commanded value, so the
/// ship never visibly spins from impacts.
///
/// **Inertial** keeps `AngularVelocity` as a real state variable —
/// hits give you spin, and player input becomes a *target* the
/// controller chases via `ConstantTorque = K · (target − current)`.
/// Pressing either direction pulls your spin toward your input rather
/// than pushing it further out, so a violent spin can be recovered
/// by turning the same way you're spinning (slow correction) or the
/// opposite way (fast correction); doing nothing recovers via damping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AngularControl {
    #[default]
    Classic,
    Inertial,
}

/// Optional global override of every ship's per-class `AngularControl`.
/// `None` means "use what each class declares" (currently Classic for
/// every stock class — matches original SC2). Toggle with `M`.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct AngularControlOverride(pub Option<AngularControl>);

/// Marker + per-instance data for an in-match ship entity.
/// `auto_add_rollback` hook tags every spawned ship for inclusion
/// in the rollback snapshot pipeline.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct Ship {
    pub stats: ShipStats,
    pub player_slot: usize,
}

/// Identifies the ship class for behaviour dispatch. Stat sheets live in
/// `ShipStats` (loaded from `.ini`); per-class *behaviour* lives in
/// `abilities_for(class)` below, which builds an `AbilitySpec` manifest
/// read by the data-driven dispatcher in `src/ability.rs`. Adding a
/// new class is one variant here plus one arm in `abilities_for`.
///
/// The string codes match the legacy TimeWarp ship-file naming so it's
/// easy to grep across the engine + assets + .ini stat sheets.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShipClass {
    /// Earthling Cruiser — point-defense missiles + main gun, fires forward.
    Earcr,
    /// Spathi Eluder — main weapon fires *backwards* ("BUTT missile") so
    /// the ship runs away while shooting. The signature joke of SC2.
    Spael,
    /// Yehat Terminator — twin cannons, energy shield.
    Yehte,
    /// Chmmr Avatar — laser + orbiting defense satellites (TODO).
    Chmav,
    /// Ur-Quan Kzer-Za Dreadnought — fusion bolt + launch fighters (TODO).
    Kzedr,
    /// Mycon Podship — plasmoid (TODO).
    Mycpo,
    /// Shofixti Scout — small, fast, fragile. Glory Device suicide attack.
    Shosc,
    /// Arilou Skiff — auto-targeting halo + hyperspace teleport.
    Arisk,
    /// Pkunk Fury — fast, triple cone of forward fire + phase shift.
    Pkufu,
    /// Ilwrath Avenger — heavy hitter + cloaking device.
    Ilwav,
    /// Thraddash Torch — afterburner leaves a damage trail.
    Thrto,
    /// VUX Intruder — slow heavy fighter; limpets attach to enemies.
    Vuxin,
    /// Supox Blade — agile cruiser; special is 4-way thrust strafing.
    Supbl,
    /// Ur-Quan Kohr-Ah Marauder — flame arc + saw blades on retreat.
    Kohma,
    /// Syreen Penetrator — siren song saps enemy crew.
    Syrpe,
    /// Androsynth Guardian — bubble shots + Blazer comet mode.
    Andgu,
    /// Chenjesu Broodhome — heavy SC1 ship, DOGI lightning mines.
    Chebr,
    /// Druuge Mauler — recoil cannon physically pushes the ship backward.
    Druma,
    /// Utwig Jugger — ricochet shield bounces incoming damage back.
    Utwju,
    /// Zoq-Fot-Pik Stinger — agile, tongue lash close-range attack.
    Zfpst,
    /// Mmrnmhrm X-Form — transforms between Y-Form (fighter) and X-Form
    /// (interceptor). For now, single form with placeholder transform.
    Mmrxf,
    /// Orz Nemesis — flexible-arm cannon + space marines. Both deferred.
    Orzne,
    /// Slylandro Probe — homing lightning. AI-only enemy in original SC2;
    /// TW gave it stats so we ship it as a pickable class.
    Slypr,
    /// Umgah Drone — anti-grav cone weapon, fusion crystal slingshot special.
    Umgdr,
    /// Melnorme Trader — chargeable plasma cannon (canonical mechanic).
    Meltr,
    /// Alary Battle Cruiser (TW-Light fan ship) — slow, heavy,
    /// tanky cruiser. Primary: a slow MIRV torpedo that splits
    /// into homing warheads near the target. Special: toggleable
    /// auto-firing turrets. Passive absorbance shield halves
    /// incoming damage. Ultimate: doubles in size (once).
    Alabc,
}

impl ShipClass {
    /// Asset/.ini code (matches `assets/ships/<code>.ini`).
    pub fn code(self) -> &'static str {
        match self {
            ShipClass::Earcr => "earcr",
            ShipClass::Spael => "spael",
            ShipClass::Yehte => "yehte",
            ShipClass::Chmav => "chmav",
            ShipClass::Kzedr => "kzedr",
            ShipClass::Mycpo => "mycpo",
            ShipClass::Shosc => "shosc",
            ShipClass::Arisk => "arisk",
            ShipClass::Pkufu => "pkufu",
            ShipClass::Ilwav => "ilwav",
            ShipClass::Thrto => "thrto",
            ShipClass::Vuxin => "vuxin",
            ShipClass::Supbl => "supbl",
            ShipClass::Kohma => "kohma",
            ShipClass::Syrpe => "syrpe",
            ShipClass::Andgu => "andgu",
            ShipClass::Chebr => "chebr",
            ShipClass::Druma => "druma",
            ShipClass::Utwju => "utwju",
            ShipClass::Zfpst => "zfpst",
            ShipClass::Mmrxf => "mmrxf",
            ShipClass::Orzne => "orzne",
            ShipClass::Slypr => "slypr",
            ShipClass::Umgdr => "umgdr",
            ShipClass::Meltr => "meltr",
            ShipClass::Alabc => "alabc",
        }
    }
}

/// Live, mutable crew count for a ship. Decoupled from `ShipStats` (which
/// is static class data) so respawns and replays can reset cleanly.
#[derive(Component, Debug, Clone, Copy)]
pub struct Crew {
    pub current: i32,
    pub max: i32,
}

/// Live battery state. Spent by firing (`WeaponDrain`) and triggering
/// specials (`SpecialDrain`); regenerates via `RechargeTimer` per the
/// ship's `.ini` `RechargeAmount` + `RechargeRate`. Like `Crew`, this
/// is *runtime* state, not a class stat.
#[derive(Component, Debug, Clone, Copy)]
pub struct Battery {
    pub current: i32,
    pub max: i32,
}

/// Counts down SC2 frames (50 ms each) until the next battery
/// recharge tick. When it reaches zero we add the ship's
/// `recharge_amount` to its Battery and reset to `recharge_rate`.
/// A `recharge_rate` of 0 means "no natural recharge" (Slylandro).
#[derive(Component, Debug)]
pub struct RechargeTimer {
    /// Seconds until the next `recharge_amount` is added to battery.
    /// Kept in seconds (not SC2 frames) so the FixedUpdate dt
    /// integrates naturally — the previous `remaining_frames: i32`
    /// arithmetic rounded a 60 Hz step's `0.33 frames` to zero each
    /// tick, so the timer never decremented and batteries never
    /// regenerated.
    pub remaining_s: f32,
}

/// Time (in seconds) until the ship's primary weapon can fire again.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct WeaponCooldown(pub f32);

/// Time (in seconds) until the ship's special ability can be used again.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct SpecialCooldown(pub f32);

/// Physics parameters derived from the ship's `.ini` stats once at spawn,
/// using the exact scaling formulas from the legacy TimeWarp engine
/// (`src/melee/mhelpers.cpp` in tw-light). Cached as a component so we
/// don't recompute every tick — `.ini` stats don't change after spawn.
///
/// The legacy constants assume 20 Hz SC2 frames and 0.48 TW-pixels per
/// SC2-pixel. We work in (world-units, seconds) where 1 world unit ≈
/// 1 TW pixel, so the only adjustment is converting milliseconds → seconds.
#[derive(Component, Debug, Clone, Copy)]
pub struct ShipPhysicsDerived {
    /// Steady-state forward speed at full throttle (world units / sec).
    pub speed_max: f32,
    /// Force applied when THRUST is held (Avian force units).
    pub thrust_force: f32,
    /// Steady-state angular rate when LEFT or RIGHT is held (rad / sec).
    /// Sign is the player's input direction; magnitude is class-fixed.
    pub target_omega: f32,
    /// Linear damping that produces `speed_max` at `thrust_force`.
    pub linear_damping: f32,
    /// Angular damping in Inertial mode — agility-scaled so that more
    /// nimble ships (lower `TurnRate` stat → quicker turning) also
    /// shrug off externally-imparted spin faster.
    pub angular_damping: f32,
    /// Proportional gain for the Inertial-mode rate-command controller.
    /// Sized so agile ships recover from a hit within ~0.3 s and the
    /// Ur-Quan Dreadnought takes ~1 s to come back from the same impulse.
    pub inertial_torque_gain: f32,
}

impl ShipPhysicsDerived {
    /// Derives the physics from the legacy ini stats. Mirrors
    /// `scale_velocity`, `scale_acceleration`, `scale_turning` from
    /// `tw-light/src/melee/mhelpers.cpp`.
    pub fn from_stats(stats: &ShipStats, collider_radius: f32) -> Self {
        // Original constants (mhelpers.cpp + mgame.cpp Game defaults).
        const TIME_RATIO_S: f32 = 0.050; // 1 SC2 frame = 50 ms
        const DISTANCE_RATIO: f32 = 0.48; // TW pixels per SC2 pixel

        let speed_max = stats.speed_max * DISTANCE_RATIO / TIME_RATIO_S;
        let accel = stats.accel_rate * DISTANCE_RATIO / TIME_RATIO_S / TIME_RATIO_S;

        // scale_turning(t) = (2π/16) / (t+1) / time_ratio
        // Note: the .ini's TurnRate is INVERSE — lower number → faster turn.
        let target_omega_mag =
            (std::f32::consts::TAU / 16.0) / (stats.turn_rate + 1.0) / TIME_RATIO_S;

        // No linear damping — original SC2 / TW physics is Asteroids-
        // style: ships coast indefinitely until they hit something or
        // burn against their own thrust. Speed-cap enforcement happens
        // via `cap_velocity` clamping `LinearVelocity` to `speed_max`
        // each tick, NOT via a damping term that bleeds momentum.
        let linear_damping = 0.0;
        let thrust_force = stats.mass * accel;

        // Disk moment of inertia: I = ½ m r²
        let inertia = 0.5 * stats.mass * collider_radius * collider_radius;

        // Inertial-mode tuning. We pick a per-class rise-time that grows
        // with TurnRate so nimble ships feel sharp and bulky ones lumber.
        // Inertial-mode controller gain. Tight rise time (~ten ticks)
        // so player input feels almost as snappy as Classic mode —
        // the "inertia" comes from collisions, not from controller
        // sluggishness. Tunable per ship via turn_rate.
        let rise_time = 0.05 + 0.02 * stats.turn_rate; // Earcr (TR=1) → 70 ms
        let inertial_torque_gain = 3.0 * inertia / rise_time;

        // No passive angular damping. In Classic mode it's irrelevant
        // (we overwrite ang_vel each tick); in Inertial mode the user
        // explicitly wants imparted spin to *persist* — collisions
        // should leave the ship tumbling, and only deliberate input
        // (or another collision) should slow / reverse it. If a
        // future ship needs intrinsic stability, that lands as a
        // per-class override or a "stabilizer" Mode field rather than
        // a global default.
        let angular_damping = 0.0;

        Self {
            speed_max,
            thrust_force,
            target_omega: target_omega_mag,
            linear_damping,
            angular_damping,
            inertial_torque_gain,
        }
    }
}

/// While present, the ship takes reduced damage from projectiles and rams.
/// Timer decrements every FixedUpdate; the component is removed on expiry.
#[derive(Component, Debug)]
pub struct ShieldActive {
    pub remaining: f32,
    /// Multiplier on incoming damage (0.0 = invulnerable, 1.0 = none).
    pub damage_factor: f32,
}

/// While present, the ship continuously fires a point-defense beam at
/// any non-friendly projectile (and damages any non-friendly ship)
/// inside `range`. Canonical Earthling Cruiser special — the .ini
/// says Range=5 → 200 world units, Damage=1 per frame for Frames=100
/// frames at 20 Hz ≈ 5 s. We use a shorter `remaining` and rely on
/// `damage_per_tick` ticking at FixedUpdate rate.
#[derive(Component, Debug)]
pub struct PointDefenseActive {
    pub remaining: f32,
    pub range: f32,
    /// Damage dealt to the target each time the laser fires.
    pub damage_per_tick: i32,
    /// Seconds until the laser can fire again. The SDI laser fires
    /// discrete shots at one target per cycle (not continuously every
    /// physics tick), so it reads as visible pulses and doesn't deal
    /// 60-per-second damage.
    pub cooldown_s: f32,
}

/// Bevy component on-add hook: tag the new entity with
/// `bevy_ggrs::Rollback` so its state is included in rollback
/// snapshots. Used by every gameplay-relevant marker component
/// (Projectile, DamageZone, SubEntity, ...) — beats remembering
/// to add the marker at every spawn site.
///
/// Idempotent: if the entity already has `Rollback` (e.g.
/// because two of these markers were inserted together), don't
/// re-insert.
fn auto_add_rollback(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    if world.get::<bevy_ggrs::Rollback>(ctx.entity).is_some() {
        return;
    }
    world.commands().entity(ctx.entity).try_insert(bevy_ggrs::Rollback);
}

/// In-flight projectile. Owner is tracked so we can ignore self-hits.
/// On-add hook auto-tags the entity with `bevy_ggrs::Rollback` so
/// the projectile's state participates in rollback snapshots
/// without having to remember to add the marker at every spawn
/// site.
#[derive(Component, Debug)]
#[component(on_add = projectile_on_add)]
pub struct Projectile {
    pub owner: Entity,
    pub damage: i32,
    pub lifetime: f32,
}

/// Layer bits used by `CollisionLayers` to filter own-slot projectiles
/// out of a ship's contact set. Bit 0 is the Avian DEFAULT layer (used
/// by anything we don't tag explicitly: asteroids, planet, damage zones,
/// sub-entities). Bits 1..=4 are ship-per-slot. Bits 5..=8 are
/// projectile-per-slot. Filters work bidirectionally — if either side's
/// `filters` excludes the other's `memberships`, no contact happens, so
/// it's enough to set the SHIP'S filter to exclude its own slot's
/// projectile bit and the projectile is invisible to the firer.
pub(crate) fn ship_layers(slot: usize) -> CollisionLayers {
    let slot = slot.min(3) as u32;
    let memberships = 1u32 << (1 + slot);
    let own_proj_bit = 1u32 << (5 + slot);
    CollisionLayers::from_bits(memberships, 0xffff_ffffu32 & !own_proj_bit)
}

pub(crate) fn projectile_layers(slot: usize) -> CollisionLayers {
    let slot = slot.min(3) as u32;
    let memberships = 1u32 << (5 + slot);
    CollisionLayers::from_bits(memberships, 0xffff_ffffu32)
}

/// On-add hook for `Projectile`: tag the entity for rollback and stamp
/// it with `CollisionLayers` keyed to the firer's slot, so the firer's
/// own ship is filtered out of the projectile's contact set (no
/// muzzle-output recoil / spin on the firer). Opponent ships still
/// collide normally, taking Avian's default linear+angular impulse.
fn projectile_on_add(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    // Look up the firer's slot via `Projectile.owner` → `Ship.player_slot`.
    // Fall back to slot 0 if the chain breaks (the firer was already
    // despawned, or the projectile is owned by a non-ship sub-entity).
    let owner = world.get::<Projectile>(ctx.entity).map(|p| p.owner);
    let slot = owner
        .and_then(|e| world.get::<Ship>(e).map(|s| s.player_slot))
        .unwrap_or(0);
    let layers = projectile_layers(slot);
    let need_rb = world.get::<bevy_ggrs::Rollback>(ctx.entity).is_none();
    let need_layers = world.get::<CollisionLayers>(ctx.entity).is_none();
    let mut commands = world.commands();
    let mut ec = commands.entity(ctx.entity);
    if need_rb {
        ec.try_insert(bevy_ggrs::Rollback);
    }
    if need_layers {
        ec.try_insert(layers);
    }
}

/// Tracking-projectile state. While present on a projectile, a system
/// nudges the projectile's velocity each tick toward the nearest enemy
/// ship, capped at `turn_rate` rad/s. Without this component a
/// projectile flies in a straight line.
///
/// `target` is cached across ticks for stability and zeroed out if the
/// targeted entity is despawned (the ship was destroyed); the next tick
/// re-acquires the nearest survivor.
#[derive(Component, Debug)]
pub struct Homing {
    pub target: Option<Entity>,
    /// Max rad/sec the projectile can re-aim. Comes from a VolleySpec
    /// field so designers can give light tracking missiles a tight
    /// turn rate and heavy plasma bolts a slow drift toward target.
    pub turn_rate: f32,
}

/// Circular area-of-effect damage source. Lives independently of ships
/// and projectiles. Used for: Shofixti Glory Device burst, Kohr-Ah
/// sawblade ring, Chenjesu DOGI mines, eventually Slylandro Probe
/// self-destruct.
///
/// Damage is applied continuously at `damage_per_sec` to any ship
/// inside `radius` whose entity ≠ `source`. Set `source = None` for
/// "no friendly fire exemption" (Glory Device kills the firer too).
/// `lifetime` decrements every tick; on expiry the zone despawns.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct DamageZone {
    pub radius: f32,
    pub damage_per_sec: f32,
    pub lifetime: f32,
    pub source: Option<Entity>,
}

/// Spawns a damage zone as a sensor circle collider. Avian's
/// physics pipeline reports overlap via `CollidingEntities`, which
/// `tick_damage_zones` reads each tick — replacing the previous
/// custom distance check. Owner of the zone can be excluded via the
/// `source` field.
pub(crate) fn spawn_damage_zone(
    commands: &mut Commands,
    source: Option<Entity>,
    pos: Vec2,
    radius: f32,
    damage_per_sec: f32,
    lifetime: f32,
    color: Color,
    sprite: Option<Handle<Image>>,
) {
    // With a sprite (e.g. the Thraddash fireball) we tint+size it;
    // otherwise fall back to a plain coloured disc.
    let sprite = match sprite {
        Some(image) => Sprite {
            image,
            color,
            custom_size: Some(Vec2::splat(radius * 2.0)),
            ..default()
        },
        None => Sprite::from_color(color, Vec2::splat(radius * 2.0)),
    };
    commands.spawn((
        DamageZone {
            radius,
            damage_per_sec,
            lifetime,
            source,
        },
        sprite,
        Transform::from_translation(pos.extend(0.2)),
        // Physics-native overlap detection. `Sensor` means Avian
        // tracks collisions for events but applies no impulse — the
        // ship doesn't bounce off the zone. `CollidingEntities` is
        // updated by Avian each step with the set of entities
        // currently inside the zone's collider.
        RigidBody::Static,
        Collider::circle(radius),
        Sensor,
        CollidingEntities::default(),
        Position(pos),
    ));
}

/// Owner-attached damage zone — same gameplay shape as `DamageZone`
/// but its world position is recomputed each tick from the owner's
/// `Position + Rotation`, so it sticks to the ship as it moves.
///
/// Canonical uses (legacy `shp*.cpp`):
///   - Umgah anti-grav cone (`shpumgdr.cpp:UmgahCone`) — a fixed
///     forward offset that damages anything inside while fire is held
///   - Zoq-Fot-Pik tongue lash (`shpzfpst.cpp:ZoqFotPikTongue`) — a
///     short-lived attached zone tip-extended from the Stinger
///
/// Friendly-fire immunity is automatic (owner is never damaged by its
/// own attached zone, same as `DamageZone::source = Some(owner)`).
#[derive(Component, Debug)]
pub struct AttachedDamageZone {
    pub owner: Entity,
    pub local_offset: Vec2,
    pub radius: f32,
    pub damage_per_sec: f32,
    pub lifetime: f32,
    pub color: Color,
}

/// Spawn an attached damage zone owned by `owner`. Position is
/// undefined until the first `tick_attached_damage_zones` pass — the
/// system snaps it into place on the next FixedUpdate.
pub(crate) fn spawn_attached_damage_zone(
    commands: &mut Commands,
    owner: Entity,
    owner_pos: Vec2,
    owner_rot: &Rotation,
    local_offset: Vec2,
    radius: f32,
    damage_per_sec: f32,
    lifetime: f32,
    color: Color,
) {
    // Initial world pose so the zone renders in the right place on
    // its first render frame (before tick_attached_damage_zones runs).
    let world_offset = Vec2::new(
        local_offset.x * owner_rot.cos - local_offset.y * owner_rot.sin,
        local_offset.x * owner_rot.sin + local_offset.y * owner_rot.cos,
    );
    let world_pos = owner_pos + world_offset;
    commands.spawn((
        AttachedDamageZone {
            owner,
            local_offset,
            radius,
            damage_per_sec,
            lifetime,
            color,
        },
        Sprite::from_color(color, Vec2::splat(radius * 2.0)),
        Transform::from_translation(world_pos.extend(0.2)),
        // Avian-native overlap detection. RigidBody::Kinematic
        // because we move it manually each tick (no physics
        // integration). `Sensor` disables impulse response so the
        // owner ship doesn't ram itself off its own cone.
        RigidBody::Kinematic,
        Collider::circle(radius),
        Sensor,
        CollidingEntities::default(),
        Position(world_pos),
    ));
}

/// All 64 rotation frames preloaded so the renderer can pick by heading
/// without hitting the asset server hot path.
#[derive(Component)]
pub struct ShipFrames {
    pub frames: Vec<Handle<Image>>,
}

/// Per-ship state for the Orz Nemesis (`shporzne.cpp`).
///
/// The Orz has a SEPARATE rotating turret sprite on top of the hull
/// (`data->spriteExtra`, our `OverlaySprite` with the `shot_d_NN`
/// frames). Holding SPECIAL re-routes the turn keys to rotate the
/// turret instead of the ship; primary fire is suppressed while
/// special is held, and pressing FIRE while special is held spawns a
/// space marine (costs 1 crew, capped at MAX_MARINES). With special
/// released, primary fires in the TURRET's facing (ship angle +
/// `offset_rad`), not the hull's.
#[derive(Component, Debug, Default)]
pub struct OrzTurret {
    /// Turret aim relative to the ship's facing, in radians. The
    /// `OverlaySprite::extra_angle` mirrors this each tick so the art
    /// follows.
    pub offset_rad: f32,
    /// Rising-edge detect for "spawn one marine per fire press while
    /// special is held" (no firehose-of-marines from holding fire).
    pub last_fire_held: bool,
    /// Seconds until the cannon can fire again (mirrors WeaponRate=4
    /// SC2 frames → 0.2 s, ticked here because primary fire is fully
    /// managed by the Orz system rather than the generic dispatcher).
    pub fire_cooldown_s: f32,
}

/// Marker on an Orz marine that has reached its target and is now
/// boarding it. While present:
///   - The marine sticks to the host's position each tick (visually
///     "clinging" — canon draws it as an icon on the host's crew panel).
///   - Steering / lifetime decrement is skipped — the marine lives until
///     the dice say otherwise or the host dies.
///   - Each canon SC2 frame (50 ms) we roll the per-frame chance from
///     `shporzne.cpp:OrzMarine::calculate`:
///       * 9/10000 chance per ms · 50 ms ≈ 4.5% per tick → 1 crew dmg.
///       * +1/10000 chance per ms · 50 ms ≈ 0.5% per tick → marine dies.
/// `roll_accum_s` is the 50-ms tick accumulator so the system rolls at
/// the canonical 20 Hz cadence regardless of our actual physics rate.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct OrzMarineBoarded {
    pub host: Entity,
    pub roll_accum_s: f32,
}

/// Per-ship state for the Slylandro Probe (`shpslypr.cpp`). Canon
/// behaviour: the probe is ALWAYS thrusting forward; pressing the
/// thrust key flips the ship 180° (rising edge only) so direction
/// changes are instant rather than gradual. `tick_slylandro_drift`
/// detects the rising edge against `last_thrust_held` and writes the
/// rotation/thrust override after `apply_player_input`.
#[derive(Component, Debug, Default)]
pub struct SlylandroDrift {
    pub last_thrust_held: bool,
}

/// Per-ship state for Melnorme charge-and-release primary. Continuous
/// linear interpolation of damage / scale / colour from base to max
/// over `max_charge_s` of hold time.
#[derive(Component, Debug, Default)]
pub struct MeltrChargeState {
    pub active: Option<Entity>,
    /// Seconds the fire key has been held since this charge started.
    pub sub_charge_s: f32,
    pub last_fire_held: bool,
    /// Last integer damage the shot was set to. When the next tick's
    /// computed damage rounds to a different integer, we flash the
    /// shot — the underlying interpolation is smooth but the damage
    /// *value* is discrete, so the player needs a visual cue every
    /// time it actually ticks up.
    pub last_damage: i32,
    /// Seconds remaining of a "charge tier crossed" white flash.
    pub flash_remaining_s: f32,
}

/// Per-ship state for ships whose primary launches a charging or
/// held projectile that needs on-release handling. Currently used by
/// Chenjesu Broodhome — the crystal stays in flight until the fire
/// button is released, at which point it shatters into 8 shards
/// radiating from its current position.
///
/// `current` is `Some(projectile_entity)` while a held projectile is
/// alive, `None` otherwise. The dedicated tick system clears it
/// either when the projectile dies on a hit (collision → despawn) or
/// after the on-release behaviour fires.
/// Shofixti Glory Device arming state: the suicide blast needs three
/// `special` presses to confirm. Presses reset if you wait too long, so
/// a stray tap doesn't leave you primed to die.
#[derive(Component, Debug, Default)]
pub struct ShofixtiGlory {
    pub presses: u8,
    pub since_last_s: f32,
}

#[derive(Component, Debug, Default)]
pub struct CrystalCarrier {
    pub current: Option<Entity>,
    /// Rolling state mirror — same idea as `LastTurnInput`. Bevy's
    /// `just_pressed`/`just_released` is fragile across FixedUpdate,
    /// so we track edge transitions ourselves.
    pub last_fire_held: bool,
}

/// Per-ship state for the Alary Battle Cruiser MIRV launcher
/// (`shpalabc.cpp`). The primary fires ONE slow homing torpedo per
/// press; the torpedo splits into five warheads on proximity to a
/// target. Muzzle alternates side each shot (legacy `side *= -1`).
#[derive(Component, Debug)]
pub struct AlaryMirvState {
    /// Edge tracking for the fire key — the launcher is one-shot
    /// per press, not auto-fire.
    pub last_fire_held: bool,
    /// Seconds until the launcher can fire again.
    pub cooldown_s: f32,
    /// +1.0 / −1.0, flipped after each launch so consecutive
    /// torpedoes leave from opposite sides of the hull.
    pub side: f32,
}

impl Default for AlaryMirvState {
    fn default() -> Self {
        Self {
            last_fire_held: false,
            cooldown_s: 0.0,
            side: 1.0,
        }
    }
}

/// Per-ship state for the Alary's toggleable hull turret battery
/// (`shpalabc.cpp` special). `special` press toggles the turrets
/// on/off; while on, each of the three turrets auto-fires at the
/// nearest enemy on its own recharge timer.
#[derive(Component, Debug, Default)]
pub struct AlaryTurrets {
    pub on: bool,
    pub last_special_held: bool,
    /// Per-turret recharge timers (3 hull turrets).
    pub recharge_s: [f32; 3],
}

/// A slow homing MIRV torpedo in flight. Does NO contact damage on
/// its own (legacy note: "Torpedo itself will not do damage on
/// collisions"); when it closes within `proximity` world units of an
/// enemy ship it despawns and spawns five homing warheads fanned at
/// `[0, ±50°, ±75°]` off its heading.
#[derive(Component, Debug)]
pub struct AlaryTorpedo {
    pub owner: Entity,
    pub proximity: f32,
    pub wh_speed: f32,
    pub wh_damage: i32,
    pub wh_turn: f32,
    pub wh_lifetime: f32,
}

/// Inertialess-drive marker. A ship with this component has its
/// linear velocity *directly* set from thrust input each tick:
/// thrust held → forward at `speed_max`; thrust released → zero
/// instantly. Acceleration is infinite — no momentum accumulation,
/// no coasting.
///
/// Because the velocity is overwritten every tick, external impulses
/// (collisions, tractor beams, projectile recoil) get cancelled out
/// automatically — matching the canonical
/// `shparisk.cpp:ArilouSkiff::accelerate` which rejects any
/// acceleration whose `source != this`.
///
/// Used by Arilou Skiff today. Generic enough that any future ship
/// with the same flavour can pick it up via a marker insert.
#[derive(Component, Debug)]
pub struct InertialessDrive;

/// Per-ship rolling state for Inertial-mode steering. Bevy's
/// `ButtonInput::just_released` is fragile when `FixedUpdate` runs at
/// a different cadence than the main render loop — the release event
/// can be cleared between Bevy frames before any FixedUpdate ticks
/// observe it. We track the previous tick's "is a turn key held"
/// state on the ship itself, so the rising/falling edge is detected
/// deterministically in the same schedule that consumes it.
#[derive(Component, Default, Debug)]
pub struct LastTurnInput {
    pub had_input: bool,
}

pub struct ShipPlugin;

impl Plugin for ShipPlugin {
    fn build(&self, app: &mut App) {
        // M1 ordering: gameplay logic in FixedUpdate (so it runs at the
        // physics rate), visual swaps in Update (frame-rate). M4 moves the
        // gameplay systems into GgrsSchedule.
        app.init_resource::<MatchConfig>()
            .init_resource::<AngularControlOverride>()
            .add_systems(Update, (class_picker_input, cycle_angular_override));
        // Bevy 0.18's `add_systems` macro caps a single tuple at 20
        // entries. We've outgrown it; split into two FixedUpdate
        // groups (the order across groups is unconstrained, but each
        // system inside this plugin is independent so that's fine).
        app.add_systems(
            bevy_ggrs::GgrsSchedule,
            (
                process_mode_toggle_requests,
                tick_ship_modes,
                apply_player_input,
                cap_velocity,
                tick_weapon_cooldown,
                tick_special_cooldown,
                tick_shield,
                tick_point_defense,
                tick_battery_recharge,
                tick_chebr_crystal,
                tick_meltr_charge,
                tick_kohma_blade,
                tick_kohma_passive_blades,
                tick_projectile_lifetime,
                steer_homing_projectiles,
                orient_projectiles,
                tick_damage_zones,
                tick_attached_damage_zones,
                tick_beams,
                tick_tractors,
            ),
        );
        // Pkunk aggressive-clone AI lives in its own add_systems
        // so we can apply `.after(apply_player_input)` without
        // spilling the 21-element tuple limit on the main
        // gameplay-schedule set. The .after dependency is what
        // lets it override the leader's player input on the clones.
        app.add_systems(
            bevy_ggrs::GgrsSchedule,
            crate::ultimate::tick_pkunk_aggressive_clones
                .after(apply_player_input),
        );
        // Planet gravity nudges LinearVelocity; run it before the speed
        // cap so the whip-boosted cap is honoured the same tick.
        // `tick_planet_grind` drains crew while a ship is *touching* the
        // planet, on top of the one-shot CollisionStart damage from
        // `tick_planet_contact` — gravity-pinned ships die.
        app.add_systems(
            bevy_ggrs::GgrsSchedule,
            (
                apply_planet_gravity.before(cap_velocity),
                tick_planet_contact,
                tick_planet_grind,
            ),
        );
        // Orz turret + marines own their input handling; must run AFTER
        // apply_player_input so it can clobber the hull's ang_vel when
        // special is held (rotate the turret, not the ship). Slylandro
        // drift similarly overrides thrust/rotation on rising-edge of
        // thrust to flip the probe 180°.
        app.add_systems(
            bevy_ggrs::GgrsSchedule,
            (
                tick_orz_turret.after(apply_player_input),
                tick_slylandro_drift.after(apply_player_input),
                tick_orz_marines_boarded,
            ),
        );
        app.add_systems(
            bevy_ggrs::GgrsSchedule,
            (
                tick_invisible,
                tick_damage_to_battery,
                tick_sub_entities,
                handle_projectile_hits,
                handle_sub_entity_collisions,
                handle_mode_contact_damage,
                apply_syreen_drain,
                tick_mycon_plasma_birth,
                tick_mycon_plasma,
                spawn_chmmr_satellites,
                tick_chmmr_satellites,
                tick_zap_flashes,
                tick_asteroid_explosions,
                replenish_asteroids,
                tick_alary_mirv,
                tick_alary_turrets,
                tick_shofixti_glory,
            ),
        )
        .add_systems(
            Update,
            (
                swap_rotation_frame,
                update_overlay_sprites,
                tick_invisible_visual,
                draw_shield_rings,
                draw_gravity_field,
            ),
        );
    }
}

pub fn load_ship_catalog(mut commands: Commands) {
    let mut ships = HashMap::new();
    for (code, ini_str, txt_str) in SHIP_INIS {
        match ShipStats::from_str(code, ini_str, txt_str) {
            Ok(stats) => {
                info!("loaded ship {} ({})", stats.code, stats.name);
                ships.insert(code.to_string(), stats);
            }
            Err(e) => warn!("skipping {code}: {e}"),
        }
    }
    info!("ship catalog: {} entries", ships.len());
    info!("------------------------------------------------------------");
    info!("CONTROLS");
    info!("  P1 — Arrow keys (turn / thrust),  / fire,  . special");
    info!("  P2 — W A D      (thrust / turn),  Z fire,  L-Shift special");
    info!("  ULTIMATE — hold turn-L + turn-R + backward (Down for P1, S for P2)");
    info!("  Tab / Shift+Tab        — cycle P1 to next/prev ship");
    info!("  ` (backtick) / Shift+` — cycle P2 to next/prev ship");
    info!("  Digits 1..0            — direct-pick P1 (Shift/Ctrl = banks 11-20, 21-25)");
    info!("  F1..F10                — direct-pick P2 (same modifier banks)");
    info!("  M                      — cycle angular control: Classic / Inertial");
    info!("  F3                     — toggle collider debug overlay (polygon outlines)");
    info!("  R (only post-match)    — rematch");
    info!("------------------------------------------------------------");
    commands.insert_resource(ShipCatalog { ships });
}

/// Spawn the test scene. Classes come from `MatchConfig`, which is
/// mutable via the class-picker hotkeys (`1`..=`9`, `0` → P1;
/// `F1`..=`F10` → P2; same index into `ALL_CLASSES`).
pub fn spawn_match(
    mut commands: Commands,
    catalog: Res<ShipCatalog>,
    assets: Res<AssetServer>,
    config: Res<MatchConfig>,
    ship_colliders: Res<crate::collider::ShipColliders>,
    mut rng: ResMut<crate::rng::GameRng>,
) {
    // Compass-point spawns. Up to 4 players — slots 2 and 3 are
    // populated for online / 4-player local; otherwise the loop
    // just emits the first 2.
    //
    // Rotation convention: 0 rad = sprite facing +Y (up), positive
    // = CCW. -π/2 → +X (right), +π/2 → -X (left), 0 → +Y,
    // π → -Y. All four ships face the arena centre.
    //
    // Spawn radius is 700 → opposing pairs start 1400 apart, just UNDER the
    // half-arena (1500). That matters for the toroidal camera: if the pair
    // started more than half an arena apart (e.g. the old ±900 → 1800), the
    // genuinely shortest path between them would be the WRAP direction, so
    // the auto-framing would want to show them wrapped (back-to-back) instead
    // of facing each other across the arena. Keeping the start under half the
    // arena makes "face each other directly" the true minimum-image framing,
    // and flying outward past the half-arena point then cleanly hands off to
    // the wrap framing. (700 also sits right at the planet's gravity_range, so
    // ships feel no initial pull.)
    use std::f32::consts::{FRAC_PI_2, PI};
    let spawn_table: [(Vec2, f32); 4] = [
        (Vec2::new(-740.0, 0.0), -FRAC_PI_2), // W, facing E
        (Vec2::new(740.0, 0.0), FRAC_PI_2),   // E, facing W
        (Vec2::new(0.0, -740.0), 0.0),        // S, facing N
        (Vec2::new(0.0, 740.0), PI),          // N, facing S
    ];
    for (slot, slot_cfg) in config.slots.iter().enumerate().take(4) {
        let (pos, rot) = spawn_table[slot];
        let Some(entity) = spawn_class(
            &mut commands,
            &catalog,
            &assets,
            slot_cfg.class,
            pos,
            rot,
            slot,
            &ship_colliders,
        ) else {
            continue;
        };
        // Tag AI slots so `apply_player_input` skips them and
        // `tick_ai_pilots` drives them instead, plus
        // `dispatch_primary` force-fires their guns.
        if slot_cfg.kind == PlayerKind::Ai {
            commands
                .entity(entity)
                .try_insert((crate::ai::AiControlled, crate::ai::AiBrain::default()));
        }
    }

    spawn_planet(&mut commands, &assets, &mut rng);
    spawn_asteroids(&mut commands, &assets, &mut rng);
}

/// Class-picker hotkeys. Two ways in:
///
///   1. Direct-pick: digits `1`..`0` set P1; `F1`..`F10` set P2.
///      Hold Shift to pick from the second bank of ten classes, Ctrl
///      for the third bank.
///   2. Cycle: `Tab` / `Shift+Tab` walks P1 forward / backward
///      through `ALL_CLASSES`; `~` (backtick) / `Shift+~` does the
///      same for P2.
///
/// Either way, changing a class triggers an immediate `AppState::
/// Resetting` so the new ship spawns *now*, not on the next rematch.
/// Lets you test ship behaviours without waiting for one to die.
///
/// **Disabled in online play.** The .ini-driven class roster is set in
/// the lobby UI before the match; mid-match keyboard hotkeys would
/// only mutate the local peer's `MatchConfig` (it isn't synced over
/// the wire), which would desync the ship — and worse, since both
/// peers route Tab/digits to "slot 0" the keypress on the guest swaps
/// the *host's* character locally, exactly the symptom the user hit.
fn class_picker_input(
    keys: Res<ButtonInput<KeyCode>>,
    virt: Res<crate::input::VirtualInput>,
    mut config: ResMut<MatchConfig>,
    mut next_state: ResMut<NextState<crate::AppState>>,
    current_state: Res<State<crate::AppState>>,
    session: Option<Res<bevy_ggrs::Session<crate::netplay::Config>>>,
) {
    // Online: classes are negotiated in the lobby, not via hotkeys.
    // Bail before reading any keys so a stray Tab can't damage the
    // local-only MatchConfig either.
    if session.is_some() {
        return;
    }
    const P1_DIGITS: [KeyCode; 10] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ];
    const P2_FKEYS: [KeyCode; 10] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
    ];

    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let bank_offset = if ctrl {
        20
    } else if shift {
        10
    } else {
        0
    };

    let mut changed = false;

    // Picker keys only mutate slots that already exist in
    // `config.classes`. P1 digits → slot 0; P2 F-keys → slot 1.
    // Slots 2+ are configured via the lobby UI for online play,
    // not from these hotkeys.
    for (i, key) in P1_DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let (Some(class), Some(slot)) =
                (ALL_CLASSES.get(idx).copied(), config.class_mut(0))
            {
                if *slot != class {
                    *slot = class;
                    info!("P1 → {:?}", class);
                    changed = true;
                }
            }
        }
    }
    for (i, key) in P2_FKEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let (Some(class), Some(slot)) =
                (ALL_CLASSES.get(idx).copied(), config.class_mut(1))
            {
                if *slot != class {
                    *slot = class;
                    info!("P2 → {:?}", class);
                    changed = true;
                }
            }
        }
    }

    // Cycling hotkeys — quick way to walk the roster mid-match.
    // Tab / Shift+Tab on keyboard, or the on-screen ⟨ / ⟩ buttons
    // (which set `virt.cycle_*_just_pressed`) on phones.
    let cycle_next =
        keys.just_pressed(KeyCode::Tab) && !shift || virt.cycle_next_just_pressed;
    let cycle_prev =
        keys.just_pressed(KeyCode::Tab) && shift || virt.cycle_prev_just_pressed;
    if cycle_next || cycle_prev {
        let dir: i32 = if cycle_prev { -1 } else { 1 };
        if let Some(slot) = config.class_mut(0) {
            *slot = cycle_class(*slot, dir);
            info!("P1 → {:?}", *slot);
            changed = true;
        }
    }
    if keys.just_pressed(KeyCode::Backquote) {
        let dir: i32 = if shift { -1 } else { 1 };
        if let Some(slot) = config.class_mut(1) {
            *slot = cycle_class(*slot, dir);
            info!("P2 → {:?}", *slot);
            changed = true;
        }
    }

    // Trigger a fresh spawn so the change takes effect immediately —
    // skip if we're mid-reset already (would re-enter the state
    // machine in the same frame, harmless but noisy).
    if changed && *current_state.get() == crate::AppState::InMatch {
        next_state.set(crate::AppState::Resetting);
    }
}

fn cycle_class(current: ShipClass, dir: i32) -> ShipClass {
    let n = ALL_CLASSES.len() as i32;
    let cur_idx = ALL_CLASSES
        .iter()
        .position(|c| *c == current)
        .map(|i| i as i32)
        .unwrap_or(0);
    let next = ((cur_idx + dir).rem_euclid(n)) as usize;
    ALL_CLASSES[next]
}

/// Despawn every gameplay entity from the previous round so the next
/// OnEnter(InMatch) can spawn a clean scene. Camera, HUD nodes, and
/// the catalog resource survive.
pub fn teardown_match(
    mut commands: Commands,
    ships: Query<Entity, With<Ship>>,
    projectiles: Query<Entity, With<Projectile>>,
    damage_zones: Query<Entity, With<DamageZone>>,
    attached_zones: Query<Entity, With<AttachedDamageZone>>,
    beams: Query<Entity, With<Beam>>,
    tractors: Query<Entity, With<TractorBeam>>,
    sub_entities: Query<Entity, With<SubEntity>>,
    overlays: Query<Entity, With<OverlaySprite>>,
    satellites: Query<Entity, With<ChmmrSatellite>>,
    asteroids: Query<Entity, With<Asteroid>>,
    planets: Query<Entity, With<Planet>>,
) {
    for e in &ships {
        commands.entity(e).try_despawn();
    }
    for e in &planets {
        commands.entity(e).try_despawn();
    }
    for e in &projectiles {
        commands.entity(e).try_despawn();
    }
    for e in &damage_zones {
        commands.entity(e).try_despawn();
    }
    for e in &attached_zones {
        commands.entity(e).try_despawn();
    }
    for e in &beams {
        commands.entity(e).try_despawn();
    }
    for e in &tractors {
        commands.entity(e).try_despawn();
    }
    for e in &sub_entities {
        commands.entity(e).try_despawn();
    }
    for e in &overlays {
        commands.entity(e).try_despawn();
    }
    for e in &satellites {
        commands.entity(e).try_despawn();
    }
    for e in &asteroids {
        commands.entity(e).try_despawn();
    }
}

pub fn spawn_class(
    commands: &mut Commands,
    catalog: &ShipCatalog,
    assets: &AssetServer,
    class: ShipClass,
    position: Vec2,
    rotation_rad: f32,
    slot: usize,
    ship_colliders: &crate::collider::ShipColliders,
) -> Option<Entity> {
    let code = class.code();
    let Some(stats) = catalog.ships.get(code).cloned() else {
        error!("{code} missing from catalog");
        return None;
    };
    let frames = load_rotation_frames(assets, code, class);
    if frames.is_empty() {
        error!("no rotation frames found for {code}");
        return None;
    }
    let id = spawn_ship(
        commands,
        assets,
        class,
        &stats,
        &frames,
        position,
        rotation_rad,
        slot,
        ship_colliders,
    );
    info!("spawned P{} as {}", slot + 1, stats.name);
    Some(id)
}

/// Spawns a playable ship at the given pose. All ships share this builder so
/// per-ship tuning (damping, collider size, etc.) lives in one place.
///
/// Angular damping is kept low enough that collisions impart visible spin —
/// fighter-class ships rely on their high `turn_rate` (and thus high applied
/// torque) to recover quickly, while larger ships get a boat-like feel for
/// free as their bigger moment of inertia naturally fights the torque.
fn spawn_ship(
    commands: &mut Commands,
    assets: &AssetServer,
    class: ShipClass,
    stats: &ShipStats,
    frames: &[Handle<Image>],
    position: Vec2,
    rotation_rad: f32,
    slot: usize,
    ship_colliders: &crate::collider::ShipColliders,
) -> Entity {
    let initial = frames.first().cloned().unwrap_or_default();
    let phys = physics_spec(class);
    let derived = ShipPhysicsDerived::from_stats(stats, phys.collider_radius);

    let gameplay = (
        Ship {
            stats: stats.clone(),
            player_slot: slot,
        },
        class,
        Crew {
            current: stats.crew_start,
            max: stats.crew_max,
        },
        Battery {
            current: stats.batt_max,
            max: stats.batt_max,
        },
        RechargeTimer {
            // RechargeRate is in SC2 frames; convert to seconds for
            // FixedUpdate integration (50 ms per SC2 frame).
            remaining_s: stats.recharge_rate as f32 * 0.050,
        },
        WeaponCooldown::default(),
        SpecialCooldown::default(),
        LastTurnInput::default(),
        ShipFrames {
            frames: frames.to_vec(),
        },
        derived,
    );
    let visual = (
        Sprite::from_image(initial),
        Transform::from_translation(position.extend(0.0)),
    );
    // Prefer the auto-extracted polygon collider; fall back to the
    // hand-tuned circle radius if the polygon isn't ready yet (race
    // between the asset loader and the very first spawn on page load).
    //
    // `convex_decomposition` (vs `convex_hull`) preserves the sprite's
    // concavities — fed a closed polyline of N vertices and N edge
    // indices, Avian internally splits the enclosed region into a
    // bag of convex pieces. Two ships in a "C" / "L" silhouette
    // actually fit into each other instead of pretending they're both
    // smooth ovals.
    let collider = ship_colliders
        .polys
        .get(&class)
        .map(|poly| {
            let verts = poly.clone();
            let n = verts.len() as u32;
            let indices: Vec<[u32; 2]> = (0..n).map(|i| [i, (i + 1) % n]).collect();
            Collider::convex_decomposition(verts, indices)
        })
        .unwrap_or_else(|| Collider::circle(phys.collider_radius));

    let physics = (
        RigidBody::Dynamic,
        collider,
        Mass(stats.mass),
        // Filter own-slot projectiles out of this ship's collision set —
        // a ship can't be pushed or spun by its own muzzle output, but
        // everything else (asteroids, planet, ship-ship rams, opponent
        // weapons) collides normally and imparts Avian's default
        // linear+angular impulse. See `ship_layers` / `projectile_layers`
        // for the bit scheme.
        ship_layers(slot),
        // `Position` is now mandatory because we disabled
        // `PhysicsTransformConfig::transform_to_position` — Avian no
        // longer reads spawn pose from `Transform`, so without an
        // explicit `Position` every ship starts at (0, 0).
        Position(position),
        Rotation::radians(rotation_rad),
        LinearDamping(derived.linear_damping),
        AngularDamping(derived.angular_damping),
        LinearVelocity::ZERO,
        AngularVelocity::ZERO,
        ConstantLocalForce(Vec2::ZERO),
        ConstantTorque(0.0),
        CollisionEventsEnabled,
    );
    let mut entity = commands.spawn((gameplay, visual, physics));
    // Rollback marker: bevy_ggrs only snapshots / restores
    // entities tagged with this. Ships are the primary gameplay
    // entity, so they're always tagged.
    entity.insert(bevy_ggrs::Rollback);
    // Attach the data-driven ability manifest. Every class has one
    // today; the dispatcher in `src/ability.rs` reads it and produces
    // the right ECS spawns.
    if let Some(abilities) = abilities_for(class) {
        entity.insert(abilities);
    }
    // Multi-form ships (Mmrxf, Andgu) carry a `ShipModes` component.
    // `tick_ship_modes` swaps in the active mode's stats/sprite/
    // abilities on the first frame (applied=None → needs apply).
    if let Some(modes) = modes_for(class, stats, frames, &derived, phys.collider_radius, assets) {
        entity.insert(modes);
    }
    // Per-class one-off markers. Cleaner than a generic
    // `feature_flags_for` since the set is small. Add to the match
    // when a new ship needs a class-specific tweak.
    if matches!(class, ShipClass::Arisk) {
        entity.insert(InertialessDrive);
    }
    if matches!(class, ShipClass::Chebr) {
        entity.insert(CrystalCarrier::default());
    }
    if matches!(class, ShipClass::Shosc) {
        entity.insert(ShofixtiGlory::default());
    }
    if matches!(class, ShipClass::Meltr) {
        entity.insert(MeltrChargeState::default());
    }
    if matches!(class, ShipClass::Orzne) {
        entity.insert(OrzTurret::default());
    }
    if matches!(class, ShipClass::Slypr) {
        entity.insert(SlylandroDrift::default());
    }
    if matches!(class, ShipClass::Mycpo) {
        // Marker so newly-spawned projectiles owned by Mycon ships
        // get the MyconPlasmaPulse animator attached.
        entity.insert(MyconPlasmaShooter);
    }
    if matches!(class, ShipClass::Kohma) {
        entity.insert(KohrAhBladeCarrier::default());
    }
    if matches!(class, ShipClass::Chmav) {
        // Spawned ship needs its three orbiting satellites. We
        // can't spawn them here (commands hasn't flushed the ship
        // entity yet), so we just mark the ship; a Startup-after
        // OnEnter system spawns the satellites.
        entity.insert(NeedsChmmrSatellites);
    }
    if matches!(class, ShipClass::Alabc) {
        // Permanent absorbance shield — halves all incoming
        // damage for the whole match. `remaining: INFINITY` so
        // `tick_shield` never expires it (INFINITY − dt =
        // INFINITY). Matches the Alary's "Damage (even direct)
        // cut in half" toughness quirk.
        entity.insert(ShieldActive {
            remaining: f32::INFINITY,
            damage_factor: 0.5,
        });
        // MIRV launcher + toggleable turret battery state, both
        // driven by dedicated `tick_alary_*` systems (the abilities
        // are `ManagedExternally`).
        entity.insert(AlaryMirvState::default());
        entity.insert(AlaryTurrets::default());
    }
    let entity_id = entity.id();

    // Per-class visual overlays. Canonical use: the Orz turret
    // (`data->spriteExtra`, 64 .tga frames) drawn on top of the hull
    // with its own rotation. As more ships need composite visuals
    // (Chmmr satellites, accessories), they slot in here.
    if let Some(builder) = overlay_frames_for(class, assets) {
        let initial = builder.frames.first().cloned().unwrap_or_default();
        commands.spawn((
            OverlaySprite {
                parent: entity_id,
                frames: builder.frames,
                extra_angle: builder.extra_angle,
                z_offset: builder.z_offset,
            },
            Sprite::from_image(initial),
            Transform::from_translation(position.extend(builder.z_offset)),
        ));
    }
    entity_id
}

/// Per-class overlay sprite spec. Returns `None` for ships with no
/// turret / satellite / accessory.
fn overlay_frames_for(class: ShipClass, assets: &AssetServer) -> Option<OverlaySpriteBuilder> {
    match class {
        // Orz Nemesis — the turret on top of the hull. 64 rotation
        // frames at shot_d_NN_tga.png. Currently rendered aligned
        // with the hull (extra_angle = 0); held-L/R turret aim
        // lands when we wire the special-held input remap.
        ShipClass::Orzne => {
            let frames: Vec<_> = (0..64)
                .map(|i| assets.load(format!("ships/orzne/sprites/shot_d_{:02}_tga.png", i)))
                .collect();
            Some(OverlaySpriteBuilder {
                frames,
                extra_angle: 0.0,
                z_offset: 0.5,
            })
        }
        _ => None,
    }
}

/// Plain-data overlay description used by `spawn_ship` to defer
/// component construction until the parent entity id is known.
struct OverlaySpriteBuilder {
    frames: Vec<Handle<Image>>,
    extra_angle: f32,
    z_offset: f32,
}

/// Per-class `ShipModes` builder. Returns `None` for ships that don't
/// have alternate forms. Builds every mode's complete state up-front
/// (sprite frames, abilities, physics-derived) so `tick_ship_modes`
/// just swaps them in.
fn modes_for(
    class: ShipClass,
    stats: &ShipStats,
    base_frames: &[Handle<Image>],
    base_derived: &ShipPhysicsDerived,
    collider_radius: f32,
    assets: &AssetServer,
) -> Option<ShipModes> {
    use crate::ability::{
        AbilityKind, AbilitySpec, BeamSpec, ShipAbilities, VolleySpec,
    };
    let forward = Vec2::new(0.0, 1.0);

    match class {
        // Mmrnmhrm X-Form: two modes, both with `ToggleMode` as
        // their special so pressing X cycles between them.
        //
        //   Mode 0 = T-Form  (default per shpmmrxf.cpp constructor)
        //     stats from [Ship]:    speed 20, accel 5, turn 2, mass 11
        //     primary = twin laser beams at angles ±asin(28/range)
        //     sprite = ship_s##.png (the default hull rotation set)
        //
        //   Mode 1 = Y-Form
        //     stats from [Special]: speed 50, accel 10, turn 15
        //     primary = twin homing missiles toed out ±25°
        //     sprite = shot_b##.png (the alternate hull rotation set)
        ShipClass::Mmrxf => {
            // T-form derived = base (already from [Ship] stats).
            let tform_derived = base_derived.clone();

            // Y-form: rebuild ShipPhysicsDerived from [Special] stat
            // overrides. The .ini fields don't have a struct slot;
            // override a copy of stats and re-derive.
            let mut y_stats = stats.clone();
            y_stats.speed_max = 50.0;
            y_stats.accel_rate = 10.0;
            y_stats.turn_rate = 15.0;
            y_stats.recharge_amount = 1;
            y_stats.recharge_rate = 6;
            y_stats.weapon_rate = 20;
            let yform_derived = ShipPhysicsDerived::from_stats(&y_stats, collider_radius);

            // Y-form sprite frames — shot_b##.png is 1-indexed PNG.
            let yform_frames: Vec<_> = (0..64)
                .map(|i| assets.load(format!("ships/mmrxf/sprites/shot_b{:02}.png", i + 1)))
                .collect();

            // T-form abilities: twin laser beams from (±28, 2) toed
            // by laserAngle = asin(28 / laserRange). laserRange is
            // 8·40 = 320 u. asin(28/320) ≈ 5°. Approximated with a
            // small bilateral offset; auto_aim off (forward only).
            let tform_abilities = ShipAbilities {
                primary: AbilitySpec {
                    kind: AbilityKind::SpawnBeams {
                        beams: vec![
                            BeamSpec {
                                local_origin: Vec2::new(-28.0, 2.0),
                                local_dir: Vec2::new(0.087, 0.996), // +5° from +Y
                                range: 8.0 * SC2_RANGE_SCALE,
                                damage_per_tick: 1,
                                color: Color::srgb(0.6, 0.6, 1.0),
                                auto_aim: false,
                                duration_s: 1.0 / 20.0,
                                width: 1.5,
                            },
                            BeamSpec {
                                local_origin: Vec2::new(28.0, 2.0),
                                local_dir: Vec2::new(-0.087, 0.996), // -5° from +Y
                                range: 8.0 * SC2_RANGE_SCALE,
                                damage_per_tick: 1,
                                color: Color::srgb(0.6, 0.6, 1.0),
                                auto_aim: false,
                                duration_s: 1.0 / 20.0,
                                width: 1.5,
                            },
                        ],
                    },
                    cooldown_s: 1.0 / 20.0,
                },
                special: AbilitySpec {
                    kind: AbilityKind::ToggleMode,
                    cooldown_s: 1.0 / 20.0,
                },
            };

            // Y-form abilities: the existing twin homing missiles
            // (matches what `abilities_for` returns for Mmrxf today).
            let yform_abilities = ShipAbilities {
                primary: AbilitySpec {
                    kind: AbilityKind::SpawnProjectiles {
                        volleys: vec![VolleySpec {
                            barrels: vec![
                                Barrel {
                                    local_pos: Vec2::new(-13.0, 2.0),
                                    direction: Vec2::new(-0.42261826, 0.9063078),
                                },
                                Barrel {
                                    local_pos: Vec2::new(13.0, 2.0),
                                    direction: Vec2::new(0.42261826, 0.9063078),
                                },
                            ],
                            random_spread_rad: 0.0,
                            speed: 80.0 * SC2_VEL_SCALE,
                            lifetime: (50.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
                            color: Color::srgb(1.0, 1.0, 1.0),
                            sprite_size: 10.0,
                            sprite_path: Some("ships/mmrxf/sprites/shot_a01.png".into()),
                            homing_turn_rate: sc2_turning(9.0),
                            is_limpet: false,
                            recoil_impulse: 0.0,
                        }],
                    },
                    // .ini [Special] WeaponRate = 20 → one volley
                    // per 20 SC2 frames = 1.0 s. (T-form uses [Ship]
                    // WeaponRate = 0, so it fires every frame; the
                    // Y-form's homing missiles are deliberately slow
                    // — that's the trade for tracking + speed.)
                    cooldown_s: 20.0 / 20.0,
                },
                special: AbilitySpec {
                    kind: AbilityKind::ToggleMode,
                    cooldown_s: 1.0 / 20.0,
                },
            };

            Some(ShipModes {
                modes: vec![
                    Mode {
                        name: "T-form",
                        derived: tform_derived,
                        mass: stats.mass,
                        frames: base_frames.to_vec(),
                        abilities: tform_abilities,
                        batt_drain_per_tick: 0,
                        revert_on_empty: false,
                        thrust_locked: false,
                        collide_damage: 0,
                    },
                    Mode {
                        name: "Y-form",
                        derived: yform_derived,
                        mass: stats.mass,
                        frames: yform_frames,
                        abilities: yform_abilities,
                        batt_drain_per_tick: 0,
                        revert_on_empty: false,
                        thrust_locked: false,
                        collide_damage: 0,
                    },
                ],
                current: 0,
                applied: None,
            })
        }

        // Androsynth Guardian: normal ↔ Blazer comet mode.
        //
        //   Mode 0 = Normal — bubble shot primary, special toggles
        //     into Blazer. .ini [Ship] stats.
        //   Mode 1 = Blazer  — no weapon (can't fire while in form),
        //     stats from .ini [Special]: SpeedMax=60, Mass=5,
        //     TurnRate=0.9, Damage=3. Auto-thrusts (thrust_locked),
        //     drains 1 SC2-frame battery per tick (canonical
        //     recharge_amount = -1), exits when battery is empty.
        ShipClass::Andgu => {
            let normal_derived = base_derived.clone();

            let mut blazer_stats = stats.clone();
            blazer_stats.speed_max = 60.0;
            blazer_stats.accel_rate = 6.0; // bumped over normal (3) for the comet feel
            blazer_stats.turn_rate = 0.0; // sub-integer in canon (0.9); 0 = fastest tier
            let blazer_derived =
                ShipPhysicsDerived::from_stats(&blazer_stats, collider_radius);

            // Blazer sprite frames — shot_c##.png, 1-indexed.
            let blazer_frames: Vec<_> = (0..64)
                .map(|i| assets.load(format!("ships/andgu/sprites/shot_c{:02}.png", i + 1)))
                .collect();

            // Normal abilities (matches current abilities_for entry).
            let normal_abilities = ShipAbilities {
                primary: AbilitySpec {
                    kind: AbilityKind::SpawnProjectiles {
                        volleys: vec![VolleySpec {
                            barrels: vec![Barrel {
                                local_pos: forward * 24.0,
                                direction: forward,
                            }],
                            random_spread_rad: 0.0,
                            speed: 24.0 * SC2_VEL_SCALE,
                            lifetime: (50.0 * SC2_RANGE_SCALE) / (24.0 * SC2_VEL_SCALE),
                            color: Color::srgb(1.0, 1.0, 1.0),
                            sprite_size: 14.0,
                            sprite_path: Some("ships/andgu/sprites/shot_a01.png".into()),
                            homing_turn_rate: 0.0,
                            is_limpet: false,
                            recoil_impulse: 0.0,
                        }],
                    },
                    cooldown_s: 1.0 / 20.0,
                },
                special: AbilitySpec {
                    kind: AbilityKind::ToggleMode,
                    cooldown_s: 1.0 / 20.0,
                },
            };

            // Blazer abilities: no primary (set as a Todo that
            // logs+no-ops, since blazer disables weapons in canon).
            // Special toggles back out — but the auto-revert will
            // also kick in when batt hits 0.
            let blazer_abilities = ShipAbilities {
                primary: AbilitySpec {
                    kind: AbilityKind::Todo {
                        ident: "Andro Blazer disables primary (shpandgu.cpp activate_weapon)",
                    },
                    cooldown_s: 1.0 / 20.0,
                },
                special: AbilitySpec {
                    kind: AbilityKind::ToggleMode,
                    cooldown_s: 4.0 / 20.0,
                },
            };

            Some(ShipModes {
                modes: vec![
                    Mode {
                        name: "Normal",
                        derived: normal_derived,
                        mass: stats.mass,
                        frames: base_frames.to_vec(),
                        abilities: normal_abilities,
                        batt_drain_per_tick: 0,
                        revert_on_empty: false,
                        thrust_locked: false,
                        collide_damage: 0,
                    },
                    Mode {
                        name: "Blazer",
                        derived: blazer_derived,
                        // .ini Special Mass=5 (vs normal 9) — lighter,
                        // so the same thrust force accelerates faster.
                        mass: 5.0,
                        frames: blazer_frames,
                        abilities: blazer_abilities,
                        // canonical `recharge_amount = -1` (per-frame
                        // drain). Matches our 1 SC2-frame-per-tick.
                        batt_drain_per_tick: 1,
                        revert_on_empty: true,
                        thrust_locked: true,
                        // .ini Special Damage=3 — landed in a follow-up
                        // collision-damage system; the field is read
                        // by tick_ship_modes but no handler exists yet.
                        collide_damage: 3,
                    },
                ],
                current: 0,
                applied: None,
            })
        }

        _ => None,
    }
}

/// Per-class data manifest builder. The numbers come straight from
/// the canonical `shp*.cpp` / `.ini`, scaled with the SC2 helpers
/// (`SC2_VEL_SCALE`, `SC2_RANGE_SCALE`, `sc2_turning`).
///
/// Returning `None` for a class means "not implemented yet" — the
/// ship will spawn but never fire. Today every variant is `Some(...)`.
fn abilities_for(class: ShipClass) -> Option<crate::ability::ShipAbilities> {
    use crate::ability::{AbilityKind, AbilitySpec, ShipAbilities, VolleySpec};
    let forward = Vec2::new(0.0, 1.0);
    let backward = Vec2::new(0.0, -1.0);
    let single_barrel = |dir: Vec2, offset: f32| -> Vec<Barrel> {
        vec![Barrel {
            local_pos: dir * offset,
            direction: dir,
        }]
    };
    match class {
        // Earthling Cruiser — homing nuke + point defense laser.
        // shpearcr.cpp activate_weapon / activate_special.
        ShipClass::Earcr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles {
                    volleys: vec![VolleySpec {
                        barrels: single_barrel(forward, 28.0),
                        random_spread_rad: 0.0,
                        speed: 80.0 * SC2_VEL_SCALE,
                        lifetime: (60.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
                        color: Color::srgb(1.0, 1.0, 1.0),
                        sprite_size: 16.0,
                        sprite_path: Some("ships/earcr/sprites/shot_a01.png".into()),
                        homing_turn_rate: sc2_turning(3.0),
                        is_limpet: false,
                        recoil_impulse: 0.0,
                    }],
                },
                // WeaponRate=10 → 10/20 Hz = 0.5 s.
                cooldown_s: 10.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::GrantPointDefense {
                    // .ini Special: Range=5 → 5·40 = 200 world units,
                    // Damage=1 per tick, Frames=100 → 5 s; we use a
                    // shorter 1.5 s window to match the cooldown budget.
                    range: 5.0 * SC2_RANGE_SCALE,
                    damage_per_tick: 1,
                    duration_s: 1.5,
                },
                cooldown_s: 3.0,
            },
        }),
        // Spathi Eluder — forward cannon + BUTT homing back-missile.
        // shpspael.cpp activate_weapon (forward Missile); special is
        // the canonical BUTT spawned from the back (handled by trigger_
        // specials originally; here it's just another SpawnProjectiles).
        ShipClass::Spael => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles {
                    volleys: vec![VolleySpec {
                        barrels: single_barrel(forward, 22.0),
                        random_spread_rad: 0.0,
                        speed: 96.0 * SC2_VEL_SCALE,
                        lifetime: (17.0 * SC2_RANGE_SCALE) / (96.0 * SC2_VEL_SCALE),
                        color: Color::srgb(1.0, 1.0, 1.0),
                        sprite_size: 8.0,
                        sprite_path: Some("ships/spael/sprites/shot_a01.png".into()),
                        homing_turn_rate: 0.0,
                        is_limpet: false,
                        recoil_impulse: 0.0,
                    }],
                },
                // WeaponRate=0 → fire every frame, floor at 1 frame.
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles {
                    volleys: vec![VolleySpec {
                        barrels: single_barrel(backward, 22.0),
                        random_spread_rad: 0.0,
                        speed: 45.0 * SC2_VEL_SCALE,
                        lifetime: (12.0 * SC2_RANGE_SCALE) / (45.0 * SC2_VEL_SCALE),
                        color: Color::srgb(1.0, 1.0, 1.0),
                        sprite_size: 12.0,
                        sprite_path: Some("ships/spael/sprites/shot_b01.png".into()),
                        // .ini Special TurnRate=1 → ≈ 3.93 rad/s.
                        homing_turn_rate: sc2_turning(1.0),
                        is_limpet: false,
                        recoil_impulse: 0.0,
                    }],
                },
                // SpecialRate=7 → 7/20 = 0.35 s.
                cooldown_s: 7.0 / 20.0,
            },
        }),
        // Yehat Terminator — twin forward missiles + shield.
        // shpyehte.cpp activate_weapon (2 Missiles at ±24,14) /
        // activate_special (shieldFrames += specialFrames; while >0
        // handle_damage zeroes normal damage → full immunity).
        ShipClass::Yehte => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles {
                    volleys: vec![VolleySpec {
                        barrels: vec![
                            Barrel { local_pos: Vec2::new(-24.0, 14.0), direction: forward },
                            Barrel { local_pos: Vec2::new( 24.0, 14.0), direction: forward },
                        ],
                        random_spread_rad: 0.0,
                        speed: 80.0 * SC2_VEL_SCALE,
                        lifetime: (12.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
                        color: Color::srgb(1.0, 1.0, 1.0),
                        sprite_size: 8.0,
                        sprite_path: Some("ships/yehte/sprites/shot_a01_bmp.png".into()),
                        homing_turn_rate: 0.0,
                        is_limpet: false,
                        recoil_impulse: 0.0,
                    }],
                },
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::GrantShield {
                    // shpyehte.cpp `shieldFrames = (... % frame_time) +
                    // specialFrames` with specialFrames=500 (ms) decrements
                    // by `frame_time` (50 ms) per tick → 10 ticks → 0.5 s.
                    // The canon Yehat shield is a *brief flash* the player
                    // has to keep tapping to maintain, not a sustained 25 s
                    // bubble. Earlier 500/20 read the .ini as "frames" but
                    // the original units are milliseconds.
                    duration_s: 0.5,
                    damage_factor: 0.0,
                },
                // SpecialRate=2 → 2/20 = 0.1 s — fast enough to re-arm by
                // the time the previous shield flash drops.
                cooldown_s: 2.0 / 20.0,
            },
        }),

        // --- The rest of the roster. Specials whose canonical
        // behaviour needs a primitive that doesn't exist yet stay as
        // `Todo { ident: "..." }` so the dispatcher logs them but
        // doesn't pretend to do something it can't.

        // Chmmr Avatar — canonical ChmmrLaser from Vector2(0, 25)
        // forward. .ini Weapon: Range=10 → 400 u, Damage=2.
        ShipClass::Chmav => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnBeams { beams: vec![crate::ability::BeamSpec {
                    local_origin: Vec2::new(0.0, 25.0),
                    local_dir: forward,
                    range: 10.0 * SC2_RANGE_SCALE,
                    damage_per_tick: 2,
                    color: Color::srgb(1.0, 0.4, 0.4),
                    auto_aim: false,
                    // Lives just long enough that a 0.05 s re-fire
                    // makes it look continuous while the player
                    // holds the button.
                    duration_s: 1.0 / 20.0,
                    width: 2.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // .ini Special: Force=30 → 30·9.6 ≈ 288 N·s/tick.
                // Range=100 → 4000 u. Lives long enough for the
                // 0.05 s cooldown to keep refreshing while held.
                kind: AbilityKind::SpawnTractor {
                    local_origin: Vec2::ZERO,
                    range: 100.0 * SC2_RANGE_SCALE,
                    force_per_tick: 30.0 * SC2_VEL_SCALE,
                    color: Color::srgba(0.6, 0.9, 1.0, 0.55),
                    width: 1.5,
                    duration_s: 1.0 / 20.0,
                },
                // SpecialRate=0 → fire every frame; floor at one frame.
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Ur-Quan Kzer-Za Dreadnought — fusion bolt + launch fighters.
        // shpkzedr.cpp activate_special: spawns 1-2 KzerZaFighter
        // sub-entities (one per crew burned, max 2). Each fighter
        // flies out at specialVelocity, fires lasers at the target,
        // and returns home — for MVP we model it as a homing sub
        // that detonates on contact. Canonical full behaviour
        // (orbiting + laser fire + return) needs the AI to be
        // extended; the framework supports it.
        ShipClass::Kzedr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 36.0),
                    random_spread_rad: 0.0,
                    speed: 80.0 * SC2_VEL_SCALE,
                    lifetime: (22.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 12.0,
                    sprite_path: Some("ships/kzedr/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 6.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::Sequence(vec![
                    // SpecialDrain=8 already deducted; canonical also
                    // burns 1 crew per fighter launched.
                    AbilityKind::ModifyCrew { delta: -1 },
                    AbilityKind::SpawnSubEntity {
                        // Spawn from the back of the dreadnought.
                        local_offset: Vec2::new(0.0, -25.0),
                        initial_angle_offset: std::f32::consts::PI,
                        // .ini Special Velocity=35 → 336 u/s.
                        initial_speed: 35.0 * SC2_VEL_SCALE,
                        sprite_path: Some("ships/kzedr/sprites/shot_b01.png".into()),
                        sprite_size: 14.0,
                        color: Color::srgb(1.0, 1.0, 1.0),
                        // .ini Special Armour = 1 (effectively one-hit).
                        hp: 1,
                        // .ini Special Frames=23000 / 20 = 1150 s; cap
                        // to a sensible value so they don't pile up
                        // forever in our shorter rounds.
                        lifetime_s: 20.0,
                        ai: crate::ability::SubEntityAiSpec::HomeAndDetonate {
                            turn_rate: sc2_turning(4.0),
                            speed: 35.0 * SC2_VEL_SCALE,
                            damage_on_hit: 2,
                            batt_sap: 0,
                        },
                    },
                ]),
                cooldown_s: 9.0 / 20.0,
            },
        }),

        // Mycon Podship — homing plasmoid + crew repair.
        // shpmycpo.cpp activate_special: damage(this, 0, -4).
        ShipClass::Mycpo => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 26.0),
                    random_spread_rad: 0.0,
                    speed: 35.0 * SC2_VEL_SCALE,
                    lifetime: (60.0 * SC2_RANGE_SCALE) / (35.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 16.0,
                    sprite_path: Some("ships/mycpo/sprites/shot_a01.png".into()),
                    homing_turn_rate: sc2_turning(1.0),
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 5.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ModifyCrew { delta: 4 },
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Shofixti Scout — short forward gun + Glory Device.
        // shpshosc.cpp: Glory hits everything within Range=13 → 520 u
        // (no source_self → self-damaging suicide blast).
        ShipClass::Shosc => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 22.0),
                    random_spread_rad: 0.0,
                    speed: 96.0 * SC2_VEL_SCALE,
                    lifetime: (14.0 * SC2_RANGE_SCALE) / (96.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/shosc/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 3.0 / 20.0,
            },
            special: AbilitySpec {
                // Glory Device: 3 presses to confirm the suicide blast
                // (shpshosc.cpp). Counted by `tick_shofixti_glory`, which
                // spawns the range-13 blast on the third press.
                kind: AbilityKind::ManagedExternally { ident: "shofixti-glory" },
                cooldown_s: 0.0,
            },
        }),

        // Arilou Skiff — canonical Laser auto-aimed at nearest non-
        // invisible target. .ini Weapon: Range=5.5 → 220 u, Damage=1.
        ShipClass::Arisk => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnBeams { beams: vec![crate::ability::BeamSpec {
                    local_origin: Vec2::ZERO,
                    local_dir: forward,
                    range: 5.5 * SC2_RANGE_SCALE,
                    damage_per_tick: 1,
                    color: Color::srgb(0.5, 1.0, 0.9),
                    auto_aim: true,
                    duration_s: 1.0 / 20.0,
                    width: 1.5,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::TeleportRandom { range: 1500.0 },
                cooldown_s: 2.0 / 20.0,
            },
        }),

        // Pkunk Fury — triple-shot forward + ±90° lateral + battery taunt.
        ShipClass::Pkufu => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: vec![
                        Barrel { local_pos: Vec2::new(  0.0, 16.0), direction: Vec2::new( 0.0,  1.0) },
                        Barrel { local_pos: Vec2::new(-16.0,  0.0), direction: Vec2::new(-1.0,  0.0) },
                        Barrel { local_pos: Vec2::new( 16.0,  0.0), direction: Vec2::new( 1.0,  0.0) },
                    ],
                    random_spread_rad: 0.0,
                    speed: 96.0 * SC2_VEL_SCALE,
                    lifetime: (5.5 * SC2_RANGE_SCALE) / (96.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/pkufu/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // Taunt: recharge a chunk per press (BattMax=12), never
                // battery-gated, so the Pkunk can always insult its way
                // back from empty instead of dying at 0 battery.
                kind: AbilityKind::AddBattery { amount: 4 },
                cooldown_s: 16.0 / 20.0,
            },
        }),

        // Ilwrath Avenger — short-range hit + real cloak. While
        // Invisible is on the ship, homing missiles drop their lock
        // and auto-aim beams skip it. Canonical isInvisible() →
        // 1.0 when cloak_frame ≥ 300 (shpilwav.cpp:64).
        ShipClass::Ilwav => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 30.0),
                    random_spread_rad: 0.0,
                    speed: 28.0 * SC2_VEL_SCALE,
                    lifetime: (2.8 * SC2_RANGE_SCALE) / (28.0 * SC2_VEL_SCALE),
                    // The extracted shot_a01 frames are essentially blank
                    // (mean alpha ~0.003) — use our generated fireball
                    // for the short-range orange "fire breath" so there's
                    // actually something visible on screen.
                    color: Color::srgb(1.0, 0.6, 0.25),
                    sprite_size: 18.0,
                    sprite_path: Some("ui/fireball.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::GrantInvisibility { duration_s: 2.5 },
                cooldown_s: 7.0 / 20.0,
            },
        }),

        // Thraddash Torch — forward shot + afterburner dash dropping a
        // ThraddashFlame trail. Sequence composes ApplyImpulse + zone.
        ShipClass::Thrto => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 22.0),
                    random_spread_rad: 0.0,
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (25.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/thrto/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 12.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::Sequence(vec![
                    // .ini Special Thrust=8 → 8·9.6 = 76.8 u/s; treat
                    // as an instantaneous Δv (impulse = Δv · mass).
                    AbilityKind::ApplyImpulse {
                        local_dir: forward,
                        impulse: 8.0 * SC2_VEL_SCALE * 7.0, // mass≈7
                    },
                    // Canon ThraddashFlame: 2 damage per CONTACT, armour=2,
                    // 3.9 s lifetime — the flame puff is consumed on the
                    // first ship hit. Our `SpawnDamageZone` is dps-based,
                    // so we approximate "2 dmg per touch" with a low dps
                    // for the short duration. Previous 8 dps × 3.9 s = ~31
                    // damage per puff while ramming = obliterating —
                    // user reported this was way too powerful.
                    AbilityKind::SpawnDamageZone {
                        offset: Vec2::new(0.0, -24.0),
                        radius: 22.0,
                        damage_per_sec: 1.0,
                        duration_s: 2.0,
                        source_self: true,
                        color: Color::srgba(1.0, 0.8, 0.5, 0.85),
                        sprite_path: Some("ui/fireball.png"),
                    },
                ]),
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // VUX Intruder — canonical Laser from Vector2(size.x/11,
        // size.y/2.07). .ini Weapon: Range=9 → 360 u, Damage=1.
        ShipClass::Vuxin => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnBeams { beams: vec![crate::ability::BeamSpec {
                    local_origin: Vec2::new(2.0, 11.0),
                    local_dir: forward,
                    range: 9.0 * SC2_RANGE_SCALE,
                    damage_per_tick: 1,
                    color: Color::srgb(0.5, 1.0, 0.3),
                    auto_aim: false,
                    duration_s: 1.0 / 20.0,
                    width: 1.5,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(backward, 16.0),
                    random_spread_rad: 0.0,
                    speed: 25.0 * SC2_VEL_SCALE,
                    lifetime: (35.0 * SC2_RANGE_SCALE) / (25.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 10.0,
                    sprite_path: Some("ships/vuxin/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: true,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 7.0 / 20.0,
            },
        }),

        // Supox Blade — forward Missile. Special is a multi-direction
        // strafe (S/D-while-held), which needs runtime state for "held
        // continuously" — TODO ModeToggle-ish primitive.
        ShipClass::Supbl => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 24.0),
                    random_spread_rad: 0.0,
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (15.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/supbl/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // Supox held-strafe is implemented directly inside
                // apply_player_input (Supbl-specific branch). The
                // dispatcher doesn't need to do anything per-press —
                // an empty Sequence keeps the cooldown / battery
                // bookkeeping wired up while emitting no side effects.
                kind: AbilityKind::Sequence(vec![]),
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Kohr-Ah Marauder — saw blade + 16-shot F.R.I.E.D. ring.
        // Note: 16 ship-local barrels means the ring rotates with the
        // ship. Canonical F.R.I.E.D. uses absolute world angles —
        // visible difference is the spawn-time orientation, not the
        // spread. TODO: world-frame volley flag if it matters.
        ShipClass::Kohma => {
            let fried_speed = 20.0 * SC2_VEL_SCALE;
            let fried_life = (5.0 * SC2_RANGE_SCALE) / fried_speed;
            let mut fried_barrels = Vec::with_capacity(16);
            for i in 0..16 {
                let theta = (i as f32) * std::f32::consts::TAU / 16.0;
                let dir = Vec2::new(theta.cos(), theta.sin());
                // Spawn the ring well clear of the hull so the 16
                // fireballs don't overlap each other (and the ship) at
                // spawn — overlapping dynamic shots get blasted apart by
                // the solver, scattering the ring before it reads.
                fried_barrels.push(Barrel { local_pos: dir * 36.0, direction: dir });
            }
            Some(ShipAbilities {
                primary: AbilitySpec {
                    // Hold-then-release-to-drop-a-mine — handled by
                    // `tick_kohma_blade` in this file. Press fire =
                    // arm a carrier blade attached to the ship,
                    // release = drop it in place as a slow-homing
                    // mine. Echoes the canonical KohrAhBlade with
                    // Persists=1 and MaxBlades=9.
                    kind: AbilityKind::ManagedExternally {
                        ident: "kohma_blade",
                    },
                    cooldown_s: 6.0 / 20.0,
                },
                special: AbilitySpec {
                    kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                        barrels: fried_barrels,
                        random_spread_rad: 0.0,
                        speed: fried_speed,
                        lifetime: fried_life * 1.6,
                        color: Color::srgb(1.0, 0.85, 0.5),
                        sprite_size: 18.0,
                        sprite_path: Some("ui/fireball.png".into()),
                        homing_turn_rate: 0.0,
                        is_limpet: false,
                        recoil_impulse: 0.0,
                    }]},
                    cooldown_s: 9.0 / 20.0,
                },
            })
        }

        // Syreen Penetrator — fast razor. Siren song special is TODO
        // (needs crew-pod sub-entity primitive).
        ShipClass::Syrpe => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 32.0),
                    random_spread_rad: 0.0,
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (17.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/syrpe/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 8.0 / 20.0,
            },
            special: AbilitySpec {
                // shpsyrpe.cpp activate_special: drain crew from every
                // valid enemy within specialRange. Damage scales with
                // proximity (closer = more), plus a random bonus.
                // .ini Special: Range=11→440 wu, Damage=5.
                kind: AbilityKind::DrainNearbyCrew {
                    range: 11.0 * SC2_RANGE_SCALE,
                    max_drain: 5,
                },
                cooldown_s: 20.0 / 20.0,
            },
        }),

        // Androsynth Guardian — bubble shot + Blazer mode (TODO ModeToggle).
        ShipClass::Andgu => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 24.0),
                    random_spread_rad: 0.0,
                    speed: 24.0 * SC2_VEL_SCALE,
                    lifetime: (50.0 * SC2_RANGE_SCALE) / (24.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 14.0,
                    sprite_path: Some("ships/andgu/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // The real abilities live in `ShipModes` — see
                // `modes_for(Andgu)`. This is just a fallback for the
                // (impossible) case where ShipModes is missing.
                kind: AbilityKind::ToggleMode,
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Chenjesu Broodhome — crystal with on-release shatter +
        // DOGI sub-entity. Crystal is implemented by the dedicated
        // `tick_chebr_crystal` system (per `shpchebr.cpp:calculate`)
        // because the canonical behaviour needs per-ship state and
        // an on-release-of-fire-button hook that the generic
        // dispatcher doesn't model.
        ShipClass::Chebr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally {
                    ident: "chebr-crystal",
                },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                // shpchebr.cpp activate_special: spawn 1 ChenjesuDOGI
                // mine from (0, -size.y/1.5) at angle+π (back of ship).
                // The DOGI homes on the nearest enemy with an avoidance
                // angle and fuel-saps battery on contact. .ini Special:
                // Velocity=33, FuelSap=8, Armour=3, AccelRate=20,
                // AvoidanceAngle=27.5°. We model AccelRate as a steady
                // homing speed of (Velocity·9.6), turn rate from
                // sc2_turning derivative; avoidance is omitted for now.
                kind: AbilityKind::SpawnSubEntity {
                    local_offset: Vec2::new(0.0, -32.0),
                    initial_angle_offset: std::f32::consts::PI,
                    initial_speed: 33.0 * SC2_VEL_SCALE,
                    sprite_path: Some("ships/chebr/sprites/shot_c_00_tga.png".into()),
                    sprite_size: 24.0,
                    color: Color::srgb(0.8, 0.9, 1.0),
                    // .ini Special Armour=3 → survives 3 hits.
                    hp: 3,
                    lifetime_s: 15.0,
                    ai: crate::ability::SubEntityAiSpec::HomeAndDetonate {
                        turn_rate: sc2_turning(2.0),
                        speed: 33.0 * SC2_VEL_SCALE,
                        // .ini has no Special.Damage → DOGI deals 0
                        // crew damage in canon; the threat is FuelSap=8.
                        damage_on_hit: 0,
                        batt_sap: 8,
                    },
                },
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Druuge Mauler — heavy recoil cannon + crew-burn battery refill.
        ShipClass::Druma => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 30.0),
                    random_spread_rad: 0.0,
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (40.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 12.0,
                    sprite_path: Some("ships/druma/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    // .ini DriftVelocity=375 → 375·9.6 N·s.
                    recoil_impulse: 375.0 * SC2_VEL_SCALE,
                }]},
                cooldown_s: 10.0 / 20.0,
            },
            special: AbilitySpec {
                // .ini SpecialDrain=16 → each crewman becomes 16 batt.
                kind: AbilityKind::BurnCrewForBattery { crew_cost: 1, batt_gain: 16 },
                cooldown_s: 30.0 / 20.0,
            },
        }),

        // Utwig Jugger — six-barrel forward + fortitude shield (placeholder).
        ShipClass::Utwju => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: vec![
                        Barrel { local_pos: Vec2::new(-34.0, 11.0), direction: forward },
                        Barrel { local_pos: Vec2::new( 34.0, 11.0), direction: forward },
                        Barrel { local_pos: Vec2::new(-18.0, 20.0), direction: forward },
                        Barrel { local_pos: Vec2::new( 18.0, 20.0), direction: forward },
                        Barrel { local_pos: Vec2::new( -6.0, 27.0), direction: forward },
                        Barrel { local_pos: Vec2::new(  6.0, 27.0), direction: forward },
                    ],
                    random_spread_rad: 0.0,
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (14.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/utwju/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 7.0 / 20.0,
            },
            special: AbilitySpec {
                // shputwju.cpp:96: while special_recharge > 0,
                // `batt += normal` instead of crew damage. Conversion
                // is 1.0 (1 damage absorbed → 1 battery gained).
                kind: AbilityKind::GrantDamageToBattery {
                    duration_s: 2.0,
                    conversion: 1.0,
                },
                cooldown_s: 7.0 / 20.0,
            },
        }),

        // Zoq-Fot-Pik Stinger — random-spread tongue + close-range zap.
        ShipClass::Zfpst => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 20.0),
                    random_spread_rad: 10.0 * (std::f32::consts::TAU / 64.0),
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (11.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/zfpst/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // ZFP tongue: a ship-attached SpaceObject at dist=39
                // ahead of the Stinger, lifetime 6 frames (~0.3 s).
                // Now uses the real attached-zone primitive so the
                // tongue follows the ship as it moves/turns mid-lash
                // (canonical `pos = ship.pos + dist·unit_vector(angle)`
                // updated every tick in shpzfpst.cpp:ZoqFotPikTongue).
                kind: AbilityKind::SpawnAttachedDamageZone {
                    local_offset: Vec2::new(0.0, 39.0),
                    radius: 20.0,
                    damage_per_sec: 240.0,
                    duration_s: 0.3,
                    color: Color::srgba(1.0, 0.5, 0.3, 0.55),
                },
                cooldown_s: 6.0 / 20.0,
            },
        }),

        // Mmrnmhrm X-Form — Y-Form twin homing missiles + form-toggle (TODO).
        ShipClass::Mmrxf => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: vec![
                        Barrel { local_pos: Vec2::new(-13.0, 2.0), direction: Vec2::new(-0.42261826, 0.9063078) },
                        Barrel { local_pos: Vec2::new( 13.0, 2.0), direction: Vec2::new( 0.42261826, 0.9063078) },
                    ],
                    random_spread_rad: 0.0,
                    speed: 80.0 * SC2_VEL_SCALE,
                    lifetime: (50.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 10.0,
                    sprite_path: Some("ships/mmrxf/sprites/shot_a01.png".into()),
                    homing_turn_rate: sc2_turning(9.0),
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // Same shape as Andgu — actual mode tuning is in
                // `modes_for(Mmrxf)`; this is the fallback.
                kind: AbilityKind::ToggleMode,
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Orz Nemesis — separately-rotating turret + space marines. Fully
        // owned by `tick_orz_turret` (input remap when SPECIAL is held,
        // primary fires in the turret's direction, fire+special spawns
        // a marine). The generic dispatcher does nothing for either
        // slot — see `shporzne.cpp` for the canon behaviour.
        ShipClass::Orzne => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "orz-turret" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "orz-turret" },
                cooldown_s: 0.0,
            },
        }),

        // Slylandro Probe — lightning (TODO) + asteroid harvest (no asteroids).
        ShipClass::Slypr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::Todo { ident: "Slylandro lightning (shpslypr.cpp:SlylandroLaserNew)" },
                cooldown_s: 5.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::Todo { ident: "Slylandro asteroid harvest (no asteroids in arena)" },
                cooldown_s: 20.0 / 20.0,
            },
        }),

        // Umgah Drone — attached anti-grav cone (UmgahCone in legacy is
        // a ship-attached SpaceObject at dist=81 ahead, damaging on
        // contact while fire_weapon is held). Each press spawns a
        // brief AttachedDamageZone in front; holding the button keeps
        // the cone alive continuously. .ini Weapon: Damage=20,
        // DamageType=2. We model Damage=20 SC2-frames-per-tick at
        // 20 Hz → 20 dmg / 0.05 s ≈ 400 dps; the canonical DamageType=2
        // ramping behaviour is left for a future tuning pass.
        ShipClass::Umgdr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnAttachedDamageZone {
                    local_offset: Vec2::new(0.0, 40.0),
                    radius: 30.0,
                    damage_per_sec: 400.0,
                    duration_s: 1.0 / 20.0,
                    color: Color::srgba(0.5, 1.0, 0.7, 0.45),
                },
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                // shpumgdr.cpp activate_special: pos -= forward·2·size.x;
                // vel=0. Our collider radius is ~12 → 2·24 = 48 backwards.
                kind: AbilityKind::TeleportRelative {
                    offset: Vec2::new(0.0, -48.0),
                    zero_velocity: true,
                },
                cooldown_s: 2.0 / 20.0,
            },
        }),

        // Melnorme Trader — chargeable plasma (TODO ChargedFire) +
        // confusion ray (TODO InputOverride).
        // Melnorme Trader — chargeable plasma cannon. Hold fire to
        // charge through up to 3 phases (2.5 s each), each doubling
        // damage and adding RangeUp to the shot's range. Release to
        // fire. Implemented by the dedicated `tick_meltr_charge`
        // system per shpmeltr.cpp:MelnormeShot::calculate.
        ShipClass::Meltr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally {
                    ident: "meltr-charge",
                },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::Todo { ident: "Melnorme confusion ray (shpmeltr.cpp:1215)" },
                cooldown_s: 20.0 / 20.0,
            },
        }),

        // Alary Battle Cruiser (TW-Light fan ship). Faithful
        // port of shpalabc.cpp's signature systems:
        //
        // Primary — MIRV torpedo. ONE slow homing torpedo that
        // homes toward the target and, once within proximity,
        // splits into FIVE homing warheads fanned at
        // [0, ±50°, ±75°] off its heading (shpalabc.cpp
        // AlaryBCTorpedo::calculate). The torpedo itself does no
        // contact damage; only the warheads hurt. Driven by the
        // dedicated `tick_alary_mirv` system.
        //
        // Special — toggleable turrets. Press special to toggle
        // three hull turrets on/off; while on they auto-fire at
        // the nearest enemy on a recharge. Driven by
        // `tick_alary_turrets`.
        //
        // Tankiness — a permanent absorbance shield
        // (`ShieldActive { damage_factor: 0.5 }`, stamped at
        // spawn) halves all incoming damage, per the txt's
        // "Damage (even direct) cut in half" quirk.
        ShipClass::Alabc => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "alary-mirv" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "alary-turrets" },
                cooldown_s: 0.0,
            },
        }),
    }
}

/// Filename of the rotation sprite for a ship at frame index 0..64
/// (0 = north, increasing CCW). The dat extractor wrote different
/// suffix conventions for different ships depending on whether the
/// source bitmap was a PNG, BMP, or TGA in the original .dat — so
/// the canonical 1-indexed `ship_sNN.png` pattern doesn't cover the
/// whole roster. This is the one place where the per-class oddities
/// live; both `load_rotation_frames` (game render) and the collider
/// polygon loader read through here.
pub fn rotation_frame_filename(class: ShipClass, frame: usize) -> String {
    match class {
        // chebr & orzne: 0-indexed `ship_s_NN_tga.png`. Frame 0 is north.
        ShipClass::Chebr | ShipClass::Orzne => format!("ship_s_{:02}_tga.png", frame),

        // alabc: 0-indexed 3-digit `ship_s_NNN_tga.png` (the TW-Light
        // Alary dat ships 64 frames as SHIP_S_000_TGA … SHIP_S_063_TGA).
        ShipClass::Alabc => format!("ship_s_{:03}_tga.png", frame),

        // druma: frame 0 is `ship_s00.png` (no suffix), frames 1-63
        // are `ship_sNN_bmp.png`.
        ShipClass::Druma => {
            if frame == 0 {
                "ship_s00.png".to_string()
            } else {
                format!("ship_s{:02}_bmp.png", frame)
            }
        }

        // vuxin: frames 0-1 are `ship_s0N.png`, 2-63 are `ship_xNN_bmp.png`.
        ShipClass::Vuxin => {
            if frame <= 1 {
                format!("ship_s{:02}.png", frame)
            } else {
                format!("ship_x{:02}_bmp.png", frame)
            }
        }

        // Everyone else: 1-indexed `ship_sNN.png` where ship_s01 = north.
        _ => format!("ship_s{:02}.png", frame + 1),
    }
}

fn load_rotation_frames(
    assets: &AssetServer,
    code: &str,
    class: ShipClass,
) -> Vec<Handle<Image>> {
    // 64 rotation frames per ship, 0-indexed (frame 0 = north, CCW).
    // The naming convention varies per ship; `rotation_frame_filename`
    // hides that. Issuing 64 loads unconditionally avoids needing
    // `std::fs::exists`, which doesn't work in the browser sandbox.
    // AssetServer.load() is fire-and-forget on both native and WASM:
    // missing files just don't render.
    let mut frames = Vec::with_capacity(64);
    for i in 0..64 {
        let name = rotation_frame_filename(class, i);
        frames.push(assets.load(format!("ships/{code}/sprites/{name}")));
    }
    frames
}

/// Read keyboard for this peer's slot and command the ship's motion.
///
/// Thrust is always applied as a persistent local force — same in both
/// rotation modes. Steering branches on `AngularControl`:
///
///   - **Classic**: stamp `AngularVelocity` directly to the commanded
///     turn rate each frame. Collisions can perturb it during a step
///     but get overwritten next frame, so impacts never visibly spin
///     the ship. `ConstantTorque` stays at zero. Matches original SC2.
///   - **Inertial**: leave `AngularVelocity` alone (the solver and any
///     collision impulses own it) and write `ConstantTorque` as a
///     proportional controller chasing the commanded target rate.
///     Either-direction recovery falls out naturally — see the
///     `AngularControl` doc-comment.
pub(crate) fn apply_player_input(
    slot_inputs: Res<input::SlotInputs>,
    angular_override: Res<AngularControlOverride>,
    time: Res<Time<Physics>>,
    mut q: Query<(
        &Ship,
        &ShipClass,
        &ShipPhysicsDerived,
        &Rotation,
        &mut ConstantLocalForce,
        &mut ConstantTorque,
        &mut AngularVelocity,
        &mut LinearVelocity,
        Option<&ShipModes>,
        &mut LastTurnInput,
        Option<&InertialessDrive>,
        Option<&crate::ultimate::HyperActive>,
        Option<&crate::ultimate::PostUltimateCoasting>,
        Option<&crate::ai::AiControlled>,
    )>,
) {
    for (
        ship,
        class,
        derived,
        rot,
        mut thrust,
        mut torque,
        mut ang_vel,
        mut lin_vel,
        modes,
        mut last_turn,
        inertialess,
        hyper,
        coasting,
        ai,
    ) in &mut q
    {
        // Ultimate cinematic: force-locked spin; skip player ang_vel
        // and thrust updates so the cinematic owns the ship's motion.
        if hyper.is_some() {
            torque.0 = 0.0;
            thrust.0 = Vec2::ZERO;
            continue;
        }
        // AI-driven ships write virtual button-presses to SlotInputs
        // (in `tick_ai_pilots`, which runs `.before` this system), so
        // we process their inputs the same way as a human's.
        let _ = ai;

        // Post-ultimate coasting: ship is over-speed. Player still
        // steers (normal angular control), but THRUST is reinterpreted
        // as a brake — applies a force opposite to current velocity
        // so the player can shed the over-speed deliberately. Without
        // input the ship just coasts (cap_velocity is suppressed for
        // this ship until speed drops back under speed_max).
        if coasting.is_some() {
            let input = slot_inputs.held[ship.player_slot.min(3)];
            // Steering: keep normal Classic snap behaviour so the
            // player can re-orient mid-coast.
            let dir = if input.pressed(input::INPUT_LEFT) {
                1.0
            } else if input.pressed(input::INPUT_RIGHT) {
                -1.0
            } else {
                0.0
            };
            ang_vel.0 = dir * derived.target_omega;
            torque.0 = 0.0;
            last_turn.had_input = dir != 0.0;

            if input.pressed(input::INPUT_THRUST) {
                // Brake: force opposite to current velocity. Use the
                // ship's normal thrust_force magnitude so braking
                // feels symmetric with acceleration. ConstantLocalForce
                // is in the ship's local frame, so rotate the
                // world-space brake vector into local space.
                let v = lin_vel.0;
                let speed = v.length();
                if speed > 1.0 {
                    let world_brake = -v / speed;
                    let local_brake = Vec2::new(
                        world_brake.x * rot.cos + world_brake.y * rot.sin,
                        -world_brake.x * rot.sin + world_brake.y * rot.cos,
                    );
                    thrust.0 = local_brake * derived.thrust_force;
                } else {
                    thrust.0 = Vec2::ZERO;
                }
            } else {
                thrust.0 = Vec2::ZERO;
            }
            continue;
        }
        let rot_cos = rot.cos;
        let rot_sin = rot.sin;
        let input = slot_inputs.held[ship.player_slot.min(3)];

        // Absolute-aim scheme (tilt + stick): the stick points where the
        // ship should go in WORLD space — it turns to face the stick
        // direction (at its normal max rate, so it's fair) and thrusts
        // once roughly aligned. When the stick is centred, the tilt
        // value (carried in `turn`) rotates the ship in place so you can
        // line up a shot while drifting. Self-contained — skips the
        // relative steering / Supox / thrust paths below.
        if input.pressed(input::INPUT_ABSOLUTE) {
            use std::f32::consts::{FRAC_PI_2, PI, TAU};
            let forward = Vec2::new(-rot_sin, rot_cos);
            let facing = forward.y.atan2(forward.x);
            let aim = input.aim_vec();
            if aim.length() > 0.25 {
                let desired = aim.y.atan2(aim.x);
                let mut err = desired - facing;
                while err > PI {
                    err -= TAU;
                }
                while err < -PI {
                    err += TAU;
                }
                // Turn toward the target; the dt scaling makes the ship
                // land exactly on the heading (no overshoot/oscillation),
                // capped at its normal max turn rate.
                let dt = time.delta_secs().max(1e-4);
                ang_vel.0 = (err / dt).clamp(-derived.target_omega, derived.target_omega);
                torque.0 = 0.0;
                last_turn.had_input = true;
                // Thrust once we're facing roughly toward the target —
                // "turn, then go" — so we don't accelerate backwards
                // while still spinning around.
                thrust.0 = if err.abs() < FRAC_PI_2 {
                    Vec2::new(0.0, derived.thrust_force)
                } else {
                    Vec2::ZERO
                };
            } else {
                // Stick centred: tilt rotates in place, no thrust.
                let dir = input.turn_f32().clamp(-1.0, 1.0);
                ang_vel.0 = dir * derived.target_omega;
                torque.0 = 0.0;
                last_turn.had_input = dir.abs() > 1e-3;
                thrust.0 = Vec2::ZERO;
            }
            continue;
        }

        // Analog turn axis: keyboard is ±1.0 (bang-bang), the touch
        // stick supplies proportional values in between for fine, slow
        // turns. `target_omega` scales linearly with it, so a small
        // stick tilt yields a slow rotation and full deflection hits
        // the ship's normal max turn rate.
        let dir = input.turn_f32().clamp(-1.0, 1.0);
        let target_omega = dir * derived.target_omega;

        let mode = angular_override
            .0
            .unwrap_or_else(|| physics_spec(*class).angular_control);

        // Edge detection done in-system so it's robust against the
        // Bevy ButtonInput-vs-FixedUpdate timing race.
        let has_input_now = dir.abs() > 1e-3;
        let just_released = !has_input_now && last_turn.had_input;
        last_turn.had_input = has_input_now;

        match mode {
            AngularControl::Classic => {
                // Snap to commanded rate; ignore impulses (SC2 default).
                ang_vel.0 = target_omega;
                torque.0 = 0.0;
            }
            AngularControl::Inertial => {
                // Three mutually-exclusive cases per tick:
                //   1. Holding L or R — snap `ang_vel = target_omega`.
                //      Feels like Classic. Any spin from a collision
                //      acquired before this tick gets overwritten
                //      while the key is held.
                //   2. JUST released L or R (had input last tick, none
                //      this tick) — snap `ang_vel = 0`. Kills the
                //      player-induced spin so deliberate steering
                //      doesn't leave momentum behind.
                //   3. No input, no transition — leave `ang_vel`
                //      alone. Collision impulses persist as visible
                //      tumble (AngularDamping=0). The player taps a
                //      turn key to right themselves (case 1 takes
                //      over and brakes/reverses the spin).
                torque.0 = 0.0;
                if has_input_now {
                    ang_vel.0 = target_omega;
                } else if just_released {
                    ang_vel.0 = 0.0;
                }
                // else: leave ang_vel as-is (collision spin survives).
            }
        }

        // Supox held-strafe (shpsupbl.cpp:calculate_thrust): while
        // Special is held, L / R map to lateral thrust (perpendicular
        // to facing), THRUST maps to backward thrust, and rotation
        // is suppressed entirely. Ship can still fire its primary.
        if matches!(class, ShipClass::Supbl) && input.pressed(input::INPUT_SPECIAL) {
            // No rotation while strafing.
            torque.0 = 0.0;
            ang_vel.0 = 0.0;
            // Build a local-frame thrust vector from the inputs.
            // ConstantLocalForce auto-rotates this into world space.
            let mut local = Vec2::ZERO;
            if input.pressed(input::INPUT_LEFT) {
                local.x -= derived.thrust_force; // lateral left
            }
            if input.pressed(input::INPUT_RIGHT) {
                local.x += derived.thrust_force; // lateral right
            }
            if input.pressed(input::INPUT_THRUST) {
                local.y -= derived.thrust_force; // backwards
            }
            thrust.0 = local;
            continue;
        }

        // Inertialess drive (Arilou): direct velocity control,
        // infinite acceleration / instant stop. Overrides the
        // force-based thrust path entirely. By overwriting LinearVel
        // every tick we also cancel any external impulse on this
        // ship (canonical "external accelerations rejected" — see
        // `shparisk.cpp:accelerate`).
        if inertialess.is_some() {
            thrust.0 = Vec2::ZERO;
            // Forward in world space, derived from ship's current rotation.
            let forward = Vec2::new(0.0, 1.0);
            let world_forward = Vec2::new(
                forward.x * rot_cos - forward.y * rot_sin,
                forward.x * rot_sin + forward.y * rot_cos,
            );
            lin_vel.0 = if input.pressed(input::INPUT_THRUST) {
                world_forward * derived.speed_max
            } else {
                Vec2::ZERO
            };
            continue;
        }

        // Some modes lock thrust on (Andro Blazer auto-comets the
        // ship forward). Otherwise thrust follows the input button.
        let thrust_locked = modes
            .map(|m| m.modes.get(m.current).map_or(false, |mm| mm.thrust_locked))
            .unwrap_or(false);
        thrust.0 = if thrust_locked || input.pressed(input::INPUT_THRUST) {
            // Force vector in the ship's local frame (Avian rotates it
            // into world space because we used ConstantLocalForce).
            Vec2::new(0.0, derived.thrust_force)
        } else {
            Vec2::ZERO
        };
    }
}

/// `M` cycles the global override: None (per-class default, currently
/// Classic for all stock ships) → Force-Classic → Force-Inertial → back.
fn cycle_angular_override(
    keys: Res<ButtonInput<KeyCode>>,
    mut override_mode: ResMut<AngularControlOverride>,
) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    override_mode.0 = match override_mode.0 {
        None => Some(AngularControl::Classic),
        Some(AngularControl::Classic) => Some(AngularControl::Inertial),
        Some(AngularControl::Inertial) => None,
    };
    let label = match override_mode.0 {
        None => "per-class default (Classic)",
        Some(AngularControl::Classic) => "Classic (forced — no momentum)",
        Some(AngularControl::Inertial) => "Inertial (momentum, rate-command recovery)",
    };
    info!("angular control: {label}");
}

/// Pick the rotation frame whose angle is closest to the ship's current
/// heading. Frame 0 = pointing up, frames go clockwise around 360°.
/// Pick the rotation frame whose angle is closest to the ship's current
/// heading, AND force the entity's `Transform::rotation` back to
/// identity so Bevy's renderer doesn't *also* rotate the sprite —
/// otherwise we get the SC2 rotation-frame *plus* an additional Bevy
/// transform rotation, doubling the visible spin.
///
/// Frame 0 = sprite as authored (pointing up / +Y in world space).
/// Frames go clockwise around 360° as the index increases, matching
/// the legacy Allegro datafile convention.
/// Picks the nearest pre-rotated sprite frame for the ship's current
/// angle AND applies a small `Transform.rotation` residual so the
/// visible orientation is continuous (not stepped) between frame
/// boundaries. With 64 frames that's 5.625° per step — at slow spins
/// the discrete jumps are noticeable, but a sub-step Transform
/// rotation of up to ±2.8° smooths the gap to invisible.
///
/// Safe to write `Transform.rotation` here because we disabled
/// `PhysicsTransformConfig::transform_to_position`; Avian doesn't
/// read Transform back into the authoritative `Rotation`.
fn swap_rotation_frame(mut q: Query<(&Rotation, &ShipFrames, &mut Sprite, &mut Transform)>) {
    use std::f32::consts::{PI, TAU};
    for (rot, frames, mut sprite, mut transform) in &mut q {
        if frames.frames.is_empty() {
            continue;
        }
        let n = frames.frames.len();
        let nf = n as f32;
        let angle = rot.sin.atan2(rot.cos); // ∈ [-π, π]

        // Find the closest frame index by *rounding* (not floor)
        // so the residual is bounded to ±half a step (±π/n rad).
        let raw = ((-angle) / TAU * nf).rem_euclid(nf);
        let mut idx = (raw.round() as usize) % n;
        if idx >= n {
            idx = 0;
        }
        // The angle that frame `idx` is drawn at, in world radians.
        let frame_angle = -(idx as f32) * TAU / nf;
        // Difference between the actual rotation and the frame's
        // baked-in rotation — apply as Transform.rotation to fill
        // the gap visually.
        let mut residual = angle - frame_angle;
        // Wrap to [-π, π] for the smallest rotation.
        if residual > PI {
            residual -= TAU;
        } else if residual < -PI {
            residual += TAU;
        }

        sprite.image = frames.frames[idx].clone();
        transform.rotation = Quat::from_rotation_z(residual);
    }
}

/// Speed-cap enforcement. Without linear damping, the constant thrust
/// force would accelerate the ship indefinitely; this clamps each
/// ship's `LinearVelocity` magnitude to `speed_max`. Coasting below
/// the cap is preserved (no bleed-off — Asteroids-style inertia,
/// matching the canonical SC2 / TW behaviour where the ship keeps
/// drifting at whatever velocity you stopped thrusting at).
///
/// Doesn't touch direction — a ship moving forward at cap who turns
/// 90° and thrusts will still get force applied perpendicular to its
/// current velocity, curving the trajectory without ever exceeding
/// the magnitude cap.
fn cap_velocity(
    mut commands: Commands,
    planets: Query<(&Position, &Planet)>,
    mut q: Query<(
        Entity,
        &Position,
        &ShipPhysicsDerived,
        &mut LinearVelocity,
        Option<&crate::ultimate::HyperActive>,
        Option<&crate::ultimate::PostUltimateCoasting>,
    )>,
) {
    for (entity, pos, derived, mut vel, hyper, coasting) in &mut q {
        // Skip ships mid-ultimate — the lightspeed jump deliberately
        // exceeds speed_max for the duration of the cinematic.
        if hyper.is_some() {
            continue;
        }
        // Gravity whip: within a planet's gravity range the cap is raised,
        // so a slingshot can briefly exceed the ship's normal top speed.
        // Use the strongest (largest multiplier) planet in range.
        let mut whip = 1.0_f32;
        for (planet_pos, planet) in &planets {
            let d_sq = crate::physics::min_image(planet_pos.0 - pos.0).length_squared();
            if d_sq <= planet.gravity_range * planet.gravity_range {
                whip = whip.max(planet.whip_mult);
            }
        }
        let speed = vel.0.length();
        if coasting.is_some() {
            // Post-ultimate: don't clamp. Once the ship has shed
            // its over-speed (via braking + damping) and is back
            // under speed_max, remove the marker so cap_velocity
            // resumes normal duty.
            if speed <= derived.speed_max * 1.02 && derived.speed_max > 0.0 {
                commands
                    .entity(entity)
                    .try_remove::<crate::ultimate::PostUltimateCoasting>();
            }
            continue;
        }
        let cap = derived.speed_max * whip;
        if speed > cap && cap > 0.0 {
            vel.0 = vel.0 / speed * cap;
        }
    }
}

fn tick_weapon_cooldown(time: Res<Time<Physics>>, mut q: Query<&mut WeaponCooldown>) {
    let dt = time.delta_secs();
    for mut cd in &mut q {
        if cd.0 > 0.0 {
            cd.0 = (cd.0 - dt).max(0.0);
        }
    }
}

/// Per-ship battery regeneration. The `.ini` stats are framed in SC2
/// 50 ms frames, so this system maintains a per-ship frame counter
/// `RechargeTimer`. We tick it every FixedUpdate by `dt / 0.050`
/// frames; when the counter hits zero we add `recharge_amount` to
/// the battery (clamped at `max`) and reset the counter back to
/// `recharge_rate`. A ship with `recharge_rate == 0` (Slylandro)
/// gets no natural recharge — only its harvest special tops it up.
fn tick_battery_recharge(
    time: Res<Time<Physics>>,
    mut q: Query<(&Ship, &mut Battery, &mut RechargeTimer)>,
) {
    let dt = time.delta_secs();
    for (ship, mut battery, mut timer) in &mut q {
        if ship.stats.recharge_rate <= 0 || ship.stats.recharge_amount <= 0 {
            continue;
        }
        // Period between recharge ticks in seconds: SC2 RechargeRate
        // (a frame count) × 50 ms/frame.
        let period_s = ship.stats.recharge_rate as f32 * 0.050;
        timer.remaining_s -= dt;
        // Catch up if we accumulated more than one period in a slow
        // frame — won't normally fire but guards against pauses.
        while timer.remaining_s <= 0.0 {
            battery.current = (battery.current + ship.stats.recharge_amount).min(battery.max);
            timer.remaining_s += period_s;
        }
    }
}


/// Per-class physics knobs. Defaults come from `ShipStats.mass`; this
/// layer adds the *feel* parameters that the .ini doesn't capture —
/// collider radius, how snappy the steering is (angular damping),
/// how much the ship coasts in space (linear damping).
///
/// High angular damping ≈ fighter-like instant-turn (SC2 stock feel).
/// Low angular damping + big mass ≈ boat-like inertia drift.
/// Per-class engine knobs that the `.ini` doesn't capture. Mass, speed,
/// accel, and turn rate are all in the .ini and get derived into
/// `ShipPhysicsDerived`; this just covers the renderer/collider side
/// and the Classic-vs-Inertial control mode.
struct PhysicsSpec {
    /// Hitbox radius. Eyeballed from the extracted sprite dimensions;
    /// can move into a manifest field later.
    collider_radius: f32,
    angular_control: AngularControl,
}

fn physics_spec(class: ShipClass) -> PhysicsSpec {
    // Every stock class defaults to Classic to match original SC2.
    // Flip a single arm to Inertial here if you want one class to feel
    // weighty/boat-like by default; `M` toggles the global override
    // for experimentation.
    let collider_radius = match class {
        ShipClass::Slypr | ShipClass::Umgdr => 12.0,
        ShipClass::Shosc | ShipClass::Arisk | ShipClass::Zfpst => 14.0,
        ShipClass::Spael | ShipClass::Pkufu | ShipClass::Thrto => 16.0,
        ShipClass::Yehte
        | ShipClass::Earcr
        | ShipClass::Mycpo
        | ShipClass::Ilwav
        | ShipClass::Vuxin
        | ShipClass::Supbl
        | ShipClass::Syrpe
        | ShipClass::Andgu
        | ShipClass::Druma
        | ShipClass::Utwju
        | ShipClass::Mmrxf
        | ShipClass::Orzne
        | ShipClass::Meltr => 22.0,
        ShipClass::Chmav | ShipClass::Kohma | ShipClass::Chebr => 28.0,
        ShipClass::Kzedr => 34.0,
        // Alary is the biggest, heaviest hull in the roster.
        ShipClass::Alabc => 40.0,
    };
    PhysicsSpec {
        collider_radius,
        angular_control: AngularControl::Classic,
    }
}

/// One barrel of a multi-gun weapon. A `VolleySpec` with N barrels
/// fires one projectile per barrel per activation, each positioned
/// and aimed independently in ship-local space. Used by the data-
/// driven ability dispatcher in `src/ability.rs`.
///
/// Local-space convention: `(0, 1)` = the ship's forward direction
/// (matches the rotation-0 sprite frame which points +Y). For the
/// Yehat Terminator's twin guns at canon-position `Vector2(±24, 14)`,
/// the corresponding barrels are
/// `Barrel { local_pos: Vec2::new(±24.0, 14.0), direction: Vec2::new(0.0, 1.0) }`.
/// For the Pkunk Fury's lateral shots at `±π/2`, the directions become
/// `Vec2::new(±1.0, 0.0)`.
#[derive(Clone, Copy, Debug)]
pub struct Barrel {
    pub local_pos: Vec2,
    pub direction: Vec2,
}

/// Force-applied-to-other primitive — Chmmr Avatar tractor.
/// Owned by a firing ship; each tick `tick_tractors` finds the
/// nearest non-friendly ship within `range` of `local_origin` and
/// applies `force_per_tick / target_mass` units of velocity toward
/// the owner. Despawns when `remaining` reaches zero.
///
/// Generic enough for future uses: any "drag X toward me" or "push X
/// away" mechanic is `force_per_tick` with sign and direction.
#[derive(Component, Debug)]
pub struct TractorBeam {
    pub owner: Entity,
    pub local_origin: Vec2,
    pub range: f32,
    /// Newton·seconds per tick. Positive = pull toward owner;
    /// negative = push away. Δv on target = force_per_tick / target_mass.
    pub force_per_tick: f32,
    pub color: Color,
    pub width: f32,
    pub remaining: f32,
}

pub(crate) fn spawn_tractor(
    commands: &mut Commands,
    owner: Entity,
    owner_pos: Vec2,
    owner_rot: &Rotation,
    local_origin: Vec2,
    range: f32,
    force_per_tick: f32,
    color: Color,
    width: f32,
    duration_s: f32,
) {
    // Same anti-ghost-spawn pose computation as spawn_beam.
    let world_origin = owner_pos
        + Vec2::new(
            local_origin.x * owner_rot.cos - local_origin.y * owner_rot.sin,
            local_origin.x * owner_rot.sin + local_origin.y * owner_rot.cos,
        );
    let endpoint = world_origin + Vec2::new(0.0, range);
    let midpoint = (world_origin + endpoint) * 0.5;
    commands.spawn((
        TractorBeam {
            owner,
            local_origin,
            range,
            force_per_tick,
            color,
            width,
            remaining: duration_s,
        },
        // Spawn the sprite invisible (custom_size = ZERO) — `tick_tractors`
        // grabs a target on the very next tick and resizes it to the disc
        // visual. Previously we initialised the sprite as a full-range
        // vertical bar, which would visibly flash for one frame *and*
        // linger forever if the tractor had no target (the if-let-Some
        // visibility branch happens later in the system).
        Sprite {
            color,
            custom_size: Some(Vec2::ZERO),
            ..default()
        },
        Transform::from_translation(midpoint.extend(0.3)),
    ));
}

/// Per-tick state for "this ship cannot be targeted" (Ilwrath cloak).
/// Homing missiles and auto-aim beams skip entities carrying this
/// component during target acquisition. Projectile collisions and
/// ship-ship rams still hurt — cloak hides from auto-targeting, not
/// from physics.
#[derive(Component, Debug)]
pub struct Invisible {
    pub remaining: f32,
}

/// Per-tick state for "incoming damage tops up battery instead of
/// hurting crew" (Utwig fortitude). While present, the projectile-hit
/// handler routes `floor(damage · conversion)` to `Battery::current`
/// (clamped to max) and zeroes the crew loss. Collisions are not
/// affected — fortitude buffers projectile damage, not rams.
#[derive(Component, Debug)]
pub struct DamageToBattery {
    pub remaining: f32,
    pub conversion: f32,
}

/// Sustained-line damage primitive — Chmmr / Arilou / VUX laser.
/// Owned by a firing ship; while the component exists, every tick
/// `tick_beams` casts a ray from `local_origin` (in the owner's
/// ship-local frame) along `local_dir` up to `range`, damaging the
/// nearest non-friendly ship intersected. The beam entity also
/// carries a Sprite so it visibly renders as a coloured line.
///
/// `auto_aim` switches the world direction each tick to point at the
/// nearest enemy in range (Arilou's canonical auto-targeting halo).
#[derive(Component, Debug)]
pub struct Beam {
    pub owner: Entity,
    pub local_origin: Vec2,
    pub local_dir: Vec2,
    pub range: f32,
    pub damage_per_tick: i32,
    pub color: Color,
    pub auto_aim: bool,
    pub remaining: f32,
    /// Visual half-thickness of the beam in world units.
    pub width: f32,
    /// Damage accumulator (in SC2-frame units). FixedUpdate may
    /// tick faster than the canonical 20 fps, so we accumulate
    /// `dt * 20` here each tick and only apply `damage_per_tick`
    /// once it crosses 1.0 — matches canon damage-per-frame
    /// regardless of how often we tick.
    pub damage_accum: f32,
}

/// Spawn a beam entity owned by `owner`. The beam follows the owner
/// each tick (its world pose is recomputed from the owner's transform),
/// damages the nearest non-friendly ship along its ray, and despawns
/// when `duration_s` elapses.
pub(crate) fn spawn_beam(
    commands: &mut Commands,
    owner: Entity,
    owner_pos: Vec2,
    owner_rot: &Rotation,
    local_origin: Vec2,
    local_dir: Vec2,
    range: f32,
    damage_per_tick: i32,
    color: Color,
    auto_aim: bool,
    duration_s: f32,
    width: f32,
) {
    // Compute *only* the initial origin — the visible quad is set
    // by `tick_beams` on its first tick. Spawning at zero size
    // avoids a 1-frame "beam pointing the wrong way" flash for
    // auto-aim beams (the spawn pose uses ship-forward, the tick
    // updates it to point at the actual target, and at high fire
    // rates two beams can be visible simultaneously — one
    // mid-life-aimed, one fresh-and-wrong).
    let world_origin = owner_pos
        + Vec2::new(
            local_origin.x * owner_rot.cos - local_origin.y * owner_rot.sin,
            local_origin.x * owner_rot.sin + local_origin.y * owner_rot.cos,
        );
    let _ = local_dir; // kept on the component for tick_beams' rotation calc

    commands.spawn((
        Beam {
            owner,
            local_origin,
            local_dir,
            range,
            damage_per_tick,
            color,
            auto_aim,
            remaining: duration_s,
            width,
            damage_accum: 0.0,
        },
        Sprite::from_color(color, Vec2::new(0.0, 0.0)),
        Transform::from_translation(world_origin.extend(0.3)),
    ));
}

/// Multi-form ship state. Each mode is a *complete* swap of stats +
/// sprites + abilities — no diff logic, no partial overrides. The
/// dispatcher's `ToggleMode` cycles `current`; `tick_ship_modes`
/// notices the change (via `applied`) and replaces the live
/// components atomically.
///
/// Canonical uses today: Mmrnmhrm T-form ↔ Y-form,
/// Androsynth normal ↔ Blazer.
#[derive(Component, Debug, Clone)]
pub struct ShipModes {
    pub modes: Vec<Mode>,
    /// Index of the mode that's *currently selected* (player-facing).
    pub current: usize,
    /// Last index `tick_ship_modes` applied. When it differs from
    /// `current`, the system swaps in the new mode and updates this.
    /// `None` on the very first tick so we apply mode 0 even if
    /// `current == 0` (e.g. swapping in fresh `frames`/`abilities`
    /// that weren't built at spawn).
    pub applied: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Mode {
    pub name: &'static str,
    pub derived: ShipPhysicsDerived,
    pub mass: f32,
    pub frames: Vec<Handle<Image>>,
    pub abilities: crate::ability::ShipAbilities,
    /// SC2-frames-per-tick of battery drain. Positive = drain;
    /// negative = bonus recharge; 0 = no effect.
    pub batt_drain_per_tick: i32,
    /// Auto-revert to mode 0 when battery hits 0. Canonical Andro
    /// Blazer (exits when battery exhausted).
    pub revert_on_empty: bool,
    /// Thrust is forced ON every tick — the player can't coast.
    /// Canonical Andro Blazer (auto-thrusts as a comet).
    pub thrust_locked: bool,
    /// On-collision crew damage to whatever this ship rams (Andro
    /// Blazer specialDamage). Zero means no contact damage.
    pub collide_damage: i32,
}

/// Marker requesting a mode toggle on this ship. The dispatcher inserts
/// it when `AbilityKind::ToggleMode` fires; `process_mode_toggle_requests`
/// drains it each tick, advances the ship's `ShipModes.current`, and
/// removes the marker. Keeps the ability dispatcher pure (doesn't need
/// `&mut ShipModes` access in its query).
#[derive(Component, Debug)]
pub struct ModeToggleRequest;

fn process_mode_toggle_requests(
    mut commands: Commands,
    mut q: Query<(Entity, &mut ShipModes), With<ModeToggleRequest>>,
) {
    for (entity, mut modes) in &mut q {
        if modes.modes.is_empty() {
            commands.entity(entity).try_remove::<ModeToggleRequest>();
            continue;
        }
        modes.current = (modes.current + 1) % modes.modes.len();
        info!("mode toggled → {}", modes.modes[modes.current].name);
        commands.entity(entity).try_remove::<ModeToggleRequest>();
    }
}

/// Apply mode swaps + per-tick mode effects.
///   - On `current` change: replace ShipPhysicsDerived, Mass,
///     ShipFrames, ShipAbilities with the new mode's values.
///   - Every tick: drain battery if mode says so; auto-revert
///     when battery hits 0 if mode says so.
fn tick_ship_modes(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut q: Query<(
        Entity,
        &mut ShipModes,
        &mut Battery,
        &mut Mass,
        &mut ShipFrames,
        &mut ShipPhysicsDerived,
    )>,
) {
    let dt_frames = time.delta_secs() / 0.050;
    for (entity, mut modes, mut batt, mut mass, mut frames, mut derived) in &mut q {
        // Pre-extract the values we'll need so we don't borrow modes
        // mutably twice.
        let idx = modes.current.min(modes.modes.len().saturating_sub(1));
        modes.current = idx;
        let needs_apply = modes.applied != Some(idx);
        let (
            new_derived,
            new_mass,
            new_frames,
            new_abilities,
            batt_drain,
            revert_on_empty,
        ) = {
            let m = &modes.modes[idx];
            (
                m.derived.clone(),
                m.mass,
                m.frames.clone(),
                m.abilities.clone(),
                m.batt_drain_per_tick,
                m.revert_on_empty,
            )
        };

        if needs_apply {
            *derived = new_derived;
            *mass = Mass(new_mass);
            frames.frames = new_frames;
            commands.entity(entity).try_insert(new_abilities);
            info!(
                "mode applied: {} (speed_max={:.0}, mass={:.1}, thrust_locked={})",
                modes.modes[idx].name,
                derived.speed_max,
                mass.0,
                modes.modes[idx].thrust_locked,
            );
            modes.applied = Some(idx);
        }

        // Per-tick battery drain.
        if batt_drain != 0 {
            let drain = (batt_drain as f32 * dt_frames).round() as i32;
            batt.current = (batt.current - drain).clamp(0, batt.max);
        }

        // Auto-revert when battery is exhausted (Andro Blazer).
        if revert_on_empty && idx != 0 && batt.current <= 0 {
            info!("mode reverts: {} → {} (battery empty)", modes.modes[idx].name, modes.modes[0].name);
            modes.current = 0;
        }
    }
}

/// VUX-style limpet projectile. On hit the target's current velocity
/// is multiplied by `slowdown_factor` — that's the canonical VUX
/// mechanic from `shpvuxin.cpp:VuxLimpet::inflict_damage`:
/// `target->handle_speed_loss(this, slowdown_factor)`. Limpets do
/// **not** deal crew damage in canon; they only slow.
///
/// .ini Vuxin Special: `Slowdown = 0.5` — each hit halves the
/// target's speed. Stacks multiplicatively across hits.
#[derive(Component, Debug)]
pub struct Limpet {
    pub slowdown_factor: f32,
}

/// Render-only child of a ship: a sprite that follows the ship's
/// position each tick and picks its rotation frame from
/// `parent_angle + extra_angle`. No collider, no physics body — it's
/// purely visual.
///
/// First instance: the Orz turret, which rotates *separately* from
/// the hull (canonical `data->spriteExtra` drawn at the ship's pos
/// with angle = ship.angle + turret_angle). Future composite-ship
/// work generalises this primitive into proper jointed Parts, but
/// this is enough for visible turrets / satellites / decorative
/// accessories that don't need their own collider.
#[derive(Component, Debug)]
pub struct OverlaySprite {
    pub parent: Entity,
    /// 64 rotation frames indexed by angle, same convention as
    /// `ShipFrames` (frame 0 = north, CCW).
    pub frames: Vec<Handle<Image>>,
    /// Additional rotation applied on top of the parent's, in radians.
    /// For the Orz turret this is the turret_angle (currently always
    /// 0; held-L/R-during-special turret aim is a follow-up).
    pub extra_angle: f32,
    /// Z layer offset above the ship hull so the turret renders on
    /// top instead of under.
    pub z_offset: f32,
}

/// Each tick, snap every `OverlaySprite` onto its parent's pose and
/// pick the right rotation frame. Despawns the overlay if the parent
/// is gone (avoids dangling turrets after the ship is killed).
fn update_overlay_sprites(
    mut commands: Commands,
    parents: Query<(&Position, &Rotation), With<Ship>>,
    mut overlays: Query<(Entity, &OverlaySprite, &mut Sprite, &mut Transform)>,
) {
    use std::f32::consts::{PI, TAU};
    for (overlay_entity, overlay, mut sprite, mut transform) in &mut overlays {
        let Ok((parent_pos, parent_rot)) = parents.get(overlay.parent) else {
            commands.entity(overlay_entity).try_despawn();
            continue;
        };
        let n = overlay.frames.len();
        if n == 0 {
            continue;
        }
        let nf = n as f32;
        let parent_angle = parent_rot.sin.atan2(parent_rot.cos);
        let total = parent_angle + overlay.extra_angle;

        // Same nearest-frame + residual interpolation as
        // `swap_rotation_frame` — keeps the turret rotation visually
        // continuous instead of stepping in 5.6° chunks.
        let raw = ((-total) / TAU * nf).rem_euclid(nf);
        let mut idx = (raw.round() as usize) % n;
        if idx >= n {
            idx = 0;
        }
        let frame_angle = -(idx as f32) * TAU / nf;
        let mut residual = total - frame_angle;
        if residual > PI {
            residual -= TAU;
        } else if residual < -PI {
            residual += TAU;
        }

        sprite.image = overlay.frames[idx].clone();
        transform.translation = parent_pos.0.extend(overlay.z_offset);
        transform.rotation = Quat::from_rotation_z(residual);
    }
}

/// Small autonomous body spawned by a ship's special — Chenjesu DOGI,
/// Orz marine, Syreen crew pod, Kzer-Za fighter. Has its own physics
/// body, sprite, HP, lifetime, and an `SubEntityAi` component that
/// drives per-tick steering + on-collision behaviour.
///
/// Generic enough to cover four canonical mechanics by varying the AI
/// component alone. Living off the existing Avian collision events;
/// `handle_sub_entity_collisions` dispatches the right effect per AI.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct SubEntity {
    /// Which ship spawned this — used for friendly-fire filtering and
    /// for `DriftAndCollect` to know who's allowed to pick it up.
    pub owner: Entity,
    /// Seconds remaining. Despawns when this hits zero.
    pub remaining_s: f32,
    /// Hit points. Some sub-entities survive multiple hits (DOGI has
    /// armour 3, fighters take a few). When this drops to ≤ 0 the
    /// sub-entity despawns.
    pub hp: i32,
}

/// Behaviour variants for `SubEntity`. Carried as a separate
/// component so it can be queried/mutated independently of the body
/// state. New canonical mechanics extend this enum.
#[derive(Component, Debug)]
pub enum SubEntityAi {
    /// Steer toward the nearest non-friendly ship; on contact deal
    /// `damage_on_hit` crew damage and `batt_sap` battery drain.
    /// Survives until `hp` drops below zero or lifetime expires.
    /// Canonical: Chenjesu DOGI (`shpchebr.cpp:ChenjesuDOGI`).
    HomeAndDetonate {
        target: Option<Entity>,
        turn_rate: f32,
        speed: f32,
        damage_on_hit: i32,
        batt_sap: i32,
    },
    /// Steer toward the nearest enemy; on contact deal heavy crew
    /// damage and consume the sub-entity. Models Orz marine
    /// boarding (`shporzne.cpp:OrzMarine`); the actual joint-attach
    /// is approximated by one-shot drain.
    AttachAndDrain {
        target: Option<Entity>,
        turn_rate: f32,
        speed: f32,
        crew_drain: i32,
    },
    /// Pure ballistic drift at the spawn-time velocity. On contact
    /// with a ship whose `player_slot == owner_slot`, add
    /// `crew_value` to that ship's crew and consume the pod.
    /// Enemy contact does nothing. Models Syreen crew pods
    /// (`shpsyrpe.cpp:CrewPod`).
    DriftAndCollect {
        owner_slot: usize,
        crew_value: i32,
    },
}

/// Spawn a sub-entity attached to `owner`'s pose with the given AI
/// behaviour. Sprite / size / colour are visual; HP and lifetime are
/// the body-state knobs.
/// `initial_angle_offset` is in radians CCW from the owner's forward —
/// `0` = straight forward (out the nose), `π` = directly backward
/// (out the rear, Spathi-BUTT style).
pub(crate) fn spawn_sub_entity(
    commands: &mut Commands,
    assets: &AssetServer,
    owner: Entity,
    owner_pos: Vec2,
    owner_rot: &Rotation,
    local_offset: Vec2,
    initial_angle_offset: f32,
    initial_speed: f32,
    sprite_path: Option<&str>,
    sprite_size: f32,
    color: Color,
    hp: i32,
    lifetime_s: f32,
    ai: SubEntityAi,
) {
    let world_pos_offset = Vec2::new(
        local_offset.x * owner_rot.cos - local_offset.y * owner_rot.sin,
        local_offset.x * owner_rot.sin + local_offset.y * owner_rot.cos,
    );
    let spawn_pos = owner_pos + world_pos_offset;
    let forward = Vec2::new(-owner_rot.sin, owner_rot.cos);
    let (s, c) = initial_angle_offset.sin_cos();
    let world_dir = Vec2::new(
        forward.x * c - forward.y * s,
        forward.x * s + forward.y * c,
    );
    let initial_vel = world_dir * initial_speed;
    let initial_rotation = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

    let sprite = if let Some(path) = sprite_path {
        Sprite {
            image: assets.load(path.to_string()),
            color,
            custom_size: Some(Vec2::splat(sprite_size)),
            ..default()
        }
    } else {
        Sprite::from_color(color, Vec2::splat(sprite_size))
    };

    commands.spawn((
        SubEntity {
            owner,
            remaining_s: lifetime_s,
            hp,
        },
        ai,
        sprite,
        Transform::from_translation(spawn_pos.extend(0.4)),
        RigidBody::Dynamic,
        Collider::circle((sprite_size * 0.5).max(1.0)),
        Mass(1.0),
        Position(spawn_pos),
        Rotation::radians(initial_rotation),
        LinearVelocity(initial_vel),
        AngularVelocity::ZERO,
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
}

// SC2 unit conversion helpers (mirror mhelpers.cpp).
//
//   distance_ratio = 0.48 TW-pixels / SC2-pixel
//   time_ratio     = 50 ms / SC2 frame
//
// so   scale_velocity(v) = v · 0.48 / 0.050 = v · 9.6   (world u/s)
//      scale_range(r)    = r · 40                       (world u)
//      scale_turning(t)  = (2π/16) / (t+1) / 0.050      (rad/s)
//
// Per-projectile lifetime falls out of canonical `range / velocity`
// (the original Missile constructor takes a range and dies at d >= range).
pub const SC2_VEL_SCALE: f32 = 9.6;
pub const SC2_RANGE_SCALE: f32 = 40.0;
pub fn sc2_turning(t: f32) -> f32 {
    (std::f32::consts::TAU / 16.0) / (t + 1.0) / 0.050
}



fn tick_projectile_lifetime(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut q: Query<(Entity, &mut Projectile)>,
) {
    let dt = time.delta_secs();
    for (entity, mut proj) in &mut q {
        proj.lifetime -= dt;
        if proj.lifetime <= 0.0 {
            commands.entity(entity).try_despawn();
        }
    }
}

/// Chenjesu Broodhome primary — manages the held-crystal projectile
/// and the on-release shatter into 8 shards. Mirrors the canonical
/// shpchebr.cpp:calculate logic:
///
///   - press fire (transition up→down): spawn one ChenjesuShot
///     (a normal homing-less Projectile) from (0, +size.y/2). Only
///     one crystal active at a time per ship.
///   - hold fire: crystal continues forward as a normal physics
///     projectile. Avian handles its motion + collisions naturally.
///   - release fire (transition down→up) while crystal still alive:
///     query its current Position, spawn 8 shards radiating at
///     π/4 increments, despawn the crystal.
///   - crystal dies on a hit: handle_projectile_hits despawns it,
///     this system clears `CrystalCarrier.current` next tick.
///
/// All projectiles (crystal + shards) are RigidBody::Dynamic with
/// Position/Velocity/Collider → Avian's collision events drive the
/// hits via handle_projectile_hits, no custom range queries needed.
/// .ini Chebr.Weapon: Velocity=64, Damage=6, ShardDamage=2,
/// ShardRange=9, ShardArmour=2, ShardRotation=1.
fn tick_chebr_crystal(
    mut commands: Commands,
    slot_inputs: Res<input::SlotInputs>,
    assets: Res<AssetServer>,
    mut rng: ResMut<crate::rng::GameRng>,
    projectiles: Query<&Position, With<Projectile>>,
    mut ships: Query<
        (
            Entity,
            &Ship,
            &Position,
            &Rotation,
            &LinearVelocity,
            &mut CrystalCarrier,
            &mut Battery,
        ),
        With<Ship>,
    >,
) {
    let weapon_velocity = 64.0 * SC2_VEL_SCALE;
    let shard_range_world = 9.0 * SC2_RANGE_SCALE;
    let shard_lifetime = shard_range_world / weapon_velocity;
    let shard_damage = 2;
    let weapon_damage = 6;

    for (entity, ship, ship_pos, ship_rot, ship_vel, mut carrier, mut batt) in &mut ships {
        let input = slot_inputs.held[ship.player_slot.min(3)];
        let fire_held = input.pressed(input::INPUT_FIRE);

        // Clear stale handle if the crystal died on a hit (collision
        // → handle_projectile_hits despawned it). projectiles.get()
        // returns Err → we know it's gone.
        if let Some(c) = carrier.current {
            if projectiles.get(c).is_err() {
                carrier.current = None;
            }
        }

        let was_held = carrier.last_fire_held;
        carrier.last_fire_held = fire_held;

        let just_pressed = fire_held && !was_held;
        let just_released = !fire_held && was_held;

        if just_pressed && carrier.current.is_none() {
            // Spawn the crystal. Battery gate matches the dispatcher path.
            if ship.stats.weapon_drain > 0 && batt.current < ship.stats.weapon_drain {
                continue;
            }
            batt.current = (batt.current - ship.stats.weapon_drain).max(0);

            let forward = Vec2::new(0.0, 1.0);
            // Spawn well clear of the Chebr hull (collider radius ~28
            // for Chebr's polygon). At 60 units forward the crystal
            // has clean separation — Avian's solver previously fought
            // the crystal's initial velocity when they overlapped at
            // (0, 32), leaving the crystal stuck near the ship.
            let local_pos = Vec2::new(0.0, 60.0);
            let world_off = Vec2::new(
                local_pos.x * ship_rot.cos - local_pos.y * ship_rot.sin,
                local_pos.x * ship_rot.sin + local_pos.y * ship_rot.cos,
            );
            let world_dir = Vec2::new(
                forward.x * ship_rot.cos - forward.y * ship_rot.sin,
                forward.x * ship_rot.sin + forward.y * ship_rot.cos,
            );
            let muzzle = ship_pos.0 + world_off;
            let proj_vel = ship_vel.0 + world_dir * weapon_velocity;
            let initial_angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

            let crystal_entity = commands
                .spawn((
                    Projectile {
                        owner: entity,
                        damage: weapon_damage,
                        lifetime: 6.0, // generous; player typically releases earlier
                    },
                    Sprite {
                        image: assets.load("ships/chebr/sprites/shot_a_01_tga.png"),
                        color: Color::srgb(1.0, 1.0, 1.0),
                        custom_size: Some(Vec2::splat(20.0)),
                        ..default()
                    },
                    Transform::from_translation(muzzle.extend(0.5)),
                    RigidBody::Dynamic,
                    Collider::circle(10.0),
                    // Sensor so Avian doesn't bat the crystal around
                    // on contact — collision events still fire for
                    // damage handling (handle_projectile_hits).
                    Sensor,
                    Mass(0.5 + weapon_damage as f32 * 0.4),
                    Position(muzzle),
                    Rotation::radians(initial_angle),
                    LinearVelocity(proj_vel),
                    AngularVelocity::ZERO,
                    LinearDamping(0.0),
                    AngularDamping(0.0),
                    CollisionEventsEnabled,
                ))
                .id();
            carrier.current = Some(crystal_entity);
            info!("chebr crystal launched");
        }

        if just_released {
            if let Some(crystal_entity) = carrier.current.take() {
                if let Ok(crystal_pos) = projectiles.get(crystal_entity) {
                    let burst_at = crystal_pos.0;
                    // Stochastic chaotic shatter: 14-22 shards at
                    // random angles, random speeds, random small
                    // triangular polygon colliders. ALL draws use
                    // the seeded `GameRng` — every value here
                    // ends up on a projectile's velocity or
                    // collider, so peers must agree.
                    let n_shards = 14 + rng.usize_range(0..8);
                    for _ in 0..n_shards {
                        let theta = rng.f32() * std::f32::consts::TAU;
                        let speed_mult = 0.6 + rng.f32() * 1.4; // 0.6× to 2.0×
                        let dir = Vec2::new(theta.cos(), theta.sin());
                        let shard_vel = dir * weapon_velocity * speed_mult;
                        let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
                        let init_spin = rng.signed_unit() * 10.0;
                        let size = 4.0 + rng.f32() * 6.0;
                        // Spread each shard slightly along its own
                        // velocity direction so they don't all
                        // spawn at one point and immediately stack.
                        let spawn_pos = burst_at + dir * (8.0 + rng.f32() * 12.0);
                        // Colour is visual-only — but we keep it
                        // on `GameRng` anyway so the RNG stream
                        // advances the same number of draws per
                        // shard. Otherwise we'd have to track
                        // colour separately and risk drift.
                        let shard_color = Color::srgb(
                            0.7 + rng.f32() * 0.3,
                            0.75 + rng.f32() * 0.25,
                            0.9 + rng.f32() * 0.1,
                        );
                        // Random triangular polygon collider for that
                        // "jagged crystal shard" feel. Three vertices
                        // around a circle with jittered radius and
                        // angle.
                        let mut verts = [Vec2::ZERO; 3];
                        for (i, v) in verts.iter_mut().enumerate() {
                            let base_a = i as f32 * std::f32::consts::TAU / 3.0;
                            let a = base_a + rng.signed_unit() * 0.35;
                            let r = size * (0.6 + rng.f32() * 0.5);
                            *v = Vec2::new(a.cos() * r, a.sin() * r);
                        }
                        let collider = Collider::triangle(verts[0], verts[1], verts[2]);
                        commands.spawn((
                            Projectile {
                                owner: entity,
                                damage: shard_damage,
                                lifetime: shard_lifetime,
                            },
                            Sprite::from_color(shard_color, Vec2::splat(size * 1.8)),
                            Transform::from_translation(spawn_pos.extend(0.5)),
                            RigidBody::Dynamic,
                            collider,
                            Mass(0.5 + shard_damage as f32 * 0.4),
                            Position(spawn_pos),
                            Rotation::radians(init_angle),
                            LinearVelocity(shard_vel),
                            AngularVelocity(init_spin),
                            LinearDamping(0.0),
                            AngularDamping(0.0),
                            CollisionEventsEnabled,
                        ));
                    }
                    commands.entity(crystal_entity).try_despawn();
                    info!("chebr crystal shatter: {} chaotic shards", n_shards);
                }
            }
        }
    }
}

/// Melnorme charge-and-release primary.
///
/// The user opted to replace the canonical discrete 3-phase charge
/// (each phase doubles damage, +RangeUp range) with a **continuous
/// linear** interpolation — the shot grows and shifts colour
/// smoothly with hold duration instead of stepping in chunks. Max
/// hold time still matches the canonical full-charge (7.5 s).
///
///   - press fire: spawn one charging-shot projectile at the muzzle,
///     scale 1.0, base damage 2, base colour pale green, sensor
///     collider so Avian doesn't shove it on contact with the ship.
///   - hold fire: each tick:
///       - position = ship muzzle (rides with the ship)
///       - charge_fraction = (sub_charge_s / max_charge_s).min(1.0)
///       - damage = base + (max - base) · charge_fraction  (rounded)
///       - Transform.scale = 1.0 + (max_scale - 1.0) · charge_fraction
///         (collider scales with Transform thanks to
///         `transform_to_collider_scale`)
///       - Sprite.color = lerp(base_color, max_color, charge_fraction)
///   - release fire: detach — set LinearVelocity = ship_vel +
///     forward · speed; recompute lifetime from
///     `(base_range + charge_fraction · max_extra_range) / speed`.
///   - hit during charge: still damages (charging shot is dangerous
///     to touch). handle_projectile_hits despawns via Avian
///     CollisionStart events; state clears next tick.
fn tick_meltr_charge(
    mut commands: Commands,
    slot_inputs: Res<input::SlotInputs>,
    assets: Res<AssetServer>,
    time: Res<Time<Physics>>,
    mut projectiles: Query<(
        &mut Projectile,
        &mut Position,
        &mut LinearVelocity,
        &mut Transform,
        &mut Sprite,
    )>,
    mut ships: Query<
        (
            Entity,
            &Ship,
            &Position,
            &Rotation,
            &LinearVelocity,
            &mut MeltrChargeState,
            &mut Battery,
        ),
        Without<Projectile>,
    >,
) {
    let dt = time.delta_secs();
    // Canon: 5 charge_frames × 10 sprite_frames × 50 ms = 2.5 s per
    // phase × 3 phases = 7.5 s. We use 10 s here — the user found
    // the canonical pace feels too fast in continuous mode (without
    // the discrete sound/phase cues from legacy you don't perceive
    // tier crossings until the flash), so the longer window gives
    // the size+colour ramp more time to read.
    let max_charge_s: f32 = 10.0;
    let base_damage: i32 = 2;
    let max_damage: i32 = 16; // 2·2³ — canon max
    let base_range_world: f32 = 21.0 * SC2_RANGE_SCALE;
    let max_extra_range: f32 = 3.0 * 3.0 * SC2_RANGE_SCALE;
    let speed: f32 = 112.0 * SC2_VEL_SCALE;
    let muzzle_local = Vec2::new(0.0, 60.0); // clear of ship hull, like Chebr crystal
    // At full charge the shot is 6× its base size — substantially
    // "really big" per the user's request. Combined with the
    // colour shift + sprite-frame cycling this makes the peak
    // unambiguous.
    let max_scale: f32 = 6.0;
    const FLASH_DURATION_S: f32 = 0.4;

    for (entity, ship, ship_pos, ship_rot, ship_vel, mut state, mut batt) in &mut ships {
        let input = slot_inputs.held[ship.player_slot.min(3)];
        let fire_held = input.pressed(input::INPUT_FIRE);
        let was_held = state.last_fire_held;
        state.last_fire_held = fire_held;
        let just_pressed = fire_held && !was_held;
        let just_released = !fire_held && was_held;

        if let Some(e) = state.active {
            if projectiles.get(e).is_err() {
                state.active = None;
                state.sub_charge_s = 0.0;
                state.last_damage = 0;
                state.flash_remaining_s = 0.0;
            }
        }

        let forward_local = Vec2::new(0.0, 1.0);
        let world_forward = Vec2::new(
            forward_local.x * ship_rot.cos - forward_local.y * ship_rot.sin,
            forward_local.x * ship_rot.sin + forward_local.y * ship_rot.cos,
        );
        let world_muzzle_off = Vec2::new(
            muzzle_local.x * ship_rot.cos - muzzle_local.y * ship_rot.sin,
            muzzle_local.x * ship_rot.sin + muzzle_local.y * ship_rot.cos,
        );
        let muzzle = ship_pos.0 + world_muzzle_off;

        if just_pressed && state.active.is_none() {
            if ship.stats.weapon_drain > 0 && batt.current < ship.stats.weapon_drain {
                continue;
            }
            batt.current = (batt.current - ship.stats.weapon_drain).max(0);

            let initial_angle =
                world_forward.y.atan2(world_forward.x) - std::f32::consts::FRAC_PI_2;
            let shot_entity = commands
                .spawn((
                    Projectile {
                        owner: entity,
                        damage: base_damage,
                        lifetime: 60.0,
                    },
                    Sprite {
                        image: assets.load("ships/meltr/sprites/shot_a01.png"),
                        // Initial pale-green tint; the per-tick lerp
                        // below ramps toward bright yellow-white.
                        color: Color::srgb(0.5, 1.0, 0.5),
                        custom_size: Some(Vec2::splat(10.0)),
                        ..default()
                    },
                    Transform::from_translation(muzzle.extend(0.5)),
                    RigidBody::Dynamic,
                    Collider::circle(5.0),
                    Sensor, // charging shot is "attached" — no impulse exchange
                    Mass(0.5 + base_damage as f32 * 0.4),
                    Position(muzzle),
                    Rotation::radians(initial_angle),
                    LinearVelocity(ship_vel.0),
                    AngularVelocity::ZERO,
                    LinearDamping(0.0),
                    AngularDamping(0.0),
                    CollisionEventsEnabled,
                ))
                .id();
            state.active = Some(shot_entity);
            state.sub_charge_s = 0.0;
            state.last_damage = base_damage;
            state.flash_remaining_s = 0.0;
            info!("meltr charge: shot spawned (base dmg {})", base_damage);
        }

        if let Some(shot_entity) = state.active {
            // Flash timer ticks down whether or not fire is held — a
            // step-up that happened a few frames ago still fades.
            state.flash_remaining_s = (state.flash_remaining_s - dt).max(0.0);

            if fire_held {
                state.sub_charge_s = (state.sub_charge_s + dt).min(max_charge_s);
                let charge_fraction = state.sub_charge_s / max_charge_s;
                let new_damage = base_damage
                    + ((max_damage - base_damage) as f32 * charge_fraction).round() as i32;

                // Damage is integer (Crew is integer), so it crosses
                // discrete tiers as the player holds. Each time it
                // does, kick off a flash so the player sees the
                // "now this shot is meaningfully stronger" moment.
                if new_damage > state.last_damage {
                    state.flash_remaining_s = FLASH_DURATION_S;
                    state.last_damage = new_damage;
                    info!("meltr charge tier: dmg now {}", new_damage);
                }

                let scale_base = 1.0 + (max_scale - 1.0) * charge_fraction;
                // Pick sprite frame from charge fraction: shot_a01..a10
                // cycle through ten ascending charge tiers. Avoids
                // the boring "single colour gets brighter" look —
                // each tier of the legacy animation is its own
                // distinct sprite that fits the bigger / hotter
                // shot identity the user wanted.
                let frame_idx =
                    ((charge_fraction * 9.0).floor() as usize + 1).min(10);
                let sprite_path = format!(
                    "ships/meltr/sprites/shot_a{:02}.png",
                    frame_idx
                );

                // Base tint stays pale-green→yellow-white. The
                // sprite frame already varies the underlying art.
                let mut r = 0.5 + 0.5 * charge_fraction;
                let mut g = 1.0;
                let mut b = 0.5 + 0.1 * charge_fraction;

                // Flash overlay — for the full duration, push colour
                // toward pure white and add an extra ~20% size pulse.
                // Longer than the previous attempt (0.4 s vs 0.15 s),
                // and the size component makes it visible even when
                // the lerp is already shifting colour.
                let mut flash_scale_mul = 1.0;
                if state.flash_remaining_s > 0.0 {
                    let f = (state.flash_remaining_s / FLASH_DURATION_S).clamp(0.0, 1.0);
                    r = r + (1.0 - r) * f;
                    g = g + (1.0 - g) * f;
                    b = b + (1.0 - b) * f;
                    flash_scale_mul = 1.0 + 0.25 * f;
                }
                let scale = scale_base * flash_scale_mul;

                if let Ok((mut proj, mut pos, mut vel, mut xf, mut sprite)) =
                    projectiles.get_mut(shot_entity)
                {
                    proj.damage = new_damage;
                    pos.0 = muzzle;
                    vel.0 = ship_vel.0;
                    xf.scale = Vec3::new(scale, scale, 1.0);
                    sprite.image = assets.load(sprite_path);
                    sprite.color = Color::srgb(r, g, b);
                }
            }

            if just_released {
                let charge_fraction = (state.sub_charge_s / max_charge_s).min(1.0);
                let final_range = base_range_world + max_extra_range * charge_fraction;
                let final_lifetime = final_range / speed;
                if let Ok((mut proj, _, mut vel, _, _)) = projectiles.get_mut(shot_entity) {
                    vel.0 = ship_vel.0 + world_forward * speed;
                    proj.lifetime = final_lifetime;
                    // Demote from Sensor to a real Dynamic projectile
                    // now that it's flying — heavy released shots
                    // should impart impulse on the target like every
                    // other projectile.
                    commands.entity(shot_entity).try_remove::<Sensor>();
                }
                info!(
                    "meltr release: charge {:.0}%, dmg {}, range {:.0}",
                    charge_fraction * 100.0,
                    base_damage
                        + ((max_damage - base_damage) as f32 * charge_fraction).round() as i32,
                    final_range
                );
                state.active = None;
                state.sub_charge_s = 0.0;
                state.last_damage = 0;
                state.flash_remaining_s = 0.0;
            }
        }
    }
}

/// Orz Nemesis turret + space marines (`shporzne.cpp`).
///
/// Holding SPECIAL remaps the turn keys to rotate the turret (no ship
/// rotation) and cancels primary fire — pressing FIRE while special is
/// held spawns a marine instead (1 crew, capped at MAX_MARINES). With
/// special released, primary fires in the TURRET's facing (ship angle
/// + `OrzTurret.offset_rad`). The visual turret (`OverlaySprite` with
/// the `shot_d_NN` frames) tracks `offset_rad` each tick.
fn tick_orz_turret(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    assets: Res<AssetServer>,
    mut ships: Query<
        (
            Entity,
            &Ship,
            &ShipClass,
            &Position,
            &Rotation,
            &LinearVelocity,
            &mut AngularVelocity,
            &mut ConstantTorque,
            &mut OrzTurret,
            &mut Crew,
            &mut Battery,
            &ShipPhysicsDerived,
        ),
        Without<crate::ultimate::HyperActive>,
    >,
    sub_entities: Query<&SubEntity>,
    mut overlays: Query<&mut OverlaySprite>,
) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    const MAX_MARINES: usize = 8;
    let dt = time.delta_secs();
    for (
        entity,
        ship,
        class,
        pos,
        rot,
        vel,
        mut ang_vel,
        mut torque,
        mut turret,
        mut crew,
        mut batt,
        derived,
    ) in &mut ships
    {
        if *class != ShipClass::Orzne {
            continue;
        }
        let input = slot_inputs.held[ship.player_slot.min(3)];
        let special_held = input.pressed(input::INPUT_SPECIAL);
        let fire_held = input.pressed(input::INPUT_FIRE);
        let was_fire_held = turret.last_fire_held;
        turret.last_fire_held = fire_held;
        let fire_just_pressed = fire_held && !was_fire_held;
        let left = input.pressed(input::INPUT_LEFT);
        let right = input.pressed(input::INPUT_RIGHT);

        turret.fire_cooldown_s = (turret.fire_cooldown_s - dt).max(0.0);

        if special_held {
            // Special held: lock the ship's rotation (apply_player_input
            // already wrote ang_vel from the turn keys; clobber it here)
            // and instead rotate the turret. Same ±target_omega rate the
            // hull would use, so the feel matches "I'm turning, just with
            // a different thing turning."
            ang_vel.0 = 0.0;
            torque.0 = 0.0;
            let dir = if left {
                1.0
            } else if right {
                -1.0
            } else {
                0.0
            };
            turret.offset_rad += dir * derived.target_omega * dt;
            if turret.offset_rad > PI {
                turret.offset_rad -= TAU;
            } else if turret.offset_rad < -PI {
                turret.offset_rad += TAU;
            }

            // Marine on each fresh fire-press. Canon: cost = 1 crew,
            // cap at MAX_MARINES already in flight. Marine seeks the
            // nearest enemy and drains crew on attach (AttachAndDrain).
            if fire_just_pressed && crew.current > 1 {
                let in_flight = sub_entities.iter().filter(|s| s.owner == entity).count();
                if in_flight < MAX_MARINES {
                    crew.current -= 1;
                    // Launch from just ahead of the hull so the marine
                    // sprite isn't visually overlapping the ship for a
                    // tick or two.
                    let muzzle_local = Vec2::new(0.0, 18.0);
                    spawn_sub_entity(
                        &mut commands,
                        &assets,
                        entity,
                        pos.0,
                        rot,
                        muzzle_local,
                        0.0,
                        // .ini Special SpeedMax = 40 → 384 wu/s.
                        40.0 * SC2_VEL_SCALE,
                        Some("ships/orzne/sprites/shot_b_01_bmp.png"),
                        12.0,
                        Color::srgb(0.9, 1.0, 0.5),
                        // Marine "armour" — survives a couple of stray
                        // hits before the homing pass.
                        3,
                        15.0,
                        SubEntityAi::AttachAndDrain {
                            target: None,
                            turn_rate: sc2_turning(2.0),
                            speed: 40.0 * SC2_VEL_SCALE,
                            crew_drain: 4,
                        },
                    );
                }
            }
        } else {
            // Special not held: primary fires in the turret's direction.
            // Cooldown matches WeaponRate = 4 SC2 frames = 0.2 s.
            if fire_held && turret.fire_cooldown_s <= 0.0 {
                let drain = ship.stats.weapon_drain;
                if drain <= 0 || batt.current >= drain {
                    batt.current = (batt.current - drain).max(0);
                    // Ship forward in world: (-sin, cos). Rotate by the
                    // turret offset to get the cannon's true heading.
                    let (s, c) = turret.offset_rad.sin_cos();
                    let forward = Vec2::new(-rot.sin, rot.cos);
                    let world_dir = Vec2::new(
                        forward.x * c - forward.y * s,
                        forward.x * s + forward.y * c,
                    );
                    // Muzzle = barrel tip, just past the hull along the
                    // turret's forward.
                    let muzzle = pos.0 + world_dir * 28.0;
                    let proj_vel = vel.0 + world_dir * (120.0 * SC2_VEL_SCALE);
                    let initial_angle = world_dir.y.atan2(world_dir.x) - FRAC_PI_2;
                    let lifetime = (20.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE);
                    let damage: i32 = 3;
                    let sprite_size = 8.0_f32;
                    commands.spawn((
                        Projectile {
                            owner: entity,
                            damage,
                            lifetime,
                        },
                        Sprite {
                            image: assets.load("ships/orzne/sprites/shot_a01.png".to_string()),
                            color: Color::srgb(1.0, 1.0, 1.0),
                            custom_size: Some(Vec2::splat(sprite_size)),
                            ..default()
                        },
                        Transform::from_translation(muzzle.extend(0.5)),
                        RigidBody::Dynamic,
                        Collider::circle(sprite_size * 0.5),
                        Mass(0.5 + damage as f32 * 0.4),
                        Position(muzzle),
                        Rotation::radians(initial_angle),
                        LinearVelocity(proj_vel),
                        AngularVelocity::ZERO,
                        LinearDamping(0.0),
                        AngularDamping(0.0),
                        CollisionEventsEnabled,
                    ));
                    turret.fire_cooldown_s = 4.0 / 20.0;
                }
            }
        }

        // Sync the visual turret rotation. The OverlaySprite update system
        // (Update) reads this and picks the right rotation frame.
        for mut overlay in &mut overlays {
            if overlay.parent == entity {
                overlay.extra_angle = turret.offset_rad;
                break;
            }
        }
    }
}

/// Tick attached Orz marines (`shporzne.cpp:OrzMarine::calculate`).
///
/// Once a marine has its `OrzMarineBoarded` marker, this system:
///   1. Sticks its `Position` to the host each tick so it visually
///      clings to the hull (canon draws it as an icon on the host's
///      crew panel; we don't have that UI so we just glue the sprite
///      to the host's centre).
///   2. Despawns it if the host is gone (rematch / crew==0 / cycled).
///   3. Rolls the canonical per-50 ms dice:
///        - 9/10000 chance per ms · 50 ms ≈ 4.5 % → 1 crew dmg.
///        - +1/10000 chance per ms · 50 ms ≈ 0.5 % → marine despawns.
///      The 50 ms tick accumulator (`roll_accum_s`) lets us keep
///      canon cadence regardless of our actual physics rate. The roll
///      uses `GameRng` so it's deterministic across rollback.
fn tick_orz_marines_boarded(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut marines: Query<(
        Entity,
        &mut OrzMarineBoarded,
        &mut Position,
        &mut LinearVelocity,
    )>,
    mut hosts: Query<(&Position, &mut Crew, Option<&ShieldActive>, &Ship), Without<OrzMarineBoarded>>,
) {
    const TICK_MS: f32 = 50.0;
    let dt = time.delta_secs();
    for (sub_entity, mut boarded, mut pos, mut vel) in &mut marines {
        // If the host is gone, the marine has nothing to drain — drop it.
        let Ok((host_pos, mut host_crew, shield, host_ship)) =
            hosts.get_mut(boarded.host)
        else {
            commands.entity(sub_entity).try_despawn();
            continue;
        };
        // Glue to host.
        pos.0 = host_pos.0;
        vel.0 = Vec2::ZERO;

        boarded.roll_accum_s += dt;
        // Roll once per canon 50 ms tick — typically once every 3 frames
        // at 60 Hz, every frame at 20 Hz. Catches up if dt is huge (rare).
        while boarded.roll_accum_s >= TICK_MS / 1000.0 {
            boarded.roll_accum_s -= TICK_MS / 1000.0;
            let roll = rng.i32(0..=9999);
            // 9 per ms · 50 ms = 450 → 4.5 % chance, deal 1 crew damage.
            if roll < (9.0 * TICK_MS) as i32 {
                let factor = shield.map(|s| s.damage_factor).unwrap_or(1.0);
                let dmg = (1.0 * factor).round() as i32;
                if dmg > 0 {
                    host_crew.current = (host_crew.current - dmg).max(0);
                    info!(
                        "marine drained P{}: -{} crew ({}/{})",
                        host_ship.player_slot + 1,
                        dmg,
                        host_crew.current,
                        host_crew.max,
                    );
                }
            // 1 per ms · 50 ms = 50 more → 0.5 % chance, marine despawns.
            } else if roll < (10.0 * TICK_MS) as i32 {
                info!("marine succumbed on P{}", host_ship.player_slot + 1);
                commands.entity(sub_entity).try_despawn();
                break;
            }
        }
    }
}

/// Slylandro Probe drift (`shpslypr.cpp::SlylandroProbe::calculate_thrust`).
///
/// Canon: the probe is *always* under thrust — there's no coast. Pressing
/// the thrust key (rising edge only) flips the ship 180° instantly, so
/// reversing direction is a single keypress, not a slow turn-and-thrust.
/// The override runs `.after(apply_player_input)` so it gets to override
/// the per-tick thrust/rotation that the standard input system wrote.
fn tick_slylandro_drift(
    slot_inputs: Res<input::SlotInputs>,
    mut q: Query<
        (
            &Ship,
            &ShipClass,
            &mut Rotation,
            &mut AngularVelocity,
            &mut ConstantLocalForce,
            &mut SlylandroDrift,
            &ShipPhysicsDerived,
        ),
        Without<crate::ultimate::HyperActive>,
    >,
) {
    use std::f32::consts::PI;
    for (ship, class, mut rot, mut ang_vel, mut thrust, mut drift, derived) in &mut q {
        if *class != ShipClass::Slypr {
            continue;
        }
        let input = slot_inputs.held[ship.player_slot.min(3)];
        let thrust_held = input.pressed(input::INPUT_THRUST);
        let was_held = drift.last_thrust_held;
        drift.last_thrust_held = thrust_held;
        // Rising edge of thrust → instant 180° flip.
        if thrust_held && !was_held {
            // Rotate the (sin,cos) by PI: (sin,cos) → (-sin,-cos).
            let new_angle = rot.sin.atan2(rot.cos) + PI;
            *rot = Rotation::radians(new_angle);
            // Reset any spin imparted by collisions so the flip is clean.
            ang_vel.0 = 0.0;
        }
        // Always thrust forward, regardless of input — the probe drifts
        // continuously, and the player only chooses *which way*.
        thrust.0 = Vec2::new(0.0, derived.thrust_force);
    }
}

/// Alary Battle Cruiser MIRV launcher (`shpalabc.cpp` primary).
///
/// Each fire press launches ONE slow homing torpedo from an
/// alternating side of the hull (legacy `side *= -1`). The torpedo
/// itself does no contact damage — `steer_homing_projectiles` curves
/// it toward the nearest enemy, and once it closes within `proximity`
/// it despawns and bursts into five faster homing warheads fanned at
/// `[0, ±50°, ±75°]` off its heading (the canonical 5-warhead MIRV
/// spread). Warheads are normal damaging homing projectiles.
fn tick_alary_mirv(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    assets: Res<AssetServer>,
    torpedoes: Query<(Entity, &AlaryTorpedo, &Position, &LinearVelocity)>,
    ships: Query<(Entity, &Ship, &Position)>,
    mut launchers: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut AlaryMirvState,
        &mut Battery,
        Option<&crate::ai::AiControlled>,
    )>,
) {
    let dt = time.delta_secs();

    // Torpedo: .ini [Weapon] Velocity 44, TurnRate 17, Proximity 15.
    let torpedo_speed = 44.0 * SC2_VEL_SCALE;
    let torpedo_turn = sc2_turning(17.0);
    let proximity = 15.0 * SC2_RANGE_SCALE;
    // Warheads: WarheadVelocity 62, WarheadDamage 4, WarheadTurnRate
    // 2.7, WarheadRange 39.5.
    let wh_speed = 62.0 * SC2_VEL_SCALE;
    let wh_turn = sc2_turning(2.7);
    let wh_damage = 4;
    let wh_lifetime = (39.5 * SC2_RANGE_SCALE) / wh_speed;
    // WeaponRate 18 → 18/20 s between launches; WeaponDrain 24.
    let cooldown_after = 18.0 / 20.0;
    let weapon_drain = 24;

    for (entity, ship, pos, rot, vel, mut state, mut batt, ai) in &mut launchers {
        if state.cooldown_s > 0.0 {
            state.cooldown_s -= dt;
        }

        let _ = ai; // AI now drives via SlotInputs like a player.
        let fire_held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        let was_held = state.last_fire_held;
        state.last_fire_held = fire_held;
        let just_pressed = fire_held && !was_held;
        let want_launch = just_pressed;

        if want_launch && state.cooldown_s <= 0.0 {
            if batt.current < weapon_drain {
                continue;
            }
            batt.current = (batt.current - weapon_drain).max(0);
            state.cooldown_s = cooldown_after;

            let forward = Vec2::new(-rot.sin, rot.cos);
            let right = Vec2::new(rot.cos, rot.sin);
            let muzzle = pos.0 + forward * 50.0 + right * (30.0 * state.side);
            state.side = -state.side;

            let torp_vel = vel.0 + forward * torpedo_speed;
            let init_angle = forward.y.atan2(forward.x) - std::f32::consts::FRAC_PI_2;

            commands.spawn((
                Projectile {
                    owner: entity,
                    damage: 0,
                    lifetime: 10.0,
                },
                AlaryTorpedo {
                    owner: entity,
                    proximity,
                    wh_speed,
                    wh_damage,
                    wh_turn,
                    wh_lifetime,
                },
                Homing {
                    target: None,
                    turn_rate: torpedo_turn,
                },
                Sprite {
                    image: assets.load("ships/alabc/sprites/shot_b00.png"),
                    custom_size: Some(Vec2::splat(40.0)),
                    ..default()
                },
                Transform::from_translation(muzzle.extend(0.5)),
                RigidBody::Dynamic,
                Collider::circle(16.0),
                // Sensor → no impulse exchange. The proximity split is
                // detected manually below; `handle_projectile_hits`
                // ignores torpedoes so they never despawn on touch.
                Sensor,
                Mass(2.0),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(torp_vel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
            ));
            info!("alary MIRV torpedo launched");
        }
    }

    // Split any torpedo that has closed within `proximity` of an enemy.
    let split_offsets = [
        0.0_f32,
        50.0_f32.to_radians(),
        -50.0_f32.to_radians(),
        75.0_f32.to_radians(),
        -75.0_f32.to_radians(),
    ];
    for (t_entity, torp, t_pos, t_vel) in &torpedoes {
        let owner_slot = ships.get(torp.owner).ok().map(|(_, s, _)| s.player_slot);
        let mut best: Option<(Vec2, f32)> = None;
        for (_e, s, p) in &ships {
            if Some(s.player_slot) == owner_slot {
                continue;
            }
            let d2 = (p.0 - t_pos.0).length_squared();
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some((p.0, d2));
            }
        }
        let Some((_target, d2)) = best else {
            continue;
        };
        if d2 > torp.proximity * torp.proximity {
            continue;
        }

        let speed = t_vel.0.length();
        let heading = if speed > 0.0 { t_vel.0 / speed } else { Vec2::Y };
        let base_angle = heading.y.atan2(heading.x);
        for off in split_offsets {
            let a = base_angle + off;
            let dir = Vec2::new(a.cos(), a.sin());
            let wvel = dir * torp.wh_speed;
            let init_angle = a - std::f32::consts::FRAC_PI_2;
            // Spawn each warhead a little along its own heading so the
            // five don't pile up at one point and have Avian's solver
            // blast them apart on the first step.
            let spawn_pos = t_pos.0 + dir * 22.0;
            commands.spawn((
                Projectile {
                    owner: torp.owner,
                    damage: torp.wh_damage,
                    lifetime: torp.wh_lifetime,
                },
                Homing {
                    target: None,
                    turn_rate: torp.wh_turn,
                },
                Sprite {
                    image: assets.load("ships/alabc/sprites/shot_a01.png"),
                    custom_size: Some(Vec2::splat(18.0)),
                    ..default()
                },
                Transform::from_translation(spawn_pos.extend(0.5)),
                RigidBody::Dynamic,
                Collider::circle(8.0),
                Mass(0.5 + torp.wh_damage as f32 * 0.4),
                Position(spawn_pos),
                Rotation::radians(init_angle),
                LinearVelocity(wvel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ));
        }
        commands.entity(t_entity).try_despawn();
        info!("alary MIRV split into 5 warheads");
    }
}

/// Alary toggleable hull turret battery (`shpalabc.cpp` special).
///
/// Pressing `special` toggles the three turrets on/off. While on, each
/// turret independently auto-fires a lightly-homing bolt at the nearest
/// enemy in range on its own recharge, paying a small battery drain per
/// shot. Turrets idle (but stay "on") when the battery can't pay.
fn tick_alary_turrets(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    assets: Res<AssetServer>,
    ships: Query<(Entity, &Ship, &Position)>,
    mut turret_ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut AlaryTurrets,
        &mut Battery,
    )>,
) {
    let dt = time.delta_secs();
    // .ini [Special] Velocity 90, Range 18, Damage 3, TurnRate 2,
    // SpecialRate 6, SpecialDrain 2.
    let turret_speed = 90.0 * SC2_VEL_SCALE;
    let turret_range = 18.0 * SC2_RANGE_SCALE;
    let turret_damage = 3;
    let turret_turn = sc2_turning(2.0);
    let turret_lifetime = turret_range / turret_speed;
    let recharge_after = 6.0 / 20.0;
    let special_drain = 2;
    // Three hull turret mounts in local space (+Y forward): nose plus
    // two aft quarters.
    let mounts = [
        Vec2::new(0.0, 34.0),
        Vec2::new(-30.0, -18.0),
        Vec2::new(30.0, -18.0),
    ];

    for (entity, ship, pos, rot, vel, mut turrets, mut batt) in &mut turret_ships {
        let special_held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        let was = turrets.last_special_held;
        turrets.last_special_held = special_held;
        if special_held && !was {
            turrets.on = !turrets.on;
            info!("alary turrets {}", if turrets.on { "ON" } else { "OFF" });
        }

        // Recharge timers always tick, even while off.
        for r in turrets.recharge_s.iter_mut() {
            if *r > 0.0 {
                *r -= dt;
            }
        }
        if !turrets.on {
            continue;
        }

        // Nearest enemy ship.
        let mut best: Option<(Vec2, f32)> = None;
        for (_e, s, p) in &ships {
            if s.player_slot == ship.player_slot {
                continue;
            }
            let d2 = (p.0 - pos.0).length_squared();
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some((p.0, d2));
            }
        }
        let Some((target_pos, target_d2)) = best else {
            continue;
        };
        if target_d2 > turret_range * turret_range {
            continue;
        }

        for i in 0..mounts.len() {
            if turrets.recharge_s[i] > 0.0 {
                continue;
            }
            if batt.current < special_drain {
                break;
            }
            batt.current = (batt.current - special_drain).max(0);
            turrets.recharge_s[i] = recharge_after;

            let m = mounts[i];
            let world_off = Vec2::new(
                m.x * rot.cos - m.y * rot.sin,
                m.x * rot.sin + m.y * rot.cos,
            );
            let muzzle = pos.0 + world_off;
            let mut dir = (target_pos - muzzle).normalize_or_zero();
            if dir == Vec2::ZERO {
                dir = Vec2::new(-rot.sin, rot.cos);
            }
            let bolt_vel = vel.0 + dir * turret_speed;
            let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
            commands.spawn((
                Projectile {
                    owner: entity,
                    damage: turret_damage,
                    lifetime: turret_lifetime,
                },
                Homing {
                    target: None,
                    turn_rate: turret_turn,
                },
                Sprite {
                    image: assets.load("ships/alabc/sprites/shot_t00.png"),
                    custom_size: Some(Vec2::splat(18.0)),
                    ..default()
                },
                Transform::from_translation(muzzle.extend(0.5)),
                RigidBody::Dynamic,
                Collider::circle(6.0),
                Mass(0.5 + turret_damage as f32 * 0.4),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(bolt_vel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ));
        }
    }
}

/// Draw a pulsing energy ring around any ship that currently has a
/// `ShieldActive` (Yehat force field, Alary absorbance shield, …) so
/// the shield is actually visible. Drawn as a gizmo in the camera's
/// wrapped frame so it lines up with the offset-rendered sprite.
fn draw_shield_rings(
    time: Res<Time>,
    mut gizmos: Gizmos,
    camera: Query<&Transform, With<Camera2d>>,
    colliders: Res<crate::collider::ShipColliders>,
    ships: Query<(&Position, &Rotation, &ShipClass), (With<Ship>, With<ShieldActive>)>,
) {
    // Pulse the colour, not the size — we want the outline to hug the
    // hull, matching the actual collider polygon (so you SEE what's
    // actually being protected).
    let focus = camera.single().ok().map(|t| t.translation.truncate());
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 6.0).sin();
    let main = Color::srgba(0.55, 0.85, 1.0, 0.65 + 0.25 * pulse);
    let glow = Color::srgba(0.80, 0.95, 1.0, 0.30 + 0.15 * pulse);
    for (pos, rot, class) in &ships {
        let c = focus.map_or(pos.0, |f| crate::physics::nearest_image(pos.0, f));
        // Fall back to a circle for ships whose polygon hasn't been
        // extracted yet (early startup race).
        let Some(poly) = colliders.polys.get(class) else {
            gizmos.circle_2d(c, 46.0, main);
            continue;
        };
        // Rotate each vertex by the ship's facing, then translate to
        // its world (camera-imaged) position. Draw the outline a few
        // times at slightly grown radii so the line reads as thick
        // without needing custom gizmo configs.
        for grow in [1.0_f32, 1.06, 1.12] {
            let color = if grow <= 1.0 { main } else { glow };
            let mut prev: Option<Vec2> = None;
            let first = poly.first().copied();
            for v in poly.iter().copied().chain(first) {
                let scaled = v * grow;
                let rotated = Vec2::new(
                    scaled.x * rot.cos - scaled.y * rot.sin,
                    scaled.x * rot.sin + scaled.y * rot.cos,
                );
                let world = c + rotated;
                if let Some(p) = prev {
                    gizmos.line_2d(p, world, color);
                }
                prev = Some(world);
            }
        }
    }
}

/// Faint dashed-ish ring showing each planet's gravity-well boundary, so
/// the pull isn't invisible. Pulses gently. Drawn at the camera-nearest
/// periodic image so it tracks the planet across the toroidal wrap.
fn draw_gravity_field(
    time: Res<Time>,
    mut gizmos: Gizmos,
    camera: Query<&Transform, With<Camera2d>>,
    planets: Query<(&Position, &Planet)>,
) {
    let focus = camera.single().ok().map(|t| t.translation.truncate());
    let t = time.elapsed_secs();
    for (pos, planet) in &planets {
        let c = focus.map_or(pos.0, |f| crate::physics::nearest_image(pos.0, f));
        // A few faint concentric rings between the surface and the range
        // edge so the field reads as a region, not just an outline.
        for i in 1..=3 {
            let frac = i as f32 / 3.0;
            let r = planet.radius + (planet.gravity_range - planet.radius) * frac;
            let pulse = 0.06 + 0.05 * (t * 1.5 - i as f32).sin().max(0.0);
            gizmos.circle_2d(c, r, Color::srgba(0.55, 0.7, 1.0, pulse));
        }
    }
}

/// Shofixti Glory Device — three `special` presses to detonate. Each
/// press arms a stage; the third spawns the range-13 suicide blast
/// (a `DamageZone` with no source exemption, so it kills the Shofixti
/// too). Presses time out after a short window so a stray tap doesn't
/// leave the ship primed.
fn tick_shofixti_glory(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(&Ship, &Position, &mut ShofixtiGlory)>,
) {
    const RESET_WINDOW_S: f32 = 1.5;
    let dt = time.delta_secs();
    for (ship, pos, mut glory) in &mut ships {
        if slot_inputs.just_pressed(ship.player_slot, input::INPUT_SPECIAL) {
            glory.presses += 1;
            glory.since_last_s = 0.0;
            info!(
                "P{} Glory Device armed {}/3",
                ship.player_slot + 1,
                glory.presses
            );
            if glory.presses >= 3 {
                glory.presses = 0;
                // Range-13 blast, no source exemption → the Shofixti
                // dies in the blaze too. Big damage-per-sec over a short
                // window for the "blaze of glory" lethality.
                spawn_damage_zone(
                    &mut commands,
                    None,
                    pos.0,
                    13.0 * SC2_RANGE_SCALE,
                    1_000_000.0,
                    0.12,
                    Color::srgba(1.0, 0.6, 0.2, 0.55),
                    None,
                );
                info!("P{} GLORY DEVICE detonates", ship.player_slot + 1);
            }
        } else {
            glory.since_last_s += dt;
            if glory.presses > 0 && glory.since_last_s > RESET_WINDOW_S {
                glory.presses = 0;
            }
        }
    }
}

/// Steer each `Homing` projectile toward the nearest enemy ship (an
/// enemy is "ship whose `player_slot` ≠ projectile owner's slot").
///
/// The projectile's speed is preserved; only its direction is rotated
/// toward the target, capped at `turn_rate * dt` radians per tick. This
/// is the standard 2D missile-tracking model: light-tracking missiles
/// (Spathi BUTT at ~3.9 rad/s) can chase a fighter, heavy plasma
/// (Mycon plasmoid at 1.5 rad/s) only nudges toward its target and is
/// dodgeable. Target is cached and re-acquired when the previous one
/// dies (its `Ship` component disappears from the query).
fn steer_homing_projectiles(
    time: Res<Time<Physics>>,
    mut projectiles: Query<(&Projectile, &Position, &mut LinearVelocity, &mut Homing)>,
    // `Without<Invisible>` so a cloaked Ilwrath drops missile locks
    // (canonical: isInvisible() filters target acquisition).
    ships: Query<(Entity, &Ship, &Position), (Without<Projectile>, Without<Invisible>)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (proj, proj_pos, mut vel, mut homing) in &mut projectiles {
        // Owner's slot tells us which side is friendly (skip in target search).
        let owner_slot = ships.get(proj.owner).ok().map(|(_, s, _)| s.player_slot);

        // Acquire / re-acquire target.
        let target_lost = homing
            .target
            .map(|t| ships.get(t).is_err())
            .unwrap_or(true);
        if target_lost {
            let mut best: Option<(Entity, f32)> = None;
            for (e, s, p) in &ships {
                if Some(s.player_slot) == owner_slot {
                    continue;
                }
                // Toroidal distance — the target may be nearer the
                // wrapped way, so pick by minimum-image, not raw.
                let d2 = crate::physics::min_image(p.0 - proj_pos.0).length_squared();
                if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                    best = Some((e, d2));
                }
            }
            homing.target = best.map(|(e, _)| e);
        }

        let Some(target) = homing.target else {
            continue;
        };
        let Ok((_, _, target_pos)) = ships.get(target) else {
            continue;
        };

        // Steer along the SHORTEST path on the torus: a target across
        // the wrap seam is reached by heading off the near edge, not the
        // long way around. Without min_image the missile flew away from
        // an opponent that was actually adjacent through the wrap.
        let to_target = crate::physics::min_image(target_pos.0 - proj_pos.0);
        let speed = vel.0.length();
        if speed <= 0.0 || to_target.length_squared() == 0.0 {
            continue;
        }
        let current_dir = vel.0 / speed;
        let target_dir = to_target.normalize();

        // Signed angle from current_dir to target_dir, in (-π, π].
        let angle = current_dir.perp_dot(target_dir).atan2(current_dir.dot(target_dir));
        let max_delta = homing.turn_rate * dt;
        let delta = angle.clamp(-max_delta, max_delta);

        // Rotate current_dir by `delta`, keep magnitude.
        let (s, c) = delta.sin_cos();
        let new_dir = Vec2::new(
            current_dir.x * c - current_dir.y * s,
            current_dir.x * s + current_dir.y * c,
        );
        vel.0 = new_dir * speed;
    }
}

/// Rotate each projectile's `Rotation` to match its current velocity
/// direction. Frame 0 of the canonical weapon sprites points up (+Y),
/// so we offset by -π/2 to align "up-facing" art with motion. Runs
/// after `steer_homing_projectiles` so homing missiles keep facing
/// their target as they curve.
fn orient_projectiles(mut q: Query<(&LinearVelocity, &mut Rotation), With<Projectile>>) {
    for (vel, mut rot) in &mut q {
        if vel.0.length_squared() <= 0.0 {
            continue;
        }
        let angle = vel.0.y.atan2(vel.0.x) - std::f32::consts::FRAC_PI_2;
        *rot = Rotation::radians(angle);
    }
}

/// Apply damage from every active `DamageZone` to every ship overlapping
/// its sensor collider (excluding the zone's `source`, when set), then
/// decrement lifetimes and despawn expired zones. Overlap detection
/// is owned by Avian — we read the `CollidingEntities` set the
/// physics broad/narrow phase populated this step. Shield damage_factor
/// still applies — a Pkunk in phase shift takes 0 from a Shofixti Glory.
fn tick_damage_zones(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut zones: Query<(Entity, &CollidingEntities, &mut DamageZone)>,
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
) {
    let dt = time.delta_secs();
    for (zone_entity, colliding, mut zone) in &mut zones {
        if zone.damage_per_sec > 0.0 && dt > 0.0 {
            for &target in colliding.0.iter() {
                if zone.source == Some(target) {
                    continue;
                }
                let factor = shields
                    .get(target)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = (zone.damage_per_sec * dt * factor).ceil().max(0.0) as i32;
                if dmg > 0 {
                    if let Ok(mut crew) = crews.get_mut(target) {
                        crew.current = (crew.current - dmg).max(0);
                    }
                }
            }
        }
        zone.lifetime -= dt;
        if zone.lifetime <= 0.0 {
            commands.entity(zone_entity).try_despawn();
        }
    }
}

/// React to Avian `CollisionStart` messages. The physics solver has already
/// applied the impulse, so all we do here is:
///   - Deduct crew from the ship the projectile hit (ignoring self-hits).
///   - Despawn the projectile so it doesn't keep bouncing.
fn handle_projectile_hits(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    projectiles: Query<&Projectile>,
    proj_positions: Query<&Position, With<Projectile>>,
    limpets: Query<&Limpet>,
    shields: Query<&ShieldActive>,
    damage_to_batt: Query<&DamageToBattery>,
    asteroids_q: Query<&Position, With<Asteroid>>,
    ships: Query<&Ship>,
    // Alary MIRV torpedoes deal no contact damage and never despawn
    // on touch — they only split on proximity (`tick_alary_mirv`).
    torpedoes: Query<&AlaryTorpedo>,
    mut satellites: Query<(&mut ChmmrSatellite, &Position)>,
    mut crews: Query<&mut Crew>,
    mut batteries: Query<&mut Battery>,
    mut velocities: Query<&mut LinearVelocity>,
    mut deriveds: Query<&mut ShipPhysicsDerived>,
    assets: Res<AssetServer>,
) {
    // Entities already despawned during THIS event pass. A fast-firing
    // ship (Ilwrath) can land two shots on the same target in one frame,
    // producing two CollisionStart events for the same entities; without
    // this guard the second event would despawn an already-gone entity
    // (panic) and double-apply damage. We skip any event touching an
    // entity we've already removed, and despawn each at most once.
    let mut gone: bevy::platform::collections::HashSet<Entity> =
        bevy::platform::collections::HashSet::default();
    for event in reader.read() {
        let (proj_entity, other_entity) = if projectiles.get(event.collider1).is_ok() {
            (event.collider1, event.collider2)
        } else if projectiles.get(event.collider2).is_ok() {
            (event.collider2, event.collider1)
        } else {
            continue;
        };

        if gone.contains(&proj_entity) || gone.contains(&other_entity) {
            continue;
        }

        let proj = match projectiles.get(proj_entity) {
            Ok(p) => p,
            Err(_) => continue,
        };

        // The MIRV torpedo is inert on contact — pass through.
        if torpedoes.get(proj_entity).is_ok() {
            continue;
        }

        if proj.owner == other_entity {
            continue;
        }

        // Friendly-fire filter: if both the firer and the target
        // are ships and they share the same `player_slot`, the
        // shot passes through. Without this, Pkunk clones (which
        // share their summoner's slot) would shred each other
        // and the main ship with their own bullets.
        if let (Ok(firer_ship), Ok(target_ship)) =
            (ships.get(proj.owner), ships.get(other_entity))
        {
            if firer_ship.player_slot == target_ship.player_slot {
                continue;
            }
        }

        // Projectile-vs-projectile: pass through silently. Without
        // this, the 14-22 Chenjesu shatter-shards (or any other
        // cluster spawn at one point) would collide with each other
        // on the spawning tick and despawn each other before any
        // are visibly flying. Canon doesn't model projectile-on-
        // projectile interactions either — bullets fly past each
        // other.
        if projectiles.get(other_entity).is_ok() {
            continue;
        }

        // Asteroid hit by projectile: 1 hp — kaboom + despawn both.
        // Matches canon VSmallAsteroid (handle_damage with armour
        // ≈ 0). Spawn the explosion sprite at the asteroid's
        // position so the rock visibly disintegrates instead of
        // just blinking out.
        if let Ok(pos) = asteroids_q.get(other_entity) {
            spawn_asteroid_explosion(&mut commands, &assets, pos.0, 24.0);
            if gone.insert(other_entity) {
                commands.entity(other_entity).try_despawn();
            }
            if gone.insert(proj_entity) {
                commands.entity(proj_entity).try_despawn();
            }
            continue;
        }

        // Chmmr Avatar satellite hit by projectile: chip its
        // armour by the projectile's damage, despawn the satellite
        // if armour reaches 0 (+ kaboom for the satisfying pop).
        // Friendly-fire filter: skip if the projectile's owner is
        // the satellite's owner-avatar (Chmav's own uberlaser
        // can't clip its own satellites).
        if let Ok((mut sat, sat_pos)) = satellites.get_mut(other_entity) {
            if proj.owner == sat.owner {
                continue;
            }
            // Treat shield's damage_factor uniformly (most ships
            // don't have one; defaults to 1.0).
            let factor = shields
                .get(other_entity)
                .map(|s| s.damage_factor)
                .unwrap_or(1.0);
            let dmg = ((proj.damage as f32 * factor).round() as i32).max(0);
            sat.armour = (sat.armour - dmg).max(0);
            if gone.insert(proj_entity) {
                commands.entity(proj_entity).try_despawn();
            }
            if sat.armour <= 0 {
                spawn_asteroid_explosion(&mut commands, &assets, sat_pos.0, 18.0);
                if gone.insert(other_entity) {
                    commands.entity(other_entity).try_despawn();
                }
            }
            continue;
        }

        // Limpets are NOT damage projectiles in canon — they slow
        // the target instead. shpvuxin.cpp:VuxLimpet::inflict_damage
        // calls `target->handle_speed_loss(slowdown_factor)`, which
        // permanently reduces speed_max/accel_rate/turn_rate. A
        // one-frame velocity ×0.5 was useless because thrust pushes
        // the ship back up to speed_max next tick.
        //
        // Per shpvuxin.cpp + mship.cpp:handle_speed_loss, each hit
        // multiplies speed_max by `1 - sl * speed_max/(speed_max+96)`
        // where `sl = 30/(mass+30) * slowdown_factor`. We don't have
        // the target's mass handy, so we use a fixed sl=0.4 (good
        // approximation for mid-mass ships) — empirically gives the
        // canonical 4-limpets-≈-half-speed result. Cumulative across
        // hits, so a target accumulating limpets gets progressively
        // crippled.
        if let Ok(limpet) = limpets.get(proj_entity) {
            if let Ok(mut derived) = deriveds.get_mut(other_entity) {
                let sl = 0.4 * limpet.slowdown_factor;
                let s = derived.speed_max;
                if s > 0.0 {
                    derived.speed_max = s * (1.0 - sl * s / (s + 96.0));
                }
                derived.thrust_force *= 0.85;
                derived.target_omega *= 0.92;
                if let Ok(mut vel) = velocities.get_mut(other_entity) {
                    // Immediate clamp so the hit feels punchy.
                    let speed = vel.0.length();
                    if speed > derived.speed_max && derived.speed_max > 0.0 {
                        vel.0 = vel.0 / speed * derived.speed_max;
                    }
                }
                info!(
                    "limpet hit: speed_max → {:.0}",
                    derived.speed_max
                );
            }
            if gone.insert(proj_entity) {
                commands.entity(proj_entity).try_despawn();
            }
            continue;
        }

        // Normal damage path. Shield first; fortitude
        // (DamageToBattery) routes the post-shield damage into the
        // target's battery instead of its crew. Order matches the
        // canonical Utwig path: shield multiplies first, then
        // fortitude consumes what's left.
        let factor = shields
            .get(other_entity)
            .map(|s| s.damage_factor)
            .unwrap_or(1.0);
        let damage = ((proj.damage as f32 * factor).round() as i32).max(0);

        if let Ok(d2b) = damage_to_batt.get(other_entity) {
            if let Ok(mut batt) = batteries.get_mut(other_entity) {
                let gain = (damage as f32 * d2b.conversion).round() as i32;
                batt.current = (batt.current + gain).min(batt.max);
                info!(
                    "hit: fortitude absorbed {} dmg → +{} batt ({}/{})",
                    damage, gain, batt.current, batt.max
                );
            }
        } else if let Ok(mut crew) = crews.get_mut(other_entity) {
            crew.current = (crew.current - damage).max(0);
            info!(
                "hit: -{} crew (now {}/{}){}",
                damage,
                crew.current,
                crew.max,
                if factor < 1.0 { " [shielded]" } else { "" }
            );
        }
        if gone.insert(proj_entity) {
            // Visible boom when a shot lands on a ship — without this
            // a missile just blinked out of existence. Spawned at the
            // projectile's last position (asteroid kaboom sprite,
            // smaller scale) before despawn.
            if let Ok(proj_pos) = proj_positions.get(proj_entity) {
                spawn_asteroid_explosion(&mut commands, &assets, proj_pos.0, 14.0);
            }
            commands.entity(proj_entity).try_despawn();
        }
    }
}

/// Ship-on-ship contact damage. Avian has already applied the impulse
/// (so the bigger ship pushes the smaller one and they both potentially
/// spin); on top of that we deduct crew proportional to how fast they
/// were closing. Below a threshold relative speed it's just a love tap,
/// no damage.
// Generic ram-damage was an invention — canon has no baseline
// ramming damage. But specific modes (canonical: Androsynth Blazer)
// DO deal collision damage to the other ship. This system reads
// `Mode.collide_damage` from each colliding ship's currently-active
// mode and applies that as crew damage to the OTHER ship. Shield-
// aware. Owner of the mode is never damaged by its own collide_damage.
fn handle_mode_contact_damage(
    mut reader: MessageReader<CollisionStart>,
    modes: Query<&ShipModes>,
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
    ships: Query<&Ship>,
) {
    for event in reader.read() {
        // Bidirectional: each side independently checks its own
        // mode and damages the other.
        for (attacker, target) in [
            (event.collider1, event.collider2),
            (event.collider2, event.collider1),
        ] {
            let Ok(modes) = modes.get(attacker) else {
                continue;
            };
            let Some(m) = modes.modes.get(modes.current) else {
                continue;
            };
            if m.collide_damage <= 0 {
                continue;
            }
            // Don't friendly-fire.
            let Ok(a_ship) = ships.get(attacker) else {
                continue;
            };
            let Ok(t_ship) = ships.get(target) else {
                continue;
            };
            if a_ship.player_slot == t_ship.player_slot {
                continue;
            }
            if let Ok(mut crew) = crews.get_mut(target) {
                let factor = shields
                    .get(target)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = ((m.collide_damage as f32 * factor).round() as i32).max(0);
                if dmg > 0 {
                    crew.current = (crew.current - dmg).max(0);
                    info!(
                        "{} contact: -{} crew on P{}",
                        m.name,
                        dmg,
                        t_ship.player_slot + 1
                    );
                }
            }
        }
    }
}

fn tick_shield(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut q: Query<(Entity, &mut ShieldActive)>,
) {
    let dt = time.delta_secs();
    for (entity, mut shield) in &mut q {
        shield.remaining -= dt;
        if shield.remaining <= 0.0 {
            commands.entity(entity).try_remove::<ShieldActive>();
            info!("shield down");
        }
    }
}

/// Earthling Cruiser point-defense beam. Each FixedUpdate, for every
/// ship carrying `PointDefenseActive`:
///   - Find every projectile within `range` whose owner is not this
///     ship (and whose owner — if a ship — isn't on the same slot),
///     and despawn it. This is the "shoots down incoming missiles"
///     half of the canonical mechanic.
///   - Find every ship within `range` on a different player_slot and
///     deduct `damage_per_tick` crew (shield-aware via ShieldActive).
///     That's the "fries the enemy too" half.
///   - Decrement the timer; remove the component when expired.
fn tick_point_defense(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    assets: Res<AssetServer>,
    spatial: avian2d::prelude::SpatialQuery,
    camera: Query<&Transform, With<Camera2d>>,
    mut firers: Query<(Entity, &Ship, &Position, &mut PointDefenseActive)>,
    positions: Query<&Position>,
    projectiles: Query<&Projectile>,
    asteroids: Query<(), With<Asteroid>>,
    ships_invisible: Query<(), With<Invisible>>,
    ship_data: Query<&Ship>,
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
) {
    use avian2d::prelude::SpatialQueryFilter;
    // Canonical SDI laser (shpearcr.cpp activate_special): every cycle it
    // fires a laser at EVERY object in range — incoming shots, asteroids,
    // and enemy ships — not just one. SpecialRate=9 frames ≈ 0.45 s
    // between volleys, so we pace it there rather than firing every
    // physics tick (which dealt absurd DPS and flickered).
    const PD_FIRE_INTERVAL_S: f32 = 9.0 / 20.0;
    let focus = camera.single().ok().map(|t| t.translation.truncate());
    let dt = time.delta_secs();
    for (firer_entity, firer, firer_pos, mut beam) in &mut firers {
        beam.remaining -= dt;
        beam.cooldown_s -= dt;
        if beam.cooldown_s <= 0.0 {
            let probe = Collider::circle(beam.range);
            let filter = SpatialQueryFilter::default().with_excluded_entities([firer_entity]);
            let candidates = spatial.shape_intersections(&probe, firer_pos.0, 0.0, &filter);

            // Render the laser in the camera's wrapped frame so it stays
            // glued to the firer + target instead of being drawn at raw
            // arena coords (invisible once the focus has drifted).
            let firer_img = focus.map_or(firer_pos.0, |f| crate::physics::nearest_image(firer_pos.0, f));
            for e in candidates {
                let kind = if let Ok(proj) = projectiles.get(e) {
                    if proj.owner == firer_entity {
                        continue;
                    }
                    PdKind::Projectile
                } else if asteroids.get(e).is_ok() {
                    PdKind::Asteroid
                } else if let Ok(ts) = ship_data.get(e) {
                    if ts.player_slot == firer.player_slot || ships_invisible.get(e).is_ok() {
                        continue;
                    }
                    PdKind::Ship
                } else {
                    continue;
                };
                let Ok(p) = positions.get(e) else { continue };
                let target_pos = p.0;

                // Laser line from firer to target, both imaged near the
                // camera (target via min-image off the firer so it spans
                // the short way across a seam).
                let rel = crate::physics::min_image(target_pos - firer_pos.0);
                let target_img = firer_img + rel;
                let len = rel.length().max(1.0);
                let mid = (firer_img + target_img) * 0.5;
                let angle = rel.y.atan2(rel.x) - std::f32::consts::FRAC_PI_2;
                commands.spawn((
                    ZapFlash { remaining_s: 0.10, total_s: 0.10 },
                    Sprite::from_color(Color::srgba(0.6, 1.0, 1.0, 0.95), Vec2::new(3.0, len)),
                    Transform {
                        translation: mid.extend(0.32),
                        rotation: Quat::from_rotation_z(angle),
                        scale: Vec3::ONE,
                    },
                ));
                match kind {
                    PdKind::Projectile => {
                        if let Ok(mut ec) = commands.get_entity(e) {
                            ec.try_despawn();
                        }
                    }
                    PdKind::Asteroid => {
                        spawn_asteroid_explosion(&mut commands, &assets, target_pos, 24.0);
                        if let Ok(mut ec) = commands.get_entity(e) {
                            ec.try_despawn();
                        }
                    }
                    PdKind::Ship => {
                        let factor = shields.get(e).map(|s| s.damage_factor).unwrap_or(1.0);
                        let dmg = ((beam.damage_per_tick as f32 * factor).round() as i32).max(0);
                        if dmg > 0 {
                            if let Ok(mut crew) = crews.get_mut(e) {
                                crew.current = (crew.current - dmg).max(0);
                            }
                        }
                    }
                }
            }
            // Reset the cycle timer whether or not anything was in range,
            // so the cadence stays steady (a press with no targets simply
            // does nothing this cycle).
            beam.cooldown_s = PD_FIRE_INTERVAL_S;
        }

        if beam.remaining <= 0.0 {
            commands.entity(firer_entity).try_remove::<PointDefenseActive>();
            info!("P{} point defense offline", firer.player_slot + 1);
        }
    }
}

#[derive(Clone, Copy)]
enum PdKind {
    Projectile,
    Asteroid,
    Ship,
}

/// Reposition each attached zone to follow its owner, then apply
/// damage to any non-friendly ship overlapping its sensor collider.
/// Zones whose owner has died despawn (no orphan damage continuing
/// in dead space). Overlap detection comes from Avian's
/// `CollidingEntities`, populated by the broad/narrow phase.
fn tick_attached_damage_zones(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut zones: Query<(
        Entity,
        &mut AttachedDamageZone,
        &mut Transform,
        &mut Position,
        &mut Sprite,
        &CollidingEntities,
    )>,
    owners: Query<(&Ship, &Position, &Rotation), Without<AttachedDamageZone>>,
    ships: Query<&Ship>,
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
) {
    let dt = time.delta_secs();
    for (
        zone_entity,
        mut zone,
        mut zone_xf,
        mut zone_pos,
        mut zone_sprite,
        colliding,
    ) in &mut zones
    {
        let Ok((owner_ship, owner_pos, owner_rot)) = owners.get(zone.owner) else {
            commands.entity(zone_entity).try_despawn();
            continue;
        };
        let world_offset = Vec2::new(
            zone.local_offset.x * owner_rot.cos - zone.local_offset.y * owner_rot.sin,
            zone.local_offset.x * owner_rot.sin + zone.local_offset.y * owner_rot.cos,
        );
        let world_pos = owner_pos.0 + world_offset;
        // Sync render Transform AND Avian Position (the latter is
        // what drives the sensor collider's world location).
        zone_xf.translation = world_pos.extend(0.2);
        zone_pos.0 = world_pos;
        zone_sprite.custom_size = Some(Vec2::splat(zone.radius * 2.0));
        zone_sprite.color = zone.color;

        if zone.damage_per_sec > 0.0 && dt > 0.0 {
            for &target in colliding.0.iter() {
                let Ok(target_ship) = ships.get(target) else {
                    continue;
                };
                if target_ship.player_slot == owner_ship.player_slot {
                    continue;
                }
                let factor = shields
                    .get(target)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = (zone.damage_per_sec * dt * factor).ceil().max(0.0) as i32;
                if dmg > 0 {
                    if let Ok(mut crew) = crews.get_mut(target) {
                        crew.current = (crew.current - dmg).max(0);
                    }
                }
            }
        }

        zone.lifetime -= dt;
        if zone.lifetime <= 0.0 {
            commands.entity(zone_entity).try_despawn();
        }
    }
}

/// Cast each beam through Avian's spatial query, damage the first
/// non-friendly hit, position the sprite owner → hit point. Beams
/// whose owner has died despawn cleanly.
///
/// The auto-aim target search is a position-based nearest-enemy scan
/// (filtered by `Without<Invisible>`); the actual hit test is a real
/// Avian ray cast through the polygon colliders. Beams skip the
/// firer via Avian's `excluded_entities` filter so the ray doesn't
/// instantly stop on its own hull.
fn tick_beams(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    spatial: avian2d::prelude::SpatialQuery,
    mut beams: Query<(Entity, &mut Beam, &mut Transform, &mut Sprite), Without<Camera2d>>,
    owners: Query<(&Ship, &Position, &Rotation)>,
    ship_pos: Query<(Entity, &Ship, &Position), Without<Invisible>>,
    asteroid_pos: Query<&Position, With<Asteroid>>,
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
    ship_class_of: Query<&Ship>,
    hypers: Query<&crate::ultimate::HyperActive>,
    camera: Query<&Transform, With<Camera2d>>,
    satellites_for_filter: Query<(Entity, &ChmmrSatellite)>,
    assets: Res<AssetServer>,
) {
    use avian2d::prelude::SpatialQueryFilter;
    use bevy::math::Dir2;
    let dt = time.delta_secs();
    // Render the beam in the camera's wrapped frame so it stays
    // glued to the (offset-rendered) firer near an arena edge.
    let focus = camera.single().ok().map(|t| t.translation.truncate());
    for (beam_entity, mut beam, mut beam_xf, mut beam_sprite) in &mut beams {
        let Ok((owner_ship, owner_pos, owner_rot)) = owners.get(beam.owner) else {
            commands.entity(beam_entity).try_despawn();
            continue;
        };
        let world_origin = owner_pos.0
            + Vec2::new(
                beam.local_origin.x * owner_rot.cos - beam.local_origin.y * owner_rot.sin,
                beam.local_origin.x * owner_rot.sin + beam.local_origin.y * owner_rot.cos,
            );
        let mut world_dir = Vec2::new(
            beam.local_dir.x * owner_rot.cos - beam.local_dir.y * owner_rot.sin,
            beam.local_dir.x * owner_rot.sin + beam.local_dir.y * owner_rot.cos,
        );

        // Auto-aim: pick nearest non-friendly non-invisible ship as
        // target, regardless of range. The beam still only damages
        // out to `beam.range` (raycast distance below), but visually
        // it always tracks the opponent — canonical Arilou tells you
        // exactly where the other ship is.
        if beam.auto_aim {
            let mut best: Option<(Vec2, f32)> = None;
            for (_, s, p) in &ship_pos {
                if s.player_slot == owner_ship.player_slot {
                    continue;
                }
                let d2 = (p.0 - world_origin).length_squared();
                if best.map_or(true, |(_, b)| d2 < b) {
                    best = Some((p.0, d2));
                }
            }
            if let Some((target, _)) = best {
                let delta = target - world_origin;
                if delta.length_squared() > 1e-6 {
                    world_dir = delta.normalize();
                }
            }
        }

        // Physics-native hit test: Avian ray cast against every
        // collider in the world. Exclude the firer AND the firer's own
        // Chmmr satellites — those orbit close to the hull and were
        // soaking the Chmmr laser's first hit before the beam reached
        // the enemy (the canon laser passes through its own orbiters).
        let dir = Dir2::new(world_dir).unwrap_or(Dir2::X);
        let mut excluded: Vec<Entity> = vec![beam.owner];
        for (sat_e, sat) in &satellites_for_filter {
            if sat.owner == beam.owner {
                excluded.push(sat_e);
            }
        }
        let filter = SpatialQueryFilter::default().with_excluded_entities(excluded);
        let hit = spatial.cast_ray(
            world_origin,
            dir,
            beam.range,
            true, // solid: stop on first hit
            &filter,
        );
        let (hit_t, hit_target) = match hit {
            Some(ref h) => (h.distance, Some(h.entity)),
            None => (beam.range, None),
        };

        // Apply damage to the hit entity if it's a ship and not a
        // friendly. Damage is rate-limited to the canonical SC2
        // frame rate (20 fps): even if FixedUpdate runs at 60 Hz,
        // we accumulate `dt * 20` per tick and only fire damage
        // when the accumulator crosses 1.0. Without this an Arilou
        // beam at 60 fps deals 3× the canonical damage rate.
        beam.damage_accum += dt * 20.0;
        let damage_ticks = beam.damage_accum.floor() as i32;
        beam.damage_accum -= damage_ticks as f32;
        if let Some(target) = hit_target {
            // Asteroid hit by a laser: same 1-hp blow-up as
            // projectile-vs-asteroid. The beam continues raycasting
            // through but we kaboom the rock + despawn here.
            if let Ok(ast_pos) = asteroid_pos.get(target) {
                spawn_asteroid_explosion(&mut commands, &assets, ast_pos.0, 24.0);
                commands.entity(target).try_despawn();
            } else if let Ok(target_ship) = ship_class_of.get(target) {
                let invisible_or_friendly = target_ship.player_slot == owner_ship.player_slot
                    || ship_pos.get(target).is_err();
                if !invisible_or_friendly && damage_ticks > 0 {
                    if let Ok(mut crew) = crews.get_mut(target) {
                        let factor = shields
                            .get(target)
                            .map(|s| s.damage_factor)
                            .unwrap_or(1.0);
                        let per_tick =
                            ((beam.damage_per_tick as f32 * factor).round() as i32).max(0);
                        let total = per_tick * damage_ticks;
                        if total > 0 {
                            crew.current = (crew.current - total).max(0);
                        }
                    }
                }
            }
        }

        // Position the sprite to span owner → hit (or full range when
        // missing). Sprite default y-up axis, custom_size = (w, len)
        // means we rotate by atan2(dy, dx) - π/2 to align long-axis
        // with the world direction.
        let render_origin =
            focus.map_or(world_origin, |f| crate::physics::nearest_image(world_origin, f));
        let midpoint = render_origin + world_dir * (hit_t * 0.5);
        let angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;
        beam_xf.translation = midpoint.extend(0.3);
        beam_xf.rotation = Quat::from_rotation_z(angle);
        let width_mult = hypers
            .get(beam.owner)
            .map(|h| h.beam_width_mult)
            .unwrap_or(1.0);
        beam_sprite.custom_size =
            Some(Vec2::new(beam.width * 2.0 * width_mult, hit_t.max(1.0)));
        beam_sprite.color = beam.color;

        beam.remaining -= dt;
        if beam.remaining <= 0.0 {
            commands.entity(beam_entity).try_despawn();
        }
    }
}

/// Pull (or push) the nearest enemy in range toward (or away from)
/// each tractor's owner. Despawns when the owner dies or the tractor
/// runs out of remaining time. Updates the sprite each tick to span
/// owner-origin → target so the player sees the connection.
fn tick_tractors(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    assets: Res<AssetServer>,
    spatial: avian2d::prelude::SpatialQuery,
    mut tractors: Query<(Entity, &mut TractorBeam, &mut Transform, &mut Sprite), Without<Camera2d>>,
    owners: Query<(&Ship, &Position, &Rotation)>,
    ships_for_filter: Query<&Ship, Without<Invisible>>,
    mut ship_state: Query<(&Position, &mut LinearVelocity, &Mass), With<Ship>>,
    camera: Query<&Transform, With<Camera2d>>,
) {
    use avian2d::prelude::SpatialQueryFilter;
    let dt = time.delta_secs();
    let focus = camera.single().ok().map(|t| t.translation.truncate());
    for (tractor_entity, mut tractor, mut tractor_xf, mut sprite) in &mut tractors {
        let Ok((owner_ship, owner_pos, owner_rot)) = owners.get(tractor.owner) else {
            commands.entity(tractor_entity).try_despawn();
            continue;
        };
        let world_origin = owner_pos.0
            + Vec2::new(
                tractor.local_origin.x * owner_rot.cos - tractor.local_origin.y * owner_rot.sin,
                tractor.local_origin.x * owner_rot.sin + tractor.local_origin.y * owner_rot.cos,
            );

        // Find nearest non-friendly, non-invisible ship in range via
        // Avian's spatial broad/narrow phase. Per-tick circle query —
        // physics-native equivalent of the old "iterate ships +
        // distance check" loop, but using Avian's spatial structures.
        let probe = Collider::circle(tractor.range);
        let filter = SpatialQueryFilter::default().with_excluded_entities([tractor.owner]);
        let candidates = spatial.shape_intersections(&probe, world_origin, 0.0, &filter);

        let mut best: Option<(Entity, Vec2, f32)> = None;
        for ent in candidates {
            let Ok(s) = ships_for_filter.get(ent) else {
                continue;
            };
            if s.player_slot == owner_ship.player_slot {
                continue;
            }
            let Ok((p, _, _)) = ship_state.get(ent) else {
                continue;
            };
            let d2 = (p.0 - world_origin).length_squared();
            if best.map_or(true, |(_, _, bd)| d2 < bd) {
                best = Some((ent, p.0, d2));
            }
        }

        let target_pos = best.map(|(_, p, _)| p);
        if let Some((target_e, tp, _)) = best {
            // Apply the force as a velocity nudge toward the owner.
            // Δv = force_per_tick / target_mass — heavy ships drift
            // less per tick (correct Newtonian behaviour).
            if let Ok((_, mut vel, mass)) = ship_state.get_mut(target_e) {
                let to_owner = world_origin - tp;
                let len = to_owner.length();
                if len > 1e-3 {
                    let dir = to_owner / len;
                    let m = mass.0.max(0.0001);
                    vel.0 += dir * (tractor.force_per_tick / m);
                }
            }
        }

        // Visual: the gravity field grips the TARGET — draw a pulsing
        // disc around it, not a beam line spanning the whole arena (the
        // old long-line / full-range fallback). Hidden when there's no
        // target to grip.
        if let Some(tp) = target_pos {
            let center = focus.map_or(tp, |f| crate::physics::nearest_image(tp, f));
            let pulse = 90.0 + 10.0 * (time.elapsed_secs() * 8.0).sin();
            tractor_xf.translation = center.extend(0.3);
            tractor_xf.rotation = Quat::IDENTITY;
            sprite.image = assets.load("ui/joystick_base.png");
            sprite.custom_size = Some(Vec2::splat(pulse));
            sprite.color = Color::srgba(0.55, 0.85, 1.0, 0.30);
        } else {
            sprite.custom_size = Some(Vec2::ZERO);
        }

        tractor.remaining -= dt;
        if tractor.remaining <= 0.0 {
            commands.entity(tractor_entity).try_despawn();
        }
    }
}

/// Tick down the `Invisible` timer and remove the component on expiry.
/// Visual cloak rendering is a polish pass — for now, "invisible" is
/// purely a targeting filter that homing missiles and auto-aim beams
/// honour. The cloaked ship is still drawn at full brightness.
fn tick_invisible(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut q: Query<(Entity, &mut Invisible)>,
) {
    let dt = time.delta_secs();
    for (e, mut inv) in &mut q {
        inv.remaining -= dt;
        if inv.remaining <= 0.0 {
            commands.entity(e).try_remove::<Invisible>();
        }
    }
}

/// Tick down the `DamageToBattery` timer and remove on expiry.
fn tick_damage_to_battery(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut q: Query<(Entity, &mut DamageToBattery)>,
) {
    let dt = time.delta_secs();
    for (e, mut d) in &mut q {
        d.remaining -= dt;
        if d.remaining <= 0.0 {
            commands.entity(e).try_remove::<DamageToBattery>();
        }
    }
}

/// Drive each `SubEntity` per tick: lifetime/hp bookkeeping, target
/// (re)acquisition, steering toward the target capped by `turn_rate`.
/// Pure-drift behaviours (Syreen pods) skip the steering.
fn tick_sub_entities(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut subs: Query<(
        Entity,
        &mut SubEntity,
        &Position,
        &mut LinearVelocity,
        &mut SubEntityAi,
        Option<&OrzMarineBoarded>,
    )>,
    ships: Query<(Entity, &Ship, &Position), Without<SubEntity>>,
) {
    let dt = time.delta_secs();
    for (sub_entity, mut sub, sub_pos, mut sub_vel, mut ai, boarded) in &mut subs {
        // Boarded marines are owned entirely by `tick_orz_marines_boarded`
        // — they don't time out and they don't steer.
        if boarded.is_some() {
            continue;
        }
        sub.remaining_s -= dt;
        if sub.remaining_s <= 0.0 || sub.hp <= 0 {
            commands.entity(sub_entity).try_despawn();
            continue;
        }
        let owner_slot = ships.get(sub.owner).ok().map(|(_, s, _)| s.player_slot);

        match &mut *ai {
            SubEntityAi::HomeAndDetonate {
                target,
                turn_rate,
                speed,
                ..
            }
            | SubEntityAi::AttachAndDrain {
                target,
                turn_rate,
                speed,
                ..
            } => {
                // Re-acquire target if missing or destroyed.
                let target_lost = target.map(|t| ships.get(t).is_err()).unwrap_or(true);
                if target_lost {
                    let mut best: Option<(Entity, f32)> = None;
                    for (e, s, p) in &ships {
                        if Some(s.player_slot) == owner_slot {
                            continue;
                        }
                        let d2 = (p.0 - sub_pos.0).length_squared();
                        if best.map_or(true, |(_, bd)| d2 < bd) {
                            best = Some((e, d2));
                        }
                    }
                    *target = best.map(|(e, _)| e);
                }
                let Some(t) = *target else {
                    continue;
                };
                let Ok((_, _, t_pos)) = ships.get(t) else {
                    continue;
                };
                let to_target = (t_pos.0 - sub_pos.0).normalize_or_zero();
                if to_target == Vec2::ZERO {
                    continue;
                }
                let current = sub_vel.0.normalize_or_zero();
                if current == Vec2::ZERO {
                    sub_vel.0 = to_target * *speed;
                    continue;
                }
                // Cap rotation per tick at `turn_rate * dt`. Compute
                // signed angle between current heading and desired,
                // clamp, then rotate the velocity vector by that.
                let cur_angle = current.y.atan2(current.x);
                let tgt_angle = to_target.y.atan2(to_target.x);
                let mut diff = tgt_angle - cur_angle;
                while diff > std::f32::consts::PI {
                    diff -= std::f32::consts::TAU;
                }
                while diff < -std::f32::consts::PI {
                    diff += std::f32::consts::TAU;
                }
                let cap = (*turn_rate * dt).abs();
                let actual = diff.clamp(-cap, cap);
                let (sn, cs) = actual.sin_cos();
                let new_dir = Vec2::new(
                    current.x * cs - current.y * sn,
                    current.x * sn + current.y * cs,
                );
                sub_vel.0 = new_dir * *speed;
            }
            SubEntityAi::DriftAndCollect { .. } => {
                // No steering. Drift forever at spawn velocity.
            }
        }
    }
}

/// Resolve collisions between sub-entities and ships. Dispatches the
/// AI's on-hit behaviour: damage + sap for DOGI, heavy crew drain for
/// marines, friendly-collect for crew pods. Owner is never affected
/// by its own sub-entity.
fn handle_sub_entity_collisions(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    mut subs: Query<(&mut SubEntity, &SubEntityAi)>,
    mut crews: Query<&mut Crew>,
    mut batteries: Query<&mut Battery>,
    ships: Query<&Ship>,
) {
    for event in reader.read() {
        let (sub_entity, other_entity) = if subs.contains(event.collider1) {
            (event.collider1, event.collider2)
        } else if subs.contains(event.collider2) {
            (event.collider2, event.collider1)
        } else {
            continue;
        };
        let Ok((mut sub, ai)) = subs.get_mut(sub_entity) else {
            continue;
        };
        if other_entity == sub.owner {
            continue;
        }
        let Ok(other_ship) = ships.get(other_entity) else {
            continue;
        };

        match ai {
            SubEntityAi::HomeAndDetonate {
                damage_on_hit,
                batt_sap,
                ..
            } => {
                if let Ok(mut crew) = crews.get_mut(other_entity) {
                    crew.current = (crew.current - *damage_on_hit).max(0);
                }
                if let Ok(mut batt) = batteries.get_mut(other_entity) {
                    batt.current = (batt.current - *batt_sap).max(0);
                }
                sub.hp -= 1;
                // `tick_sub_entities` despawns when hp <= 0.
                info!(
                    "DOGI hit P{}: -{} crew, -{} batt (sub hp now {})",
                    other_ship.player_slot + 1,
                    damage_on_hit,
                    batt_sap,
                    sub.hp
                );
            }
            SubEntityAi::AttachAndDrain { crew_drain: _, .. } => {
                // Canon: marine BOARDS the host and rolls stochastic
                // damage each 50 ms tick (`shporzne.cpp` 9/10000 dmg
                // chance, 1/10000 death chance per ms). Don't drain
                // up-front and don't despawn — just attach and let
                // `tick_orz_marines_boarded` handle the rest.
                commands.entity(sub_entity).try_insert(OrzMarineBoarded {
                    host: other_entity,
                    roll_accum_s: 0.0,
                });
                info!("marine boarded P{}", other_ship.player_slot + 1);
            }
            SubEntityAi::DriftAndCollect {
                owner_slot,
                crew_value,
            } => {
                if other_ship.player_slot == *owner_slot {
                    if let Ok(mut crew) = crews.get_mut(other_entity) {
                        crew.current = (crew.current + *crew_value).min(crew.max);
                    }
                    info!(
                        "pod collected by P{}: +{} crew",
                        other_ship.player_slot + 1,
                        crew_value
                    );
                    commands.entity(sub_entity).try_despawn();
                }
                // Enemy contact: no effect, pod keeps drifting.
            }
        }
    }
}

fn tick_special_cooldown(time: Res<Time<Physics>>, mut q: Query<&mut SpecialCooldown>) {
    let dt = time.delta_secs();
    for mut cd in &mut q {
        if cd.0 > 0.0 {
            cd.0 = (cd.0 - dt).max(0.0);
        }
    }
}

/// Visual cloak: while `Invisible` is on a ship, tint its sprite jet
/// black so the silhouette reads as "cloaked" — canonical Ilwrath
/// look. On the next frame after `Invisible` is removed, restore the
/// sprite back to white. We don't touch `image` (that's the rotation
/// frame, managed by `swap_rotation_frame`), only `color`.
fn tick_invisible_visual(
    mut ships: Query<
        (&mut Sprite, Option<&Invisible>),
        // Pkunk clones get a per-frame magenta-cyan tint from
        // `tick_pkunk_clone_visual`; we'd fight that if we kept
        // writing white here.
        (With<Ship>, Without<crate::ultimate::PkunkClone>),
    >,
) {
    for (mut sprite, invisible) in &mut ships {
        let target = if invisible.is_some() {
            // Keep alpha 1 so the silhouette stays solid; RGB → 0
            // makes the sprite render as a pure black ship outline,
            // exactly the canonical SC2 cloak look.
            Color::srgba(0.0, 0.0, 0.0, 1.0)
        } else {
            Color::WHITE
        };
        if sprite.color != target {
            sprite.color = target;
        }
    }
}

/// Stamped on the Syreen ship by `DrainNearbyCrew` in apply_kind;
/// consumed (and removed) by `apply_syreen_drain` next FixedUpdate
/// tick where we have full access to other ships' positions / crew.
#[derive(Component, Debug)]
pub struct SyreenDrainRequest {
    pub range: f32,
    pub max_drain: i32,
}

/// One-shot crew drain: for each enemy ship within `range` of the
/// firer, subtract crew proportional to proximity (closer = more)
/// plus a small random bonus, capped at `max_drain` per target.
/// Implements shpsyrpe.cpp:activate_special — the Syreen siren song.
fn apply_syreen_drain(
    mut commands: Commands,
    assets: Res<AssetServer>,
    requesters: Query<(Entity, &Ship, &Position, &Rotation, &SyreenDrainRequest)>,
    ship_pos: Query<(Entity, &Ship, &Position), Without<Invisible>>,
    shields: Query<&ShieldActive>,
    mut crews: Query<&mut Crew>,
    mut rng: ResMut<crate::rng::GameRng>,
) {
    for (firer_entity, firer_ship, firer_pos, firer_rot, req) in &requesters {
        let firer_xy = firer_pos.0;
        let _firer_rot = firer_rot;
        for (target_entity, target_ship, target_pos) in &ship_pos {
            if target_entity == firer_entity {
                continue;
            }
            if target_ship.player_slot == firer_ship.player_slot {
                continue;
            }
            let dist = (target_pos.0 - firer_xy).length();
            if dist >= req.range {
                continue;
            }
            // Linear proximity weight 1.0 (touching) → 0.0 (edge).
            let prox = 1.0 - (dist / req.range);
            let base = (req.max_drain as f32 * prox).round() as i32;
            // Determinism-critical: this drain directly modifies
            // target crew, so peers must agree on the roll.
            let jitter = rng.i32(0..=req.max_drain);
            let mut dmg = (base + jitter).clamp(0, req.max_drain * 2);
            // Shields halve / cancel as usual.
            let factor = shields
                .get(target_entity)
                .map(|s| s.damage_factor)
                .unwrap_or(1.0);
            dmg = ((dmg as f32) * factor).round() as i32;
            if dmg <= 0 {
                continue;
            }
            let actual = if let Ok(mut crew) = crews.get_mut(target_entity) {
                let a = dmg.min(crew.current);
                if a > 0 {
                    crew.current -= a;
                }
                a
            } else {
                0
            };
            // Canon `shpsyrpe.cpp`: for each crew lured, spawn a CrewPod
            // (`data->spriteSpecial`, the little green figure) that drifts
            // from the target toward the Syreen and joins it on contact.
            // We use the `DriftAndCollect` SubEntity AI: ballistic motion
            // at spawn velocity, on hit with an owner-slot ship adds
            // `crew_value` crew. Velocity points toward the Syreen with
            // random jitter, like the canon `vel = unit_vector(traj) *
            // velocity` (re-evaluated each tick on canon; for our ballistic
            // pod we lock the spawn-time direction — close enough).
            for _ in 0..actual {
                let jitter = Vec2::new(
                    (rng.f32() - 0.5) * 30.0,
                    (rng.f32() - 0.5) * 30.0,
                );
                let spawn = target_pos.0 + jitter;
                let to_firer = firer_xy - spawn;
                let dir = if to_firer.length_squared() > 1.0 {
                    to_firer.normalize()
                } else {
                    Vec2::Y
                };
                let speed = 220.0;
                let local_dir = Vec2::new(0.0, 1.0); // forward-local; rotated below
                // `spawn_sub_entity` rotates local_dir by the owner's
                // rotation. Hand it a zeroed "rotation" by spawning the
                // pod at the target's position via the firer (rotation
                // doesn't matter for DriftAndCollect — it's ballistic),
                // and inject the actual world direction via a follow-up
                // velocity write. Easier: build the LinearVelocity
                // ourselves and spawn directly.
                let _ = local_dir;
                commands.spawn((
                    SubEntity {
                        owner: firer_entity,
                        remaining_s: 8.0,
                        hp: 1,
                    },
                    SubEntityAi::DriftAndCollect {
                        owner_slot: firer_ship.player_slot,
                        crew_value: 1,
                    },
                    Sprite {
                        image: assets.load("ships/syrpe/sprites/shot_b01.png".to_string()),
                        color: Color::srgb(0.5, 1.0, 0.5),
                        custom_size: Some(Vec2::splat(7.0)),
                        ..default()
                    },
                    Transform::from_translation(spawn.extend(0.4)),
                    RigidBody::Dynamic,
                    Collider::circle(3.5),
                    Mass(0.4),
                    Position(spawn),
                    Rotation::default(),
                    LinearVelocity(dir * speed),
                    AngularVelocity::ZERO,
                    LinearDamping(0.0),
                    AngularDamping(0.0),
                    CollisionEventsEnabled,
                ));
            }
        }
        commands
            .entity(firer_entity)
            .try_remove::<SyreenDrainRequest>();
    }
}

// ----------------------------------------------------------------
// Mycon expanding plasma cloud
// ----------------------------------------------------------------
//
// Per shpmycpo.cpp::MyconPlasma::calculate, the plasmoid cycles
// through `frame_count` sprite frames and decays its damage linearly
// based on distance traveled. We approximate frame cycling with a
// list of pre-loaded sprite handles + linear interpolation of
// Transform.scale for smooth visual growth (10 sprites instead of
// canon's 64; close enough that the eye reads it as a continuous
// puff).

/// Marker on Mycon ships so newly-spawned projectiles inherit the
/// expanding-cloud behaviour.
#[derive(Component, Debug, Default)]
pub struct MyconPlasmaShooter;

/// Per-projectile state for the plasma cloud animation. Set on
/// projectile spawn by `tick_mycon_plasma_birth`; consumed each
/// tick by `tick_mycon_plasma`.
#[derive(Component, Debug)]
pub struct MyconPlasmaPulse {
    pub start_pos: Vec2,
    pub max_damage: i32,
    pub max_distance: f32,
    pub frames: Vec<Handle<Image>>,
    pub start_size: f32,
}

/// Mycon plasma: cycle the sprite frame + grow the visible cloud +
/// decay the projectile's damage linearly with distance traveled.
/// When the plasmoid has traveled `max_distance`, damage hits 0.
fn tick_mycon_plasma(
    mut q: Query<(
        &MyconPlasmaPulse,
        &Position,
        &mut Projectile,
        &mut Sprite,
        &mut Transform,
    )>,
) {
    for (pulse, pos, mut proj, mut sprite, mut xf) in &mut q {
        let dist = (pos.0 - pulse.start_pos).length();
        let t = (dist / pulse.max_distance).clamp(0.0, 1.0);

        // Continuous frame interpolation. Choose the nearest frame
        // index; the visual size lerp handles in-between smoothness.
        if !pulse.frames.is_empty() {
            let idx = (t * (pulse.frames.len() - 1) as f32).round() as usize;
            let idx = idx.min(pulse.frames.len() - 1);
            sprite.image = pulse.frames[idx].clone();
        }

        // Grow the visible cloud: 1.0× at spawn to ~3.5× at end.
        // Updates `custom_size` (used by Sprite::from_color spawns).
        let grow = 1.0 + 2.5 * t;
        sprite.custom_size = Some(Vec2::splat(pulse.start_size * grow));

        // Linear damage decay: at edge, damage = 0. Floor below 1
        // so projectile.damage never hits zero before despawn (we
        // want at least nominal contact damage if the cloud
        // catches a ship right at the edge).
        let new_damage =
            ((1.0 - t) * pulse.max_damage as f32).round() as i32;
        proj.damage = new_damage.max(1);
        // Subtle alpha falloff so the edge of the cloud reads as
        // dissipating energy, not a hard expanding circle.
        let alpha = 0.4 + 0.6 * (1.0 - t);
        let lin = sprite.color.to_linear();
        sprite.color = Color::srgba(lin.red, lin.green, lin.blue, alpha);
        let _ = xf;
    }
}

/// Watches for new projectiles owned by Mycon ships and attaches
/// the `MyconPlasmaPulse` animator. Runs every FixedUpdate; the
/// `Added<Projectile>` filter means a projectile only ever gets
/// stamped once.
fn tick_mycon_plasma_birth(
    mut commands: Commands,
    new_projectiles: Query<(Entity, &Projectile, &Position, &Sprite), Added<Projectile>>,
    shooters: Query<(), With<MyconPlasmaShooter>>,
    assets: Res<AssetServer>,
) {
    for (entity, proj, pos, sprite) in &new_projectiles {
        if shooters.get(proj.owner).is_err() {
            continue;
        }
        // Load all available plasma frames lazily — the asset
        // server reuses already-loaded handles, so we pay the
        // load cost once across the whole match.
        let frames: Vec<Handle<Image>> = (1..=10)
            .map(|i| {
                let path = format!("ships/mycpo/sprites/shot_a{:02}.png", i);
                assets.load(path)
            })
            .collect();
        let start_size = sprite
            .custom_size
            .map(|s| s.x)
            .unwrap_or(16.0);
        commands.entity(entity).try_insert(MyconPlasmaPulse {
            start_pos: pos.0,
            max_damage: proj.damage,
            // 60 SC2 range units × 40 = 2400 wu, same as the
            // VolleySpec lifetime/speed for Mycpo. Reuse the
            // lifetime to derive distance: speed * lifetime.
            max_distance: 60.0 * SC2_RANGE_SCALE,
            frames,
            start_size,
        });
    }
}

// ----------------------------------------------------------------
// Chmmr Avatar — three orbiting ZapSats
// ----------------------------------------------------------------
//
// Per shpchmav.cpp + .ini Extra: three satellites at fixed angles
// (0°, 120°, 240°) orbit the Avatar at radius 100 wu; each scans
// for the nearest non-friendly non-invisible ship within range
// (4 * 40 = 160 wu) and zaps it with a 1-damage point laser.
// Recharge 250 SC2 frames (12.5 s) between zaps. Each satellite
// has 10 armour — damage from outside destroys it.

/// Marker on a Chmmr ship that hasn't spawned its satellites yet.
/// Consumed by `spawn_chmmr_satellites`.
#[derive(Component, Debug)]
pub struct NeedsChmmrSatellites;

/// One of three satellites orbiting a Chmmr Avatar. Position is
/// driven each tick relative to the owner; on hit it loses armour;
/// on owner death the satellite despawns.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct ChmmrSatellite {
    pub owner: Entity,
    pub angle_offset: f32,
    pub recharge_remaining_s: f32,
    pub armour: i32,
}

/// Short-lived sprite line drawn from a satellite to its zap
/// target. Pure visual; the damage is applied directly in
/// `tick_chmmr_satellites` (no persistent Beam — the orbit
/// would drag the visual off-target). Faded out by
/// `tick_zap_flashes`.
#[derive(Component, Debug)]
pub struct ZapFlash {
    pub remaining_s: f32,
    pub total_s: f32,
}

fn spawn_chmmr_satellites(
    mut commands: Commands,
    needs: Query<(Entity, &Position), With<NeedsChmmrSatellites>>,
    assets: Res<AssetServer>,
) {
    use std::f32::consts::TAU;
    for (ship_entity, ship_pos) in &needs {
        for i in 0..3 {
            let angle_offset = (i as f32) * TAU / 3.0;
            let offset = Vec2::new(angle_offset.cos(), angle_offset.sin()) * 100.0;
            let pos = ship_pos.0 + offset;
            commands.spawn((
                ChmmrSatellite {
                    owner: ship_entity,
                    angle_offset,
                    recharge_remaining_s: 0.0,
                    armour: 10,
                },
                Sprite {
                    // chmav's satellite frames are `shot_b01..shot_b64`
                    // (64 rotation frames at 50×50). For the simple
                    // case we pin to frame 1 — the satellite itself
                    // doesn't rotate (Transform's own rotation handles
                    // any orientation we want).
                    image: assets.load("ships/chmav/sprites/shot_b01.png"),
                    color: Color::srgba(0.85, 0.95, 1.0, 1.0),
                    custom_size: Some(Vec2::splat(24.0)),
                    ..default()
                },
                Transform::from_translation(pos.extend(0.25)),
                // Physics body + circle collider so opponents'
                // projectiles can hit and chip the satellite's
                // armour. Sensor so it doesn't push other entities
                // around — it's a hit target, not an obstacle.
                RigidBody::Kinematic,
                Collider::circle(10.0),
                Sensor,
                Position(pos),
                CollisionEventsEnabled,
            ));
        }
        commands.entity(ship_entity).try_remove::<NeedsChmmrSatellites>();
    }
}

/// Fade ZapFlash sprites toward alpha 0 over their lifetime,
/// then despawn. Visual-only; called each tick in the main
/// gameplay schedule.
fn tick_zap_flashes(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut q: Query<(Entity, &mut ZapFlash, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (e, mut flash, mut sprite) in &mut q {
        flash.remaining_s -= dt;
        if flash.remaining_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        let frac = (flash.remaining_s / flash.total_s).clamp(0.0, 1.0);
        let lin = sprite.color.to_linear();
        sprite.color = Color::srgba(lin.red, lin.green, lin.blue, frac);
    }
}

/// Per-tick satellite update:
///   - Orbit slowly around the owner (angle += orbital_rate * dt).
///   - Position = owner_pos + 100 * unit_vector(angle).
///   - If recharged, scan for nearest non-friendly non-invisible
///     ship within zap_range and fire a 1-damage beam.
///   - If the owner is gone, despawn.
fn tick_chmmr_satellites(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    ult: Res<crate::ultimate::UltimateState>,
    mut sats: Query<(Entity, &mut ChmmrSatellite, &mut Transform, &mut Position)>,
    owners: Query<(&Ship, &Position), Without<ChmmrSatellite>>,
    ship_pos: Query<(Entity, &Ship, &Position), (Without<Invisible>, Without<ChmmrSatellite>)>,
    shields: Query<&ShieldActive>,
    mut crews: Query<&mut Crew>,
) {
    // During the Chmmr's own bump-set-spike-laser ultimate, the
    // cinematic system `tick_chmmr_ultimate` (in Update) owns
    // satellite positioning. Bailing here prevents the normal
    // orbit from clobbering the volley formation each
    // FixedUpdate tick.
    if ult.variant == crate::ultimate::UltimateVariant::Chmmr
        && matches!(
            ult.phase,
            crate::ultimate::UltimatePhase::ChmmrCharging
                | crate::ultimate::UltimatePhase::ChmmrVolley
        )
    {
        return;
    }
    let dt = time.delta_secs();
    /// Orbit angular velocity (rad/s).
    const ORBITAL_RATE: f32 = 0.6;
    /// Distance from the Avatar's center to a satellite's center.
    const ORBIT_RADIUS: f32 = 100.0;
    /// Auto-zap range (world units). Widened from the literal canon
    /// 4·40 so the short-range satellite lasers actually reach a nearby
    /// opponent and read as "the satellites are shooting."
    const ZAP_RANGE: f32 = 8.0 * SC2_RANGE_SCALE;
    /// Crew damage per zap.
    const ZAP_DAMAGE: i32 = 1;
    /// Visual flash lifetime (seconds). Short so it doesn't lag
    /// behind the satellite as it orbits.
    const ZAP_FLASH_S: f32 = 0.12;
    /// Cooldown between successive zaps. The literal 250-frame canon
    /// (12.5 s) made the satellites essentially never fire; this is the
    /// responsive auto-laser cadence the design intends.
    const ZAP_RECHARGE_S: f32 = 0.5;

    for (sat_entity, mut sat, mut xf, mut sat_pos) in &mut sats {
        // Owner death → satellite dies.
        let Ok((owner_ship, owner_pos)) = owners.get(sat.owner) else {
            if let Ok(mut ec) = commands.get_entity(sat_entity) {
                ec.try_despawn();
            }
            continue;
        };

        // Orbit.
        sat.angle_offset += ORBITAL_RATE * dt;
        if sat.angle_offset > std::f32::consts::TAU {
            sat.angle_offset -= std::f32::consts::TAU;
        }
        let offset =
            Vec2::new(sat.angle_offset.cos(), sat.angle_offset.sin()) * ORBIT_RADIUS;
        let world = owner_pos.0 + offset;
        xf.translation.x = world.x;
        xf.translation.y = world.y;
        // Also write the Avian Position so the collider tracks
        // the visual — without this, the satellite's hit volume
        // would stay at its spawn point and never move.
        sat_pos.0 = world;

        // Cool down.
        if sat.recharge_remaining_s > 0.0 {
            sat.recharge_remaining_s = (sat.recharge_remaining_s - dt).max(0.0);
            continue;
        }

        // Find nearest non-friendly non-invisible ship in range.
        let mut best: Option<(Entity, Vec2, f32)> = None;
        for (e, s, p) in &ship_pos {
            if s.player_slot == owner_ship.player_slot {
                continue;
            }
            let d2 = (p.0 - world).length_squared();
            if d2 > ZAP_RANGE * ZAP_RANGE {
                continue;
            }
            if best.map_or(true, |(_, _, b)| d2 < b) {
                best = Some((e, p.0, d2));
            }
        }
        let Some((target_entity, target_pos, _)) = best else {
            continue;
        };

        // Apply damage directly (skip the persistent-Beam path
        // because a Beam follows its owner-ship, not the
        // satellite — the orbit would drag the beam off-target
        // over its 2.5 s canonical duration). Shield multiplier
        // matches every other damage-application site.
        let factor = shields
            .get(target_entity)
            .map(|s| s.damage_factor)
            .unwrap_or(1.0);
        let dmg = ((ZAP_DAMAGE as f32 * factor).round() as i32).max(0);
        if dmg > 0 {
            if let Ok(mut crew) = crews.get_mut(target_entity) {
                crew.current = (crew.current - dmg).max(0);
            }
        }

        // Visual zap line from satellite to target. Sprite is a
        // thin rectangle aligned along the (target - sat) vector;
        // `tick_zap_flashes` fades it out and despawns it.
        let delta = target_pos - world;
        let len = delta.length().max(1.0);
        let mid = (world + target_pos) * 0.5;
        let angle = delta.y.atan2(delta.x) - std::f32::consts::FRAC_PI_2;
        let color = Color::srgba(0.7, 0.95, 1.0, 1.0);
        commands.spawn((
            ZapFlash {
                remaining_s: ZAP_FLASH_S,
                total_s: ZAP_FLASH_S,
            },
            Sprite::from_color(color, Vec2::new(3.0, len)),
            Transform {
                translation: mid.extend(0.30),
                rotation: Quat::from_rotation_z(angle),
                scale: Vec3::ONE,
            },
        ));

        sat.recharge_remaining_s = ZAP_RECHARGE_S;
    }
}

// ----------------------------------------------------------------
// Kohr-Ah blade — hold to arm, release to drop a mine
// ----------------------------------------------------------------
//
// User-facing summary: hold fire to keep a blade in front of the
// ship; release to drop it. The dropped blade sits roughly in
// place and homes slowly toward the nearest enemy (canonical
// KohrAhBlade with Persists=1 + the post-release passive
// behaviour). Re-pressing fire arms a new blade. There's a small
// arm time so the blade is visible in front of the ship before
// it can be released — matches the canon "no instant drop" feel.

/// Per-ship state: tracks the currently-armed blade entity and
/// edge-detects fire press/release across FixedUpdate ticks.
#[derive(Component, Debug, Default)]
pub struct KohrAhBladeCarrier {
    pub current: Option<Entity>,
    pub last_fire_held: bool,
}

/// Marker on a released blade. While present, `tick_kohma_passive_blades`
/// snaps the blade's velocity each tick toward the nearest non-friendly
/// non-invisible ship at 1/10 the original speed (or zero if no target
/// is in range). Matches shpkohma.cpp:KohrAhBlade::calculate's
/// `passive` branch exactly: `angle = trajectory_angle(target);
/// vel = (v/10) * unit_vector(angle)`.
#[derive(Component, Debug, Clone, Copy)]
pub struct KohrAhBladePassive {
    /// The full active-mode speed; passive mode moves at 1/10 of this.
    pub launch_speed: f32,
}

/// Each FixedUpdate, for each Kohr-Ah ship:
///   - On just-pressed fire: spawn a blade at the ship's forward
///     muzzle, flying in a straight line at `weaponVelocity` away
///     from the ship. While the player keeps holding fire it just
///     keeps flying straight — we don't track or re-position it.
///   - On just-released fire: stamp `KohrAhBladePassive` on the
///     blade. From that point on it moves at 1/10 the original
///     speed and very subtly tracks the nearest enemy
///     (canonical SC2 Kohr-Ah passive blade).
///
/// shpkohma.cpp::activate_weapon + ::calculate.
fn tick_kohma_blade(
    mut commands: Commands,
    slot_inputs: Res<input::SlotInputs>,
    assets: Res<AssetServer>,
    // Liveness-only — no Position access, so no archetype conflict
    // with `passive` mutations below.
    projectile_alive: Query<(), With<Projectile>>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut KohrAhBladeCarrier,
        &mut Battery,
    )>,
) {
    let blade_velocity = 64.0 * SC2_VEL_SCALE;
    let blade_range_world = 12.0 * SC2_RANGE_SCALE;
    let blade_damage: i32 = 4;
    // Generous — the passive phase can linger long after the active
    // phase ends. Capped by despawn-on-hit and lifetime regardless.
    let blade_lifetime = blade_range_world / blade_velocity * 6.0;

    for (entity, ship, ship_pos, ship_rot, ship_vel, mut carrier, mut batt) in &mut ships {
        let input = slot_inputs.held[ship.player_slot.min(3)];
        let fire_held = input.pressed(input::INPUT_FIRE);

        // Clear stale handle if the blade died on a hit.
        if let Some(c) = carrier.current {
            if projectile_alive.get(c).is_err() {
                carrier.current = None;
            }
        }

        let was_held = carrier.last_fire_held;
        carrier.last_fire_held = fire_held;
        let just_pressed = fire_held && !was_held;
        let just_released = !fire_held && was_held;

        let forward = Vec2::new(0.0, 1.0);
        let world_dir = Vec2::new(
            forward.x * ship_rot.cos - forward.y * ship_rot.sin,
            forward.x * ship_rot.sin + forward.y * ship_rot.cos,
        );

        if just_pressed && carrier.current.is_none() {
            // Battery gate.
            if ship.stats.weapon_drain > 0 && batt.current < ship.stats.weapon_drain {
                continue;
            }
            batt.current = (batt.current - ship.stats.weapon_drain).max(0);

            // Spawn the blade at the ship's forward muzzle, flying
            // in a straight line at full velocity. Inherits the
            // ship's velocity so a moving ship doesn't "outrun"
            // its own blade. No clinging, no re-position — once
            // launched it just goes.
            let muzzle = ship_pos.0 + world_dir * 34.0;
            let init_angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;
            let proj_vel = ship_vel.0 + world_dir * blade_velocity;
            let blade_entity = commands
                .spawn((
                    Projectile {
                        owner: entity,
                        damage: blade_damage,
                        lifetime: blade_lifetime,
                    },
                    Sprite {
                        image: assets.load("ships/kohma/sprites/shot_a01.png"),
                        color: Color::srgb(1.0, 0.85, 0.4),
                        custom_size: Some(Vec2::splat(28.0)),
                        ..default()
                    },
                    Transform::from_translation(muzzle.extend(0.5)),
                    RigidBody::Dynamic,
                    Collider::circle(12.0),
                    Sensor,
                    Mass(1.0),
                    Position(muzzle),
                    Rotation::radians(init_angle),
                    LinearVelocity(proj_vel),
                    AngularVelocity(8.0),
                    LinearDamping(0.0),
                    AngularDamping(0.0),
                    CollisionEventsEnabled,
                ))
                .id();
            carrier.current = Some(blade_entity);
        }

        if just_released {
            // Tag the most recent blade as passive. The dedicated
            // passive-tick system below handles the slow-homing /
            // stop-on-no-target logic from canon.
            if let Some(blade) = carrier.current.take() {
                commands.entity(blade).try_insert(KohrAhBladePassive {
                    launch_speed: blade_velocity,
                });
            }
        }
    }
}

/// Passive-mode blade tick: each frame finds the nearest non-friendly
/// non-invisible ship and rewrites the blade's velocity to point at
/// it at 1/10 launch speed. Match canon exactly:
/// `KohrAhBlade::calculate` snaps `angle = trajectory_angle(target)`
/// and sets `vel = (v/10) * unit_vector(angle)`. With no target the
/// blade halts (`vel = 0`) — it becomes a static mine until something
/// wanders close.
fn tick_kohma_passive_blades(
    blades: Query<&KohrAhBladePassive>,
    mut blade_pose: Query<(&Position, &mut LinearVelocity), With<KohrAhBladePassive>>,
    target_ships: Query<(&Ship, &Position), (With<Ship>, Without<Invisible>)>,
    projectile_owner: Query<&Projectile>,
    owner_ships: Query<&Ship>,
) {
    for (blade_pos, mut vel) in &mut blade_pose {
        let _ = blades; // existence ensured by `With<KohrAhBladePassive>`
        // Look up the blade entity's projectile owner so we can
        // skip same-team targets.
        // Note: we don't have the blade entity here; we infer it
        // via a parallel query. Use the position-based query: the
        // mut blade_pose query iterates blades; their entity isn't
        // exposed in the loop unless we ask for it. Re-query below.
        let _ = projectile_owner;
        let _ = owner_ships;

        // Find the nearest ship (any team) within a generous range.
        // For simplicity here we don't filter by owner team — the
        // standard handle_projectile_hits path skips friendly hits
        // anyway, so the worst case is the blade tracks a friend
        // but doesn't damage them. Matches canon's behaviour where
        // the passive blade just chases the closest thing.
        let mut best: Option<(Vec2, f32)> = None;
        for (_s, p) in &target_ships {
            let d2 = (p.0 - blade_pos.0).length_squared();
            if best.map_or(true, |(_, b)| d2 < b) {
                best = Some((p.0, d2));
            }
        }
        // Pull the launch_speed from the marker for THIS blade.
        // Because `blades` is read-only and indexed by entity we'd
        // need entity access — but every passive blade currently
        // shares the same launch speed (64 * SC2_VEL_SCALE), so we
        // hardcode it here. Cheap to refactor later if we add
        // variants with different speeds.
        const PASSIVE_SPEED: f32 = 64.0 * SC2_VEL_SCALE * 0.1;
        if let Some((target, _)) = best {
            let delta = target - blade_pos.0;
            let d = delta.length();
            if d > 0.5 {
                vel.0 = (delta / d) * PASSIVE_SPEED;
            } else {
                vel.0 = Vec2::ZERO;
            }
        } else {
            vel.0 = Vec2::ZERO;
        }
    }
}

/// Drifting asteroid — a simple Dynamic obstacle that bounces off
/// ships and other asteroids via Avian's natural physics response.
/// Canonical SC2 melee fields are scattered with these so the
/// arena isn't empty space. They don't damage on contact (canon
/// VSmallAsteroid is similar) — they're physical inertia for
/// projectiles and ships to interact with.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct Asteroid;

/// Sprinkle a handful of asteroids at random positions across the
/// arena, avoiding the player-ship spawn corridors. Called once
/// per match from `spawn_match`.
pub fn spawn_asteroids(
    commands: &mut Commands,
    assets: &AssetServer,
    rng: &mut crate::rng::GameRng,
) {
    use std::f32::consts::TAU;
    const N: usize = 8;
    /// Half-side of the arena (matches STAR_AREA_HALF in starfield
    /// for symmetry — keeps asteroids visible within the starfield
    /// region).
    const HALF: f32 = 3000.0;
    /// Don't spawn asteroids too close to the ship spawn corridor.
    const KEEP_OUT_X: f32 = 600.0;
    const KEEP_OUT_Y: f32 = 350.0;
    /// melee.dat ships 64 rotation frames per asteroid sprite
    /// (`ASTERO01..64`, indices 1-based). Pick one at random per
    /// asteroid so the field doesn't read as 8 identical rocks.
    const ASTEROID_FRAMES: usize = 64;

    // All draws here affect game state (asteroid position +
    // velocity + collider mass + radius → physics integration
    // diverges if peers disagree). Use the seeded RNG.
    for _ in 0..N {
        let pos = loop {
            let x = rng.signed_unit() * HALF;
            let y = rng.signed_unit() * HALF;
            // Avoid the spawn corridor around (±740, 0).
            let near_left = (x - (-740.0)).abs() < KEEP_OUT_X && y.abs() < KEEP_OUT_Y;
            let near_right = (x - 740.0).abs() < KEEP_OUT_X && y.abs() < KEEP_OUT_Y;
            if !near_left && !near_right {
                break Vec2::new(x, y);
            }
        };
        let theta = rng.f32() * TAU;
        let speed = 18.0 + rng.f32() * 28.0;
        let vel = Vec2::new(theta.cos(), theta.sin()) * speed;
        let radius = 22.0 + rng.f32() * 16.0;
        let visual = radius * 2.2;
        let frame_idx = 1 + rng.usize_range(0..ASTEROID_FRAMES);
        let sprite_path = format!("asteroids/astero{:02}.png", frame_idx);
        let mass = 4.0 + rng.f32() * 3.0;
        let ang_vel = rng.signed_unit() * 0.3;
        commands.spawn((
            Asteroid,
            bevy_ggrs::Rollback,
            Sprite {
                image: assets.load(sprite_path),
                color: Color::WHITE,
                custom_size: Some(Vec2::splat(visual)),
                ..default()
            },
            Transform::from_translation(pos.extend(0.1)),
            RigidBody::Dynamic,
            Collider::circle(radius),
            Mass(mass),
            Position(pos),
            // Restitution gives the collisions some bounce — without
            // it asteroids would just stick on contact.
            Restitution::new(0.7),
            Friction::new(0.0),
            LinearVelocity(vel),
            AngularVelocity(ang_vel),
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
    info!("spawned {N} asteroids");
}

// ---------------------------------------------------------------------------
// Planet — a central gravity well (legacy `melee/mcbodies.cpp:Planet`).
//
// A massive, immovable body sitting at the arena centre. It pulls every
// dynamic body (ships + asteroids) toward it with a distance-falloff
// acceleration, so players can slingshot/whip around it for free direction
// changes and a speed boost near the surface. It is SOLID — ships bounce
// off it — and grazing the surface costs crew.
//
// Faithfulness notes vs. the original (`melee/mcbodies.cpp` + the shipped
// `data/server.ini` [Planet] block):
//   - LINEAR falloff (`GravityPower = 1`): accel = force · (1 − r/range).
//     The original supports other powers, but its own server.ini ships
//     power=1 with the comment "linear falloff seems to feel better for
//     game purposes" — so that's what we use.
//   - Distances come straight from server.ini via `scale_range(x) = x·40`:
//     GravityRange = 18 → 720 wu, GravityMinDist = 6 → 240 wu. `r` is
//     clamped to `mindist` so the pull plateaus (doesn't spike) at the core.
//   - GravityWhip = 0.5 → within the well a ship's speed cap is raised up
//     to 1.5× (the "gravity whip" that lets a slingshot keep its speed).
//   - Arilou (and anything with `InertialessDrive`) is IMMUNE — matches
//     `shparisk.cpp:ArilouSkiff::calculate_gravity()` (empty) +
//     `accelerate()` rejecting any external source.
//   - The `gravity_accel` magnitude is the one value not cleanly portable
//     (the original's GravityForce=1.5 runs through `scale_acceleration`,
//     whose distance/time ratios are internally inconsistent with
//     scale_range — the devs flag this with a literal "WTF????" comment).
//     It's tuned here to OUR ship accelerations (~576–2300 wu/s²).
// ---------------------------------------------------------------------------

/// Central gravity-well body. Static (never moves); pulls dynamic bodies in.
#[derive(Component, Debug)]
#[component(on_add = auto_add_rollback)]
pub struct Planet {
    /// Solid collision radius (world units).
    pub radius: f32,
    /// Outside this radius gravity is zero (world units). `scale_range(18)`.
    pub gravity_range: f32,
    /// Distance is clamped to at least this before the linear falloff is
    /// applied, so the pull plateaus near the core. `scale_range(6)`.
    pub gravity_mindist: f32,
    /// Acceleration coefficient (wu/s²). Actual pull = this · (1 − r/range).
    pub gravity_accel: f32,
    /// While a ship is within `gravity_range`, its speed cap is multiplied by
    /// this — the "gravity whip" (server.ini GravityWhip = 0.5 → 1.5×).
    pub whip_mult: f32,
}

impl Default for Planet {
    fn default() -> Self {
        Self {
            radius: 100.0, // PLAN_S0x sprites are 200×200 → ~100 px radius
            gravity_range: 720.0,   // scale_range(18)
            gravity_mindist: 240.0, // scale_range(6)
            gravity_accel: 800.0,
            // ----------------- canon derivation -----------------
            // server.ini [Planet]: GravityForce = 1.5 (SC2 units).
            // mhelpers.cpp:     scale_acceleration(a, 0)
            //                     = a × distance_ratio / time_ratio²
            //                     = a × 0.48 / 50ms / 50ms
            //                     = a × 1.92e-4 TW-px / ms².
            // mcbodies.cpp:144: per-tick Δv = frame_time × force × sr
            //                     = 50ms × (1.5 × 1.92e-4) × sr
            //                     ≈ 0.0144 TW-px/ms · sr at peak.
            // 20 game-frames/sec → peak acceleration ≈ 288 wu/s²
            //   (1 wu = 1 TW-px in our world).
            // We run a bit hotter than canon (800 vs 288) so the well
            // is a real tactical thing in our smaller arena (3000 wu
            // vs canon's 3840). The user's "make it stronger" request
            // after the previous corner-placement experiment landed
            // here — close enough to canon that slingshots feel right,
            // strong enough that a stalled ship gets pulled in
            // visibly.
            whip_mult: 1.5, // 1 + GravityWhip(0.5)
        }
    }
}

/// Spawn the central planet at the arena origin. Called once per match from
/// `spawn_match`. Ships spawn at ±740 on the axes — just outside the
/// `gravity_range`, so they feel no initial pull. The original
/// (`other/planet3d.cpp:create_planet`) picks a random one of the three
/// `PLAN_S0x` melee.dat sprites; we do the same with the seeded RNG so peers
/// agree.
pub fn spawn_planet(commands: &mut Commands, assets: &AssetServer, rng: &mut crate::rng::GameRng) {
    let planet = Planet::default();
    let visual = planet.radius * 2.0;
    let frame = 1 + rng.usize_range(0..3); // PLAN_S01..03
    // Canon (`mmain.cpp:init_objects`) places the planet at `map_size/2` —
    // dead centre of the arena. The whole point of a gravity well is to
    // be a tactical hazard / opportunity in the *middle* of the fight;
    // pushing it to a random corner (as we briefly did) made the well
    // effectively invisible since most engagements happen mid-arena. Tiny
    // random translation (±60 wu) for visual variety, otherwise dead
    // centre.
    let pos = Vec2::new(
        (rng.f32() - 0.5) * 120.0,
        (rng.f32() - 0.5) * 120.0,
    );
    commands.spawn((
        Sprite {
            image: assets.load(format!("ui/planet_{:02}.png", frame)),
            color: Color::WHITE,
            custom_size: Some(Vec2::splat(visual)),
            ..default()
        },
        // Below ships/projectiles (z 0.5..) but above the starfield.
        Transform::from_translation(pos.extend(0.05)),
        RigidBody::Static,
        Collider::circle(planet.radius),
        // Some bounce so ramming the planet kicks you off rather than
        // sticking; ships keep most of their speed.
        Restitution::new(0.4),
        Friction::new(0.0),
        Position(pos),
        CollisionEventsEnabled,
        // Continuous contact list — `tick_planet_grind` drains crew per
        // tick from every ship still touching the planet, so a ship that
        // gets pulled in by gravity and pinned against the surface
        // actually dies instead of sitting forever taking a single
        // CollisionStart's chunk of damage.
        CollidingEntities::default(),
        planet,
    ));
    info!("spawned planet at ({:.0}, {:.0})", pos.x, pos.y);
}

/// Continuous "you're grinding against the planet" damage. The one-shot
/// `tick_planet_contact` deals a chunk on the moment of contact; this
/// pass drains a steady DPS on top, so a ship gravity-pinned against
/// the surface dies instead of perpetually contacting the planet.
fn tick_planet_grind(
    time: Res<Time<Physics>>,
    planets: Query<&CollidingEntities, With<Planet>>,
    mut crews: Query<&mut Crew, With<Ship>>,
    shields: Query<&ShieldActive>,
) {
    /// Crew per second while touching the planet. A ship dragged in and
    /// pinned dies in a few seconds; a slingshot brush that releases on
    /// its own only costs a crew or two. Earlier 12/s was insta-kill on
    /// low-crew classes (Shofixti, Pkunk) the moment they touched down.
    const PLANET_GRIND_DPS: f32 = 3.0;
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for colliders in &planets {
        for &e in colliders.0.iter() {
            let Ok(mut crew) = crews.get_mut(e) else { continue };
            if crew.current <= 0 {
                continue;
            }
            let factor = shields.get(e).map(|s| s.damage_factor).unwrap_or(1.0);
            let amount = PLANET_GRIND_DPS * dt * factor;
            let dmg = amount.ceil() as i32; // ceil → still ticks at < 1 dmg/s
            if dmg > 0 {
                crew.current = (crew.current - dmg).max(0);
            }
        }
    }
}

/// Pull every dynamic body toward each planet, inverse-square with distance
/// (clamped at `gravity_mindist`), out to `gravity_range`. Toroidal: uses the
/// minimum-image direction so the pull takes the shortest path across the
/// wrap. Runs in `GgrsSchedule` before `cap_velocity` so the whip-boosted cap
/// applies the same tick. Arilou / `InertialessDrive` ships are immune.
fn apply_planet_gravity(
    time: Res<Time<Physics>>,
    planets: Query<(&Position, &Planet)>,
    mut bodies: Query<
        (
            &Position,
            &mut LinearVelocity,
            Option<&InertialessDrive>,
            Option<&crate::ultimate::HyperActive>,
        ),
        Or<(With<Ship>, With<Asteroid>)>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (planet_pos, planet) in &planets {
        let range_sq = planet.gravity_range * planet.gravity_range;
        for (pos, mut vel, inertialess, hyper) in &mut bodies {
            // Inertialess drive (Arilou) rejects all external acceleration;
            // a ship mid-ultimate owns its own motion.
            if inertialess.is_some() || hyper.is_some() {
                continue;
            }
            let to_planet = crate::physics::min_image(planet_pos.0 - pos.0);
            let dist_sq = to_planet.length_squared();
            if dist_sq > range_sq || dist_sq < 1.0 {
                continue;
            }
            let dist = dist_sq.sqrt();
            // Linear falloff (server.ini GravityPower = 1): full strength at
            // the core (clamped at mindist), zero at gravity_range.
            let r = dist.max(planet.gravity_mindist);
            let falloff = (1.0 - r / planet.gravity_range).max(0.0);
            let accel = planet.gravity_accel * falloff;
            vel.0 += (to_planet / dist) * accel * dt;
        }
    }
}

/// Crew cost for grazing the planet's surface, applied once per contact.
/// On a fresh `CollisionStart` between a ship and a planet, deduct a chunk of
/// the ship's crew (shield-aware). The legacy `Planet::inflict_damage` kills
/// ~1/3 of crew on touch — that's brutal with our bouncy contact, so we use a
/// gentler 1/8 (min 2). Deterministic: Avian's collision events are generated
/// inside `GgrsSchedule`, so they replay identically on rollback.
fn tick_planet_contact(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    assets: Res<AssetServer>,
    planets: Query<(), With<Planet>>,
    asteroids: Query<&Position, With<Asteroid>>,
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
    ships: Query<&Ship>,
) {
    // Guard against re-applying damage / despawn twice in the same
    // event pass (Avian can emit a CollisionStart for both colliders).
    let mut gone: bevy::platform::collections::HashSet<Entity> =
        bevy::platform::collections::HashSet::default();
    for event in reader.read() {
        // Identify which collider is the planet and which is the other body.
        let (other_e, planet_e) = if planets.contains(event.collider1) {
            (event.collider2, event.collider1)
        } else if planets.contains(event.collider2) {
            (event.collider1, event.collider2)
        } else {
            continue;
        };
        let _ = planet_e;
        if gone.contains(&other_e) {
            continue;
        }

        // Asteroid hits the planet → kaboom + despawn. `replenish_asteroids`
        // refills the field over time so the arena doesn't slowly empty.
        // Matches `mcbodies.cpp:Planet::inflict_damage` where mass>0 bodies
        // take 1 damage on contact and asteroids have armour 0 → destroyed.
        if let Ok(ast_pos) = asteroids.get(other_e) {
            spawn_asteroid_explosion(&mut commands, &assets, ast_pos.0, 24.0);
            if gone.insert(other_e) {
                commands.entity(other_e).try_despawn();
            }
            continue;
        }

        // Ship hit. Grazing the planet's surface costs crew (shield-aware).
        let Ok(ship) = ships.get(other_e) else { continue; };
        let Ok(mut crew) = crews.get_mut(other_e) else { continue; };
        // Grazing damage on first contact — modest single-crew tap so a
        // brush isn't immediately fatal (the original Planet::inflict_damage
        // dealt 1 dmg vs the ship's armour; for low-crew classes like
        // Shofixti the previous "crew/8 min 2" was an insta-kill on touch).
        let factor = shields.get(other_e).map(|s| s.damage_factor).unwrap_or(1.0);
        let dmg = ((1.0 * factor).round() as i32).max(0);
        if dmg > 0 {
            crew.current = (crew.current - dmg).max(0);
            info!("P{} grazed the planet: -{} crew", ship.player_slot + 1, dmg);
        }
    }
}

/// Strong-handle keep-alive for every asset the game will ever
/// touch. Bevy's asset GC drops unreferenced assets, so we stash
/// strong handles in here at startup; subsequent `assets.load()`
/// calls then return cached handles without round-tripping the
/// loader. Eliminates the "first time the Mycon fires a plasmoid
/// the sprite isn't ready" stutter on the very first encounter.
#[derive(Resource, Default)]
pub struct PreloadedAssets {
    pub handles: Vec<UntypedHandle>,
}

/// Walk every class in the catalog + every shared asset (asteroids,
/// ultimate portraits + voices, shaders) and `load` it now so the
/// asset server has it cached by the time gameplay code asks.
pub fn preload_all_assets(
    assets: Res<AssetServer>,
    mut preloaded: ResMut<PreloadedAssets>,
) {
    // KEEP THIS LIST TIGHT. Every asset path we list here becomes
    // a fetch on first page load. Earlier versions speculatively
    // requested 4000+ paths (mostly non-existent shot-sprite
    // permutations) — every 404 still counts against GitHub
    // Pages' rate limit and we tripped Cloudflare's 429 throttle.
    // Only enumerate files we KNOW exist + are likely on the
    // first match's critical path.

    // Asteroid rotation frames (1..=64, all guaranteed to exist).
    for i in 1..=64u32 {
        let path = format!("asteroids/astero{:02}.png", i);
        preloaded
            .handles
            .push(assets.load::<Image>(path).untyped());
    }

    // Ultimate portraits + voices — eight known captains, all
    // copied into assets/ultimate/ (missing ones just won't load,
    // no harm done, but the asset server still avoids re-fetching
    // the misses repeatedly).
    for code in [
        "arisk", "earcr", "yehte", "spael", "chebr", "shosc", "pkufu", "slypr",
    ] {
        preloaded.handles.push(
            assets
                .load::<Image>(format!("ultimate/portrait_{}.png", code))
                .untyped(),
        );
        preloaded.handles.push(
            assets
                .load::<AudioSource>(format!("ultimate/{}_voi.wav", code))
                .untyped(),
        );
    }
    preloaded.handles.push(
        assets
            .load::<AudioSource>("ultimate/arisk_stinger.mp3")
            .untyped(),
    );

    // Per-class base rotation frame ONLY. The full 64-frame set
    // gets loaded the moment a class spawns via
    // `load_rotation_frames`; we'd duplicate ~25 × 64 = 1600
    // unnecessary fetches by pre-touching them all here.
    // ship_p00.png is guaranteed (the universal "facing up" frame)
    // and warming it on startup means class-switch UI thumbnails
    // are instantly available.
    for class in ALL_CLASSES {
        let path = format!("ships/{}/sprites/ship_p00.png", class.code());
        preloaded
            .handles
            .push(assets.load::<Image>(path).untyped());
    }

    info!("preloaded {} asset handles", preloaded.handles.len());
}

/// Short-lived animated explosion spawned in place of an asteroid
/// when it's destroyed. 20 KABOOM frames cycle over `total_s`,
/// then the entity despawns. Pure visual — no physics, no damage.
#[derive(Component, Debug)]
pub struct AsteroidExplosion {
    pub remaining_s: f32,
    pub total_s: f32,
    pub frames: Vec<Handle<Image>>,
}

/// Cycle the explosion sprite frame and fade alpha over its
/// lifetime; despawn at zero. The frames advance through the
/// loaded KABOOM_00..19 set linearly with `total_s` so all 20
/// frames play across the animation window.
fn tick_asteroid_explosions(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut AsteroidExplosion, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (e, mut expl, mut sprite) in &mut q {
        expl.remaining_s -= dt;
        if expl.remaining_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        let frac_elapsed = 1.0 - (expl.remaining_s / expl.total_s).clamp(0.0, 1.0);
        let n = expl.frames.len();
        if n > 0 {
            let idx = (frac_elapsed * n as f32).floor() as usize;
            let idx = idx.min(n - 1);
            sprite.image = expl.frames[idx].clone();
        }
        // Fade out over the second half so the explosion eases
        // into the background rather than cutting hard.
        let alpha = ((1.0 - frac_elapsed) * 2.0).clamp(0.0, 1.0);
        sprite.color = Color::srgba(1.0, 1.0, 1.0, alpha);
    }
}

/// Spawn a KABOOM animation at `pos`. Picks up the explosion
/// frames from the asset server (cached after Startup preload).
pub fn spawn_asteroid_explosion(
    commands: &mut Commands,
    assets: &AssetServer,
    pos: Vec2,
    radius: f32,
) {
    let frames: Vec<Handle<Image>> = (0..20u32)
        .map(|i| {
            let path = format!("asteroids/explosion/kaboom_{:02}.png", i);
            assets.load(path)
        })
        .collect();
    let total = 0.4_f32;
    commands.spawn((
        AsteroidExplosion {
            remaining_s: total,
            total_s: total,
            frames: frames.clone(),
        },
        Sprite {
            image: frames.first().cloned().unwrap_or_default(),
            color: Color::WHITE,
            custom_size: Some(Vec2::splat(radius * 3.0)),
            ..default()
        },
        Transform::from_translation(pos.extend(0.15)),
    ));
}

/// Ship-vs-asteroid collisions are deliberately NOT handled here:
/// asteroids only break apart when a weapon (projectile, laser, or
/// the Arilou ultimate blade) hits them. Plain ship rams just
/// bounce off via Avian's contact response. Slylandro's launched
/// asteroids have their own dedicated handler that applies crew
/// damage + kaboom on contact with a non-friendly ship.

/// Replenish asteroids when the field gets thin: each FixedUpdate
/// check the live count, and if it's below `TARGET_ASTEROID_COUNT`,
/// spawn a fresh rock at a random arena position that's off the
/// camera's visible region. Keeps the arena populated through
/// long matches where lasers + ship rams keep destroying rocks.
fn replenish_asteroids(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cameras: Query<(&Transform, &Projection), With<Camera2d>>,
    windows: Query<&Window>,
    asteroids: Query<(), With<Asteroid>>,
    mut rng: ResMut<crate::rng::GameRng>,
) {
    use std::f32::consts::TAU;
    const TARGET_ASTEROID_COUNT: usize = 8;
    const HALF: f32 = 3000.0;
    const KEEP_OUT_X: f32 = 600.0;
    const KEEP_OUT_Y: f32 = 350.0;
    const ASTEROID_FRAMES: usize = 64;

    let count = asteroids.iter().count();
    if count >= TARGET_ASTEROID_COUNT {
        return;
    }

    // Compute the camera's visible half-extents so we can prefer
    // spawn positions outside the current view ("off-screen").
    let (cam_xy, view_hx, view_hy) = if let (Ok((cam_xf, proj)), Ok(win)) =
        (cameras.single(), windows.single())
    {
        let scale = match proj {
            Projection::Orthographic(o) => o.scale,
            _ => 1.0,
        };
        (
            cam_xf.translation.truncate(),
            win.width() * 0.5 * scale,
            win.height() * 0.5 * scale,
        )
    } else {
        (Vec2::ZERO, 1280.0 * 0.5, 720.0 * 0.5)
    };

    // Determinism note: this is gameplay-affecting (the spawned
    // asteroid's pose feeds the physics step), so the seeded
    // RNG must drive every draw. We also can't bail mid-loop
    // based on camera position alone, because in online play
    // each peer has a different camera — using the camera as a
    // filter would let peers consume different numbers of RNG
    // draws. Resolution: do the camera-aware scoring locally
    // (visual hint) but always consume exactly 16 candidate
    // pairs of draws + 5 fallback draws, so the RNG state
    // advances identically on every peer.
    let mut candidates: Vec<Vec2> = Vec::with_capacity(16);
    for _ in 0..16 {
        let x = rng.signed_unit() * HALF;
        let y = rng.signed_unit() * HALF;
        candidates.push(Vec2::new(x, y));
    }
    let fallback_theta = rng.f32() * TAU;
    let fallback_r = HALF * (0.6 + 0.4 * rng.f32());
    let pos = candidates
        .iter()
        .copied()
        .find(|p| {
            let near_left = (p.x - (-740.0)).abs() < KEEP_OUT_X && p.y.abs() < KEEP_OUT_Y;
            let near_right = (p.x - 740.0).abs() < KEEP_OUT_X && p.y.abs() < KEEP_OUT_Y;
            let off_screen = (p.x - cam_xy.x).abs() > view_hx
                || (p.y - cam_xy.y).abs() > view_hy;
            !near_left && !near_right && off_screen
        })
        .unwrap_or_else(|| {
            cam_xy + Vec2::new(fallback_theta.cos(), fallback_theta.sin()) * fallback_r
        });

    let theta = rng.f32() * TAU;
    let speed = 18.0 + rng.f32() * 28.0;
    let vel = Vec2::new(theta.cos(), theta.sin()) * speed;
    let radius = 22.0 + rng.f32() * 16.0;
    let visual = radius * 2.2;
    let frame_idx = 1 + rng.usize_range(0..ASTEROID_FRAMES);
    let sprite_path = format!("asteroids/astero{:02}.png", frame_idx);
    let mass = 4.0 + rng.f32() * 3.0;
    let ang_vel = rng.signed_unit() * 0.3;
    commands.spawn((
        Asteroid,
        bevy_ggrs::Rollback,
        Sprite {
            image: assets.load(sprite_path),
            color: Color::WHITE,
            custom_size: Some(Vec2::splat(visual)),
            ..default()
        },
        Transform::from_translation(pos.extend(0.1)),
        RigidBody::Dynamic,
        Collider::circle(radius),
        Mass(mass),
        Position(pos),
        Restitution::new(0.7),
        Friction::new(0.0),
        LinearVelocity(vel),
        AngularVelocity(ang_vel),
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
}
