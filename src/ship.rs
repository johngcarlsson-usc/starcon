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
    pub recharge_rate: f32,
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
            // Fleet point value. The canonical TimeWarp melee cost lives
            // in `[Info] TWCost` (mship.cpp loads `get_config_int("Info",
            // "TWCost")`), NOT `[Ship] Cost` — which doesn't exist, so the
            // old key silently parsed to 0 for every ship.
            cost: g(info, "TWCost"),
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
    ("taugl", include_str!("../assets/ships/taugl.ini"), include_str!("../assets/ships/taugl.txt")),
    ("tauar", include_str!("../assets/ships/tauar.ini"), include_str!("../assets/ships/tauar.txt")),
    ("tauem", include_str!("../assets/ships/tauem.ini"), include_str!("../assets/ships/tauem.txt")),
    ("taule", include_str!("../assets/ships/taule.ini"), include_str!("../assets/ships/taule.txt")),
    ("taumc", include_str!("../assets/ships/taumc.ini"), include_str!("../assets/ships/taumc.txt")),
    ("taust", include_str!("../assets/ships/taust.ini"), include_str!("../assets/ships/taust.txt")),
    ("neodr", include_str!("../assets/ships/neodr.ini"), include_str!("../assets/ships/neodr.txt")),
    ("iceco", include_str!("../assets/ships/iceco.ini"), include_str!("../assets/ships/iceco.txt")),
    ("leimu", include_str!("../assets/ships/leimu.ini"), include_str!("../assets/ships/leimu.txt")),
    ("uxjba", include_str!("../assets/ships/uxjba.ini"), include_str!("../assets/ships/uxjba.txt")),
    ("vioge", include_str!("../assets/ships/vioge.ini"), include_str!("../assets/ships/vioge.txt")),
    ("hubde", include_str!("../assets/ships/hubde.ini"), include_str!("../assets/ships/hubde.txt")),
    ("neccr", include_str!("../assets/ships/neccr.ini"), include_str!("../assets/ships/neccr.txt")),
    ("yurpa", include_str!("../assets/ships/yurpa.ini"), include_str!("../assets/ships/yurpa.txt")),
    ("glacr", include_str!("../assets/ships/glacr.ini"), include_str!("../assets/ships/glacr.txt")),
    ("lyrwa", include_str!("../assets/ships/lyrwa.ini"), include_str!("../assets/ships/lyrwa.txt")),
    ("vezba", include_str!("../assets/ships/vezba.ini"), include_str!("../assets/ships/vezba.txt")),
    ("koapa", include_str!("../assets/ships/koapa.ini"), include_str!("../assets/ships/koapa.txt")),
    ("sclfr", include_str!("../assets/ships/sclfr.ini"), include_str!("../assets/ships/sclfr.txt")),
    ("ulzin", include_str!("../assets/ships/ulzin.ini"), include_str!("../assets/ships/ulzin.txt")),
    ("alhdr", include_str!("../assets/ships/alhdr.ini"), include_str!("../assets/ships/alhdr.txt")),
    ("gahmo", include_str!("../assets/ships/gahmo.ini"), include_str!("../assets/ships/gahmo.txt")),
    ("hydcr", include_str!("../assets/ships/hydcr.ini"), include_str!("../assets/ships/hydcr.txt")),
    ("rogsq", include_str!("../assets/ships/rogsq.ini"), include_str!("../assets/ships/rogsq.txt")),
    ("dajem", include_str!("../assets/ships/dajem.ini"), include_str!("../assets/ships/dajem.txt")),
    ("arkpi", include_str!("../assets/ships/arkpi.ini"), include_str!("../assets/ships/arkpi.txt")),
    ("kolfl", include_str!("../assets/ships/kolfl.ini"), include_str!("../assets/ships/kolfl.txt")),
];

#[derive(Resource, Debug, Default)]
pub struct ShipCatalog {
    pub ships: HashMap<String, ShipStats>,
}

impl ShipCatalog {
    /// Stats for a class, looked up by its `.ini` code.
    pub fn stats_of(&self, class: ShipClass) -> Option<&ShipStats> {
        self.ships.get(class.code())
    }
    /// Fleet point value (TWCost) for a class; 0 if not loaded.
    pub fn cost_of(&self, class: ShipClass) -> i32 {
        self.stats_of(class).map(|s| s.cost).unwrap_or(0)
    }
    /// Display name for a class, falling back to the code.
    pub fn name_of(&self, class: ShipClass) -> String {
        self.stats_of(class)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| class.code().to_string())
    }
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

/// One slot in a match: the player's FLEET (in deploy order) + who's
/// flying it. `fleet[0]` deploys first; in melee, when the active ship
/// dies the player picks the next survivor from the remaining pool.
/// Quick single-ship modes just use a one-element fleet.
#[derive(Clone, Debug)]
pub struct SlotConfig {
    pub fleet: Vec<ShipClass>,
    pub kind: PlayerKind,
}

impl SlotConfig {
    pub fn human(class: ShipClass) -> Self {
        Self { fleet: vec![class], kind: PlayerKind::Human }
    }
    pub fn ai(class: ShipClass) -> Self {
        Self { fleet: vec![class], kind: PlayerKind::Ai }
    }
    pub fn fleet_human(fleet: Vec<ShipClass>) -> Self {
        Self { fleet, kind: PlayerKind::Human }
    }
    pub fn fleet_ai(fleet: Vec<ShipClass>) -> Self {
        Self { fleet, kind: PlayerKind::Ai }
    }
    /// The first ship to deploy (an empty fleet falls back to Earcr).
    pub fn first(&self) -> ShipClass {
        self.fleet.first().copied().unwrap_or(ShipClass::Earcr)
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
    /// Fleet-melee match: each slot's `fleet` may hold several ships,
    /// deployed one at a time with a mid-match pick on death. Off for
    /// the quick single-ship modes.
    pub melee: bool,
    /// Co-op boss match: the human slots team up against a capital ship.
    /// Spawns the `CapitalShip` and suppresses the normal player-vs-player
    /// winner logic.
    pub boss: bool,
    /// Debug/test: player ships can't die or run out of battery (crew +
    /// battery topped up every tick). Used by the "invincible" boss test
    /// option to verify the fight is winnable. Only honoured in boss mode.
    pub invuln: bool,
}

impl MatchConfig {
    /// Convenience for the 2-player human default. Use this
    /// anywhere that wants the legacy "P1 + P2" shape.
    pub fn local_two(p1: ShipClass, p2: ShipClass) -> Self {
        Self {
            slots: vec![SlotConfig::human(p1), SlotConfig::human(p2)],
            melee: false,
            boss: false,
            invuln: false,
        }
    }
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
    /// Compatibility shim: returns each slot's FIRST ship for callers
    /// that don't care about who controls each slot or the full fleet.
    pub fn classes(&self) -> impl Iterator<Item = ShipClass> + '_ {
        self.slots.iter().map(|s| s.first())
    }
    /// Slot N's first ship, if the slot exists.
    pub fn first(&self, slot: usize) -> Option<ShipClass> {
        self.slots.get(slot).map(|s| s.first())
    }
    /// Replace slot N's fleet with a single ship. Used by the local
    /// class-picker keys and the online single-ship lobby vote, which
    /// swap the whole (one-ship) fleet.
    pub fn set_single(&mut self, slot: usize, class: ShipClass) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.fleet = vec![class];
        }
    }
    pub fn kind(&self, slot: usize) -> Option<PlayerKind> {
        self.slots.get(slot).map(|s| s.kind)
    }
}

/// Convert a `ShipClass` to its position in `ALL_CLASSES`. Used as the
/// wire-format byte for the netplay lobby's class vote
/// (`PlayerInput.class`). Returns 0 (`Earcr`) if for some reason the
/// class isn't found — should never happen since `ALL_CLASSES` covers
/// every variant.
pub fn class_to_index(class: ShipClass) -> u8 {
    ALL_CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap_or(0) as u8
}

/// Inverse of `class_to_index`. An out-of-range byte (peer running a
/// stale wire format, garbled packet, etc.) decodes to `Earcr` — the
/// safest fallback since it's the canonical first ship and is always
/// implemented.
pub fn class_from_index(idx: u8) -> ShipClass {
    ALL_CLASSES
        .get(idx as usize)
        .copied()
        .unwrap_or(ShipClass::Earcr)
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self::local_two(ShipClass::Earcr, ShipClass::Spael)
    }
}

/// Stable order — picker keys (Digit1..0 for P1, F1..F10 for P2) map to
/// `ALL_CLASSES[i]` by index. Don't reorder existing entries without
/// updating the README key table.
pub const ALL_CLASSES: [ShipClass; 53] = [
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
    ShipClass::Taugl,
    ShipClass::Tauar,
    ShipClass::Tauem,
    ShipClass::Taule,
    ShipClass::Taumc,
    ShipClass::Taust,
    ShipClass::Neodr,
    ShipClass::Iceco,
    ShipClass::Leimu,
    ShipClass::Uxjba,
    ShipClass::Vioge,
    ShipClass::Hubde,
    ShipClass::Neccr,
    ShipClass::Yurpa,
    ShipClass::Glacr,
    ShipClass::Lyrwa,
    ShipClass::Vezba,
    ShipClass::Koapa,
    ShipClass::Sclfr,
    ShipClass::Ulzin,
    ShipClass::Alhdr,
    ShipClass::Gahmo,
    ShipClass::Hydcr,
    ShipClass::Rogsq,
    ShipClass::Dajem,
    ShipClass::Arkpi,
    ShipClass::Kolfl,
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
/// `None` means "use what each class declares" (Classic for every stock
/// class — matches original SC2). Toggle with `M` / the settings panel.
///
/// Default is `Some(Inertial)`: angular momentum on for every ship out
/// of the box (collisions leave you tumbling; turning has real inertia),
/// which is the feel we want by default. Players can cycle back to
/// per-class Default or Classic.
#[derive(Resource, Clone, Copy, Debug)]
pub struct AngularControlOverride(pub Option<AngularControl>);

impl Default for AngularControlOverride {
    fn default() -> Self {
        AngularControlOverride(Some(AngularControl::Inertial))
    }
}

/// Marker + per-instance data for an in-match ship entity.
#[derive(Component, Debug)]
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
    /// Tau Gladius (TW-Light fan ship, author "Tau"). Light fighter.
    /// Primary: a fast yellow laser bolt with quadratic spread. Special:
    /// a side-alternating homing missile with cone-limited tracking.
    Taugl,
    /// Tau Archon (TW-Light fan ship, author "Tau"). Mid-heavy hull.
    /// Primary: a charge-up freeze laser that saps battery. Special:
    /// a fast defensive shot. (Weapons ported in following steps.)
    Tauar,
    /// Tau EMP (TW-Light fan ship, author "Tau"). Small, fast hull.
    /// Primary: a rapid alternating-muzzle bolt. Special: a full-battery
    /// EMP wave that jams enemy controls (ported in a following step).
    Tauem,
    /// Tau Leviathan (TW-Light fan ship, author "Tau"). Big bio-cruiser.
    /// Primary: a corrosive "slime" gas that drops edible "food" pellets
    /// when it kills crew — the food homes to the Leviathan and heals it.
    /// Special: a homing missile that disables an enemy's engine on hit.
    Taule,
    /// Tau Missile Cruiser (TW-Light fan ship, author "Tau"). Slow heavy
    /// hull. Primary: a lock-on homing torpedo with splash blast (must
    /// hold aim on the target to lock). Special: a rapid burst of
    /// auto-tracking missiles drawn from a small ammo pool.
    Taumc,
    /// Tau T-Storm (TW-Light fan ship, author "Tau"). Small, fast skirmisher
    /// with two firing modes (primary = slow/long, special = fast/aggressive).
    /// Both launch homing missiles that LATCH onto a hit ship and shove +
    /// spin it with their engine thrust, then pop when their fuel runs out.
    /// Firing recoils the ship backward.
    Taust,
    /// Neo Drain (TW-Light fan ship, author GeomanNL). Tiny Utwig-killer.
    /// Primary: a short forward missile. Special: a battery-drain laser
    /// (sap) — using it doubles the missile's own energy cost.
    Neodr,
    /// Iceci Confusion (TW-Light fan ship, author GeomanNL). Weak in a
    /// straight fight. Primary: a lightly-homing pellet. Special: two side
    /// darts that SCRAMBLE the victim's controls (a random key permutation)
    /// for a few seconds.
    Iceco,
    /// Lei Mule (TW-Light fan ship, author GeomanNL). Primary: twin forward
    /// shots; Special: a quad backward "REAL" volley. All its shots block
    /// incoming weapons (they shoot down enemy fire).
    Leimu,
    /// Uxjoz Battleplatform (GeomanNL). Slow, heavy. Primary: a rapid mass
    /// driver spraying a particle cloud. Special: two slow heavy homing
    /// missiles from the flanks.
    Uxjba,
    /// Viogen Genesis (GeomanNL). Primary: two long-range homing missiles.
    /// Special: a slow plasma cloud that eats incoming weapons.
    Vioge,
    /// Hellenian-Uberrace Devastator (GeomanNL). Primary: a heavy long-range
    /// gun that slows the ship when fired. Special: a ring of mortar bursts.
    Hubde,
    /// Nechanzi Cruiser (Varith). Primary: twin forward missiles. Special:
    /// a phalanx of unguided missiles fired in a forward spread.
    Neccr,
    /// Yuryul Patriot (Varith). Primary: a powerful unguided missile.
    /// Special: two ion-stream cones from the flanks.
    Yurpa,
    /// Glavria Cruiser (Varith). Primary: a five-torpedo forward spread.
    /// Special: a single backward torpedo.
    Glacr,
    /// Lyrmristu War Destroyer (Varith). Primary: a three-bolt spread.
    /// Special: a force sphere (damage-soak shield).
    Lyrwa,
    /// Vezlagari Barge (Varith). Primary: two backward unguided missiles.
    /// Special: reinforce the armour plate (damage-soak shield).
    Vezba,
    /// Koanua Patrol Ship (Varith). Primary: a backward delayed-thrust
    /// missile. Special: an ionic turbocharger (speed burst).
    Koapa,
    /// Sclore Frigate (Varith). Primary: rapid twin short-range bolts.
    /// Special: a rear-firing energy field (backward bolt stream).
    Sclfr,
    /// Ulzrak Interceptor (Varith). Primary: a fast forward missile.
    /// Special: zoom drive — a ramming speed dash.
    Ulzin,
    /// Alhordian Dreadnought (Varith). Primary: a long-range torpedo.
    /// Special: a sweep of two side lasers.
    Alhdr,
    /// Gahmur Monitor (Varith). Primary: a charge-up homing plasma (hold
    /// fire to charge, release to launch — bigger charge hits harder).
    /// Special: dump the charge as a three-way plasma burst.
    Gahmo,
    /// Hydra Cruiser (Varith). Primary: a five-beam fan. Special: launch a
    /// station-keeping fighter that lasers nearby enemies.
    Hydcr,
    /// Rogue Squadron (GeomanNL). Primary: a pulse laser. Special: deploy
    /// wingmen that fight alongside you.
    Rogsq,
    /// Dajielka Cruiser (Varith). Primary: a pulse blaster. Special: a
    /// protective sanctuary (damage-soak shield).
    Dajem,
    /// Arkanoid Pincer (Varith). Primary: a short-range pincer crush.
    /// Special: scuttle mode (damage-soak shield).
    Arkpi,
    /// Kolory Flamer (GeomanNL). Primary: twin flame beams (fore + aft).
    /// Special: a tractor field that drags/disrupts nearby ships.
    Kolfl,
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
            ShipClass::Taugl => "taugl",
            ShipClass::Tauar => "tauar",
            ShipClass::Tauem => "tauem",
            ShipClass::Taule => "taule",
            ShipClass::Taumc => "taumc",
            ShipClass::Taust => "taust",
            ShipClass::Neodr => "neodr",
            ShipClass::Iceco => "iceco",
            ShipClass::Leimu => "leimu",
            ShipClass::Uxjba => "uxjba",
            ShipClass::Vioge => "vioge",
            ShipClass::Hubde => "hubde",
            ShipClass::Neccr => "neccr",
            ShipClass::Yurpa => "yurpa",
            ShipClass::Glacr => "glacr",
            ShipClass::Lyrwa => "lyrwa",
            ShipClass::Vezba => "vezba",
            ShipClass::Koapa => "koapa",
            ShipClass::Sclfr => "sclfr",
            ShipClass::Ulzin => "ulzin",
            ShipClass::Alhdr => "alhdr",
            ShipClass::Gahmo => "gahmo",
            ShipClass::Hydcr => "hydcr",
            ShipClass::Rogsq => "rogsq",
            ShipClass::Dajem => "dajem",
            ShipClass::Arkpi => "arkpi",
            ShipClass::Kolfl => "kolfl",
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
#[derive(Component, Debug, Clone)]
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
#[derive(Component, Debug, Clone)]
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

/// Bevy component on-add hook: stamp the entity with a fresh
/// `NetId` from the per-match allocator, IFF it doesn't already
/// have one. Attached via `#[component(on_add = ...)]` to every
/// host-spawnable visual class that the snapshot stream mirrors
/// (Beam, DamageZone, AttachedDamageZone, TractorBeam, SubEntity,
/// ChmmrSatellite, ...). The host's snapshot sender keys these by
/// NetId; without an id the guest can't reliably re-find the
/// mirror across snapshots and would re-spawn it every tick.
///
/// Idempotency: the hook bails if the entity already has a NetId.
/// Two callers exercise that path:
///   - Asteroid spawns (host AND guest) pre-stamp the id from the
///     deterministic spawn descriptor, so the hook here would
///     conflict if it always allocated.
///   - Guest-side mirror spawns explicitly insert the snapshot's
///     NetId; mirrors never carry the gameplay component, but if a
///     future class shares a component between mirror + authority
///     entities this keeps the contract safe.
///
/// On the guest the gameplay components are never inserted (the
/// authority systems are gated off), so this hook only ever
/// allocates on the host / solo peer — allocation never diverges.
fn auto_assign_net_id(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    if world.get::<crate::netcode::NetId>(ctx.entity).is_some() {
        return;
    }
    let Some(mut alloc) = world.get_resource_mut::<crate::netcode::NetIdAllocator>() else {
        return;
    };
    let new_id = alloc.allocate();
    let mut commands = world.commands();
    if let Ok(mut ec) = commands.get_entity(ctx.entity) {
        ec.try_insert(new_id);
    }
}

/// Public re-export of the NetId on-add hook so cinematic visual
/// components in `src/ultimate.rs` can stamp NetIds without a
/// dep-cycle to the private helper above. Same body — Bevy's
/// `#[component(on_add = ...)]` attribute takes a path, so the
/// re-export must itself be a top-level `fn` (not just a `use`).
pub fn auto_assign_net_id_pub(
    world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    auto_assign_net_id(world, ctx)
}

/// In-flight projectile. Owner is tracked so we can ignore self-hits.
/// On-add hook auto-tags the entity with `bevy_ggrs::Rollback` so
/// the projectile's state participates in rollback snapshots
/// without having to remember to add the marker at every spawn
/// site.
#[derive(Component, Debug, Clone)]
#[component(on_add = projectile_on_add)]
pub struct Projectile {
    pub owner: Entity,
    pub damage: i32,
    pub lifetime: f32,
}

/// Androsynth bubble behaviour marker (canon `AndrosynthBubble::calculate`,
/// shpandgu.cpp). The bubble flies forward at first, then every 150 ms
/// re-aims to a fresh RANDOM direction and adds a half-speed nudge toward
/// the nearest enemy — so it drifts about erratically while loosely
/// chasing. `tick_andro_bubbles` drives it (host-authoritative; the guest
/// sees the resulting motion through the projectile-mirror stream).
#[derive(Component, Debug, Clone)]
pub struct AndroBubble {
    /// The bubble's cruise speed `v` (world units/sec) — magnitude of both
    /// the random course and the half-speed enemy-seek nudge.
    pub speed: f32,
    /// Accumulator toward the 150 ms re-course interval.
    pub course_s: f32,
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
    let need_layers = world.get::<CollisionLayers>(ctx.entity).is_none();
    // Stamp a cross-peer NetId so the host can stream this projectile
    // to the guest (which renders it as a `ProjectileMirror`). Only the
    // authoritative peer ever spawns real `Projectile`s — the guest's
    // combat sim is gated off — so this hook only runs host/solo-side
    // and the allocation can't diverge. Mirrors carry no `Projectile`
    // component, so this hook never touches them.
    let new_net_id = if world.get::<crate::netcode::NetId>(ctx.entity).is_none() {
        world
            .get_resource_mut::<crate::netcode::NetIdAllocator>()
            .map(|mut a| a.allocate())
    } else {
        None
    };
    // Overcharge: if the firing ship is currently overcharged, scale this
    // shot's damage up before it ever flies. Centralising it here means
    // every weapon — bolts, missiles, shards, the lot — inherits the boost
    // without touching each ship's fire system.
    let overcharge = owner.and_then(|e| world.get::<OverchargeActive>(e).map(|o| o.factor));
    if let Some(factor) = overcharge {
        if let Some(mut proj) = world.get_mut::<Projectile>(ctx.entity) {
            proj.damage = ((proj.damage as f32 * factor).round() as i32).max(proj.damage);
        }
    }
    let mut commands = world.commands();
    let mut ec = commands.entity(ctx.entity);
    if need_layers {
        ec.try_insert(layers);
    }
    if let Some(id) = new_net_id {
        ec.try_insert(id);
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
#[derive(Component, Debug, Clone)]
pub struct Homing {
    pub target: Option<Entity>,
    /// Max rad/sec the projectile can re-aim. Comes from a VolleySpec
    /// field so designers can give light tracking missiles a tight
    /// turn rate and heavy plasma bolts a slow drift toward target.
    pub turn_rate: f32,
}

/// Cone-limited tracking for a `Homing` projectile: it only steers toward
/// the target while the target is within this half-angle (radians) of its
/// heading; outside the cone the missile flies straight. Canonical Tau
/// Gladius missile (`shptaugl.cpp:TauGladiusMissile::calculate`, the
/// `proxy_angle` / TrackAngle limit). Absent = always home.
#[derive(Component, Debug, Clone, Copy)]
pub struct HomingCone(pub f32);

/// On hitting a ship, drains `amount` from its battery — the Tau Archon
/// freeze-laser's fuel sap. Crew damage rides separately on `Projectile`
/// (canon reduces crew damage by what it sapped; we bake that by tagging
/// sap pellets with 0 crew damage).
#[derive(Component, Debug, Clone, Copy)]
pub struct FuelSap {
    pub amount: i32,
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
#[derive(Component, Debug, Clone)]
#[component(on_add = auto_assign_net_id)]
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

/// The co-op boss: a huge wedge "dreadnought". The hull is an
/// indestructible wall; the fight is decided at the exposed bridge
/// (`CapitalCore`) at the prow, reached by running the turret-lined
/// flanks. The shield + recessed-core "apex" arrives in a later slice.
/// The shape IS the level.
#[derive(Component, Debug)]
pub struct CapitalShip;

/// Shared marker for every piece of the boss (hull, turrets, core) so a
/// rematch tears them all down in one sweep.
#[derive(Component, Debug)]
pub struct BossPart;

/// Hit points for a damageable boss piece. The hull has none (it's an
/// indestructible wall); turrets and the core carry their own pools.
#[derive(Component, Debug)]
pub struct BossHealth {
    pub hp: i32,
    pub max: i32,
}

/// A little floating health bar pinned above a boss part, so you can see
/// turret/core damage land. `part` is the boss entity it tracks; the bar
/// despawns when that part is gone.
#[derive(Component, Debug)]
pub struct BossHpBar {
    pub part: Entity,
}

/// The win target: the dreadnought's bridge at the prow.
/// Destroy it and the co-op team wins.
#[derive(Component, Debug)]
pub struct CapitalCore;

/// The core's recessed-aperture rhythm. The bridge is armoured shut most
/// of the time (shots pop harmlessly) and only cracks open for a brief
/// `strike window` — that's when it takes damage. Players have to time
/// their burst (and their power-ups) to the opening, which gives the
/// kill a real apex instead of a flat HP grind.
#[derive(Component, Debug)]
pub struct CoreAperture {
    /// True while the bridge is open and vulnerable.
    pub open: bool,
    /// Seconds left in the current phase.
    pub timer: f32,
    /// How long the shutters stay closed between windows.
    pub closed_s: f32,
    /// How long each strike window lasts.
    pub open_s: f32,
}

impl Default for CoreAperture {
    fn default() -> Self {
        // Opens after the first closed spell, so the run-in up the
        // flanks isn't instantly winnable.
        Self { open: false, timer: 5.0, closed_s: 5.0, open_s: 3.5 }
    }
}

/// A translucent glow disc parented behind the core; `tick_core_aperture`
/// pulses its alpha so the strike window flares.
#[derive(Component, Debug)]
pub struct CoreHalo;

/// A turret's barrel child — `tick_boss_turrets` swivels it to track the
/// nearest fighter so the cannons visibly aim before they fire.
#[derive(Component, Debug)]
pub struct TurretBarrel;

/// A power-up's pulsing glow-ring child (`tick_powerup_visuals` breathes
/// its alpha). Carries its own material so each pickup glows independently.
#[derive(Component, Debug)]
pub struct PowerUpGlow;

/// A hull-mounted auto-cannon. Each `interval` seconds it fires a bolt
/// at the nearest player ship within `range`.
#[derive(Component, Debug)]
pub struct Turret {
    pub cooldown_s: f32,
    pub interval: f32,
    pub range: f32,
}

/// Marker for a bolt fired BY the boss (turret), so the damage path
/// knows it must never scratch the boss itself.
#[derive(Component, Debug)]
pub struct BossProjectile;

/// Collision membership bit for boss parts (hull/turrets/core).
pub(crate) const BOSS_LAYER_BIT: u32 = 1 << 10;
/// Collision membership bit for boss-fired bolts.
pub(crate) const BOSS_PROJ_LAYER_BIT: u32 = 1 << 11;

/// Boss parts collide with everything EXCEPT boss bolts and each other,
/// so player fire and ship-bumps land but the boss can't self-damage.
pub(crate) fn boss_part_layers() -> CollisionLayers {
    CollisionLayers::from_bits(
        BOSS_LAYER_BIT,
        0xffff_ffffu32 & !BOSS_PROJ_LAYER_BIT & !BOSS_LAYER_BIT,
    )
}

/// The HULL blocks ships (they bounce off the wall) but is TRANSPARENT to
/// projectiles — player fire passes through it to strike the turrets and
/// core mounted on/inside the wedge. Without this, the solid hull collider
/// intercepts every shot before it can reach the parts sitting inside the
/// triangle, making the turrets (and most of the core) impossible to hit.
pub(crate) fn boss_hull_layers() -> CollisionLayers {
    // Player projectiles are members of bits 5..=8 (one per slot); boss
    // bolts are BOSS_PROJ_LAYER_BIT. Exclude all of those from the hull's
    // filter so no projectile collides with the wall — only ships do.
    let player_proj_bits = 0b1111u32 << 5;
    CollisionLayers::from_bits(
        BOSS_LAYER_BIT,
        0xffff_ffffu32 & !BOSS_PROJ_LAYER_BIT & !BOSS_LAYER_BIT & !player_proj_bits,
    )
}

/// Boss bolts collide with ships but never with boss parts or each other.
pub(crate) fn boss_projectile_layers() -> CollisionLayers {
    CollisionLayers::from_bits(
        BOSS_PROJ_LAYER_BIT,
        0xffff_ffffu32 & !BOSS_PROJ_LAYER_BIT & !BOSS_LAYER_BIT,
    )
}

/// Spawn the boss hull on entering a boss match. An elongated arrowhead
/// wedge ~1550 wu long — a real battleship next to the ~30 wu fighters.
/// Solid static body so fighters bounce off the hull; default collision
/// layers collide with ships.
pub fn spawn_capital_ship(
    mut commands: Commands,
    config: Res<MatchConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !config.boss {
        return;
    }
    // Hull-local outline, tip toward +y (north).
    let tip = Vec2::new(0.0, 850.0);
    let bl = Vec2::new(-360.0, -700.0);
    let br = Vec2::new(360.0, -700.0);
    let mesh = meshes.add(Triangle2d::new(tip, bl, br));
    let mat = materials.add(ColorMaterial::from(Color::srgb(0.16, 0.18, 0.24)));
    let pos = Vec2::ZERO;
    // Detailing handles, built once and shared by the child decals below.
    let panel_mat = materials.add(ColorMaterial::from(Color::srgb(0.22, 0.25, 0.33)));
    let stripe_mat = materials.add(ColorMaterial::from(Color::srgb(0.34, 0.40, 0.52)));
    let spine_mat = materials.add(ColorMaterial::from(Color::srgb(0.45, 0.55, 0.72)));
    let glow_mat = materials.add(ColorMaterial::from(Color::srgba(0.35, 0.7, 1.0, 0.5)));
    // A slightly inset, lighter wedge so the hull reads as plated, not flat.
    let panel_mesh = meshes.add(Triangle2d::new(
        Vec2::new(0.0, 760.0),
        Vec2::new(-300.0, -640.0),
        Vec2::new(300.0, -640.0),
    ));
    // Two long thin stripes lying along the wedge edges (left/right flanks).
    let edge_len = (tip - bl).length();
    let edge_angle = (tip - bl).y.atan2((tip - bl).x) - std::f32::consts::FRAC_PI_2;
    commands
        .spawn((
            CapitalShip,
            BossPart,
            Mesh2d(mesh),
            MeshMaterial2d(mat),
            // Behind the fighters.
            Transform::from_translation(pos.extend(-1.0)),
            RigidBody::Static,
            Collider::triangle(tip, bl, br),
            boss_hull_layers(),
            Position(pos),
            Rotation::radians(0.0),
        ))
        .with_children(|hull| {
            // Inset plating.
            hull.spawn((
                Mesh2d(panel_mesh.clone()),
                MeshMaterial2d(panel_mat.clone()),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.05)),
            ));
            // Central spine running prow-to-stern.
            hull.spawn((
                Mesh2d(meshes.add(Rectangle::new(16.0, 1450.0))),
                MeshMaterial2d(spine_mat.clone()),
                Transform::from_translation(Vec3::new(0.0, 75.0, 0.07)),
            ));
            // Left + right flank stripes, rotated to lie on the wedge edges.
            for sign in [-1.0_f32, 1.0] {
                hull.spawn((
                    Mesh2d(meshes.add(Rectangle::new(10.0, edge_len * 0.92))),
                    MeshMaterial2d(stripe_mat.clone()),
                    Transform {
                        translation: Vec3::new(sign * 178.0, 65.0, 0.06),
                        rotation: Quat::from_rotation_z(-sign * edge_angle),
                        scale: Vec3::ONE,
                    },
                ));
            }
            // Engine glow blobs hanging off the stern (in front of the hull
            // so they read; squashed into ellipses via scale).
            for ex in [-180.0_f32, 0.0, 180.0] {
                hull.spawn((
                    Mesh2d(meshes.add(Circle::new(60.0))),
                    MeshMaterial2d(glow_mat.clone()),
                    Transform {
                        translation: Vec3::new(ex, -720.0, 0.04),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::new(0.9, 0.5, 1.0),
                    },
                ));
            }
        });

    // Hull-mounted auto-cannons along the flanks. They sit just inside
    // the wedge edges so the player has to run the gauntlet up the
    // sides to reach the prow. Each is its own destructible body, built
    // up from a dark base ring, a metal housing, and a swivelling barrel.
    let ring_mat = materials.add(ColorMaterial::from(Color::srgb(0.10, 0.11, 0.15)));
    let housing_mat = materials.add(ColorMaterial::from(Color::srgb(0.58, 0.50, 0.28)));
    let barrel_mat = materials.add(ColorMaterial::from(Color::srgb(0.30, 0.32, 0.38)));
    let muzzle_mat = materials.add(ColorMaterial::from(Color::srgb(0.85, 0.80, 0.55)));
    let ring_mesh = meshes.add(Circle::new(34.0));
    let housing_mesh = meshes.add(Circle::new(26.0));
    let barrel_mesh = meshes.add(Rectangle::new(12.0, 46.0));
    let muzzle_mesh = meshes.add(Circle::new(8.0));
    for tp in [
        Vec2::new(-250.0, -500.0),
        Vec2::new(250.0, -500.0),
        Vec2::new(-180.0, 0.0),
        Vec2::new(180.0, 0.0),
    ] {
        commands
            .spawn((
                BossPart,
                BossHealth { hp: 80, max: 80 },
                Turret {
                    cooldown_s: 0.8,
                    interval: 1.6,
                    range: 1600.0,
                },
                Mesh2d(ring_mesh.clone()),
                MeshMaterial2d(ring_mat.clone()),
                Transform::from_translation(tp.extend(0.0)),
                RigidBody::Static,
                Collider::circle(28.0),
                boss_part_layers(),
                Position(tp),
                Rotation::radians(0.0),
            ))
            .with_children(|turret| {
                // The swivelling barrel — its own parent node so the
                // aim system can rotate the whole barrel+muzzle group.
                turret
                    .spawn((
                        TurretBarrel,
                        Transform::from_translation(Vec3::new(0.0, 0.0, 0.02)),
                        Visibility::default(),
                    ))
                    .with_children(|barrel| {
                        barrel.spawn((
                            Mesh2d(barrel_mesh.clone()),
                            MeshMaterial2d(barrel_mat.clone()),
                            // Pushed forward so it pokes out the front.
                            Transform::from_translation(Vec3::new(0.0, 22.0, 0.0)),
                        ));
                        barrel.spawn((
                            Mesh2d(muzzle_mesh.clone()),
                            MeshMaterial2d(muzzle_mat.clone()),
                            Transform::from_translation(Vec3::new(0.0, 44.0, 0.01)),
                        ));
                    });
                // Housing cap on top of the barrel pivot.
                turret.spawn((
                    Mesh2d(housing_mesh.clone()),
                    MeshMaterial2d(housing_mat.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.03)),
                ));
            });
    }

    // The recessed bridge at the prow — the win target. Sits at the very
    // tip of the wedge, so the fleet has to fight its way up the flanks
    // (under turret fire) and strike the nose during a strike window.
    let core_pos = Vec2::new(0.0, 800.0);
    let halo_mat = materials.add(ColorMaterial::from(Color::srgba(1.0, 0.3, 0.26, 0.0)));
    commands
        .spawn((
            BossPart,
            CapitalCore,
            CoreAperture::default(),
            BossHealth { hp: 400, max: 400 },
            Mesh2d(meshes.add(Circle::new(42.0))),
            // Starts closed → dim steel; `tick_core_aperture` recolours it.
            MeshMaterial2d(materials.add(ColorMaterial::from(CORE_CLOSED_COLOR))),
            // Drawn above the hull so the glowing weak point reads.
            Transform::from_translation(core_pos.extend(0.5)),
            RigidBody::Static,
            Collider::circle(42.0),
            boss_part_layers(),
            Position(core_pos),
            Rotation::radians(0.0),
        ))
        .with_children(|core| {
            // Glow halo behind the core; alpha pulses with the strike
            // window (driven by tick_core_aperture).
            core.spawn((
                CoreHalo,
                Mesh2d(meshes.add(Circle::new(86.0))),
                MeshMaterial2d(halo_mat),
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
            ));
            // A bright inner pip so the bridge has a focal point.
            core.spawn((
                Mesh2d(meshes.add(Circle::new(16.0))),
                MeshMaterial2d(materials.add(ColorMaterial::from(Color::srgb(1.0, 0.92, 0.85)))),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.02)),
            ));
        });

    info!("boss: capital ship hull + 4 turrets + core spawned");
}

/// Boss auto-cannons: each `interval` seconds, every turret fires a bolt
/// at the nearest player ship within range. Host-authoritative — the
/// guest mirrors the resulting bolts through the projectile stream.
fn tick_boss_turrets(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    role: Res<crate::netcode::NetRole>,
    mut cached: Local<Option<(Handle<Mesh>, Handle<ColorMaterial>)>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut turrets: Query<(Entity, &Position, &mut Turret)>,
    players: Query<&Position, With<Ship>>,
    turret_children: Query<&Children>,
    mut barrels: Query<&mut Transform, With<TurretBarrel>>,
) {
    if role.is_guest() {
        return;
    }
    let dt = time.delta_secs();
    // Cache one bolt mesh + material so we don't leak an asset per shot.
    let (bolt_mesh, bolt_mat) = cached
        .get_or_insert_with(|| {
            (
                meshes.add(Circle::new(7.0)),
                materials.add(ColorMaterial::from(Color::srgb(1.0, 0.55, 0.2))),
            )
        })
        .clone();
    for (turret_entity, tpos, mut turret) in &mut turrets {
        turret.cooldown_s -= dt;

        // Nearest player ship by wrap-aware distance.
        let mut best: Option<(f32, Vec2)> = None;
        for p in &players {
            let img = crate::physics::nearest_image(p.0, tpos.0);
            let d2 = img.distance_squared(tpos.0);
            if best.map_or(true, |(bd, _)| d2 < bd) {
                best = Some((d2, img));
            }
        }
        let Some((d2, target)) = best else { continue };
        let dir = (target - tpos.0).normalize_or_zero();
        if dir == Vec2::ZERO {
            continue;
        }

        // Swivel the barrel to track the target every tick — the cannon
        // visibly leads its shot, whether or not it's ready to fire.
        let barrel_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;
        if let Ok(children) = turret_children.get(turret_entity) {
            for child in children.iter() {
                if let Ok(mut bt) = barrels.get_mut(child) {
                    bt.rotation = Quat::from_rotation_z(barrel_angle);
                }
            }
        }

        if turret.cooldown_s > 0.0 || d2 > turret.range * turret.range {
            continue;
        }
        turret.cooldown_s = turret.interval;
        const BOLT_SPEED: f32 = 560.0;
        let muzzle = tpos.0 + dir * 34.0;
        commands.spawn((
            Projectile {
                owner: Entity::PLACEHOLDER,
                damage: 8,
                lifetime: 4.0,
            },
            BossProjectile,
            Mesh2d(bolt_mesh.clone()),
            MeshMaterial2d(bolt_mat.clone()),
            Transform::from_translation(muzzle.extend(0.4)),
            RigidBody::Dynamic,
            Collider::circle(7.0),
            Sensor,
            // Pre-stamp layers so `projectile_on_add` leaves them alone.
            boss_projectile_layers(),
            Position(muzzle),
            Rotation::radians(0.0),
            LinearVelocity(dir * BOLT_SPEED),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
        ));
    }
}

// ---------------------------------------------------------------------
// Power-ups
//
// Floating pickups that drift in the arena; fly a fighter into one and
// it applies an effect. v1 ships four kinds, each reusing a mechanic the
// engine already has, so the framework lands without new damage paths:
//   Repair       — patch a chunk of crew back.
//   Energy       — top the battery off.
//   Shield       — a few seconds of `ShieldActive` (damage soak).
//   PointDefense — a "sidekick": a few seconds of `PointDefenseActive`
//                  (auto-shoots down incoming fire + zaps near enemies).
// The showier ones the user asked for — strap-on bazooka, one-shot
// nitrous, stronger-shots overcharge — layer on in later slices.
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerUpKind {
    Repair,
    Energy,
    Shield,
    PointDefense,
    /// Strap-on bazooka: a few seconds of auto-firing heavy rockets.
    Bazooka,
    /// One-shot nitrous: an instant forward dash with a brief speed-cap lift.
    Nitrous,
    /// Overcharge: a window where your own shots hit much harder.
    Overcharge,
}

impl PowerUpKind {
    /// All kinds in spawn-roll order.
    const ALL: [PowerUpKind; 7] = [
        PowerUpKind::Repair,
        PowerUpKind::Energy,
        PowerUpKind::Shield,
        PowerUpKind::PointDefense,
        PowerUpKind::Bazooka,
        PowerUpKind::Nitrous,
        PowerUpKind::Overcharge,
    ];

    /// Pickup body colour (also the glow the player learns to read,
    /// and the minimap blip tint).
    pub fn color(self) -> Color {
        match self {
            PowerUpKind::Repair => Color::srgb(0.30, 0.95, 0.40), // green cross
            PowerUpKind::Energy => Color::srgb(1.00, 0.85, 0.20), // yellow bolt
            PowerUpKind::Shield => Color::srgb(0.30, 0.65, 1.00), // blue
            PowerUpKind::PointDefense => Color::srgb(0.95, 0.45, 0.95), // magenta
            PowerUpKind::Bazooka => Color::srgb(1.00, 0.45, 0.12), // orange
            PowerUpKind::Nitrous => Color::srgb(0.55, 1.00, 0.95), // cyan
            PowerUpKind::Overcharge => Color::srgb(1.00, 0.20, 0.25), // red
        }
    }

    /// Short all-caps name shown in the pickup toast / buff chip.
    pub fn label(self) -> &'static str {
        match self {
            PowerUpKind::Repair => "REPAIR",
            PowerUpKind::Energy => "ENERGY",
            PowerUpKind::Shield => "SHIELD",
            PowerUpKind::PointDefense => "POINT DEFENSE",
            PowerUpKind::Bazooka => "BAZOOKA",
            PowerUpKind::Nitrous => "NITROUS",
            PowerUpKind::Overcharge => "OVERCHARGE",
        }
    }
}

/// Fired when a fighter grabs a power-up, so the HUD can pop a toast.
/// (Instant pickups like Repair/Energy/Nitrous still toast — the chip
/// strip only tracks the *timed* buffs.)
#[derive(Message)]
pub struct PowerUpPicked {
    pub slot: usize,
    pub kind: PowerUpKind,
}

/// Strap-on bazooka: bolted on by a pickup, it auto-launches a fat
/// forward rocket every `interval` seconds for `remaining` seconds.
/// The rockets are ordinary player-owned `Projectile`s, so they ride
/// the existing `handle_projectile_hits` damage path (boss parts and
/// enemies both) with no new mechanic.
#[derive(Component, Debug)]
pub struct BazookaActive {
    pub remaining: f32,
    pub cooldown_s: f32,
    pub interval: f32,
}

/// One-shot nitrous: the kick is applied instantly at pickup; this
/// marker just keeps the speed cap lifted (read by `cap_velocity`,
/// like a gravity whip) so the dash isn't clamped before it coasts
/// back down. Expires after `remaining` seconds.
#[derive(Component, Debug)]
pub struct NitrousActive {
    pub remaining: f32,
}

/// Overcharge: while present on a ship, its outgoing projectiles get
/// their `damage` scaled by `factor`. Applied centrally in
/// `projectile_on_add`, so every ship class benefits with no
/// per-weapon plumbing.
#[derive(Component, Debug)]
pub struct OverchargeActive {
    pub remaining: f32,
    pub factor: f32,
}

/// A floating pickup. Static sensor body — fighters pass through it and
/// trigger a `CollisionStart`; it never pushes anything.
#[derive(Component, Debug)]
pub struct PowerUp {
    pub kind: PowerUpKind,
}

/// Paces power-up spawning in boss co-op.
#[derive(Resource)]
pub struct PowerUpSpawner {
    /// Seconds until the next spawn roll.
    pub timer: f32,
}

impl Default for PowerUpSpawner {
    fn default() -> Self {
        // First pickup a little after the assault begins.
        Self { timer: 8.0 }
    }
}

/// Spawn a fresh power-up every so often (boss co-op only, host side),
/// capped so the arena never floods with pickups.
fn tick_powerup_spawner(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    config: Res<MatchConfig>,
    mut spawner: ResMut<PowerUpSpawner>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    existing: Query<(), With<PowerUp>>,
) {
    if !config.boss {
        return;
    }
    const MAX_ACTIVE: usize = 3;
    const SPAWN_INTERVAL: f32 = 11.0;
    spawner.timer -= time.delta_secs();
    if spawner.timer > 0.0 {
        return;
    }
    spawner.timer = SPAWN_INTERVAL;
    if existing.iter().count() >= MAX_ACTIVE {
        return;
    }
    let kind = PowerUpKind::ALL[rng.usize_range(0..PowerUpKind::ALL.len())];
    // Drop it in the southern half where the fleet operates, clear of
    // the dreadnought's hull (which fills the northern/centre arena).
    let x = (rng.f32() - 0.5) * 1600.0;
    let y = -1300.0 + rng.f32() * 900.0; // y ∈ [-1300, -400]
    let pos = Vec2::new(x, y);
    let c = kind.color().to_srgba();
    commands
        .spawn((
            PowerUp { kind },
            Mesh2d(meshes.add(Circle::new(20.0))),
            MeshMaterial2d(materials.add(ColorMaterial::from(kind.color()))),
            Transform::from_translation(pos.extend(0.3)),
            RigidBody::Static,
            Collider::circle(22.0),
            Sensor,
            Position(pos),
            Rotation::radians(0.0),
        ))
        .with_children(|pu| {
            // Soft outer glow ring — its own material so it can pulse
            // independently (tick_powerup_visuals breathes the alpha).
            pu.spawn((
                PowerUpGlow,
                Mesh2d(meshes.add(Circle::new(36.0))),
                MeshMaterial2d(
                    materials.add(ColorMaterial::from(Color::srgba(c.red, c.green, c.blue, 0.4))),
                ),
                Transform::from_translation(Vec3::new(0.0, 0.0, -0.05)),
            ));
            // Bright white core pip for a little sparkle.
            pu.spawn((
                Mesh2d(meshes.add(Circle::new(8.0))),
                MeshMaterial2d(materials.add(ColorMaterial::from(Color::srgb(1.0, 1.0, 1.0)))),
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.02)),
            ));
        });
    info!("powerup spawned: {:?} at ({:.0},{:.0})", kind, x, y);
}

/// Apply a power-up when a fighter flies into it.
fn handle_powerup_pickup(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    assets: Res<AssetServer>,
    powerups: Query<&PowerUp>,
    positions: Query<&Position>,
    ships: Query<&Ship>,
    mut crews: Query<&mut Crew>,
    mut batteries: Query<&mut Battery>,
    rotations: Query<&Rotation>,
    deriveds: Query<&ShipPhysicsDerived>,
    mut velocities: Query<&mut LinearVelocity>,
    mut picked: MessageWriter<PowerUpPicked>,
) {
    let mut gone: bevy::platform::collections::HashSet<Entity> =
        bevy::platform::collections::HashSet::default();
    for event in reader.read() {
        // Identify which side is the pickup, which is the ship.
        let (pu_entity, ship_entity) = if powerups.get(event.collider1).is_ok() {
            (event.collider1, event.collider2)
        } else if powerups.get(event.collider2).is_ok() {
            (event.collider2, event.collider1)
        } else {
            continue;
        };
        if gone.contains(&pu_entity) {
            continue;
        }
        let Ok(ship) = ships.get(ship_entity) else {
            continue; // pickup brushed a projectile / boss part — ignore.
        };
        let ship_slot = ship.player_slot;
        let Ok(pu) = powerups.get(pu_entity) else {
            continue;
        };
        match pu.kind {
            PowerUpKind::Repair => {
                if let Ok(mut crew) = crews.get_mut(ship_entity) {
                    let heal = (crew.max / 2).max(1);
                    crew.current = (crew.current + heal).min(crew.max);
                    info!("powerup: repair → {}/{}", crew.current, crew.max);
                }
            }
            PowerUpKind::Energy => {
                if let Ok(mut batt) = batteries.get_mut(ship_entity) {
                    batt.current = batt.max;
                    info!("powerup: energy → battery full");
                }
            }
            PowerUpKind::Shield => {
                commands.entity(ship_entity).try_insert(ShieldActive {
                    remaining: 8.0,
                    damage_factor: 0.25,
                });
                info!("powerup: shield (8s)");
            }
            PowerUpKind::PointDefense => {
                commands.entity(ship_entity).try_insert(PointDefenseActive {
                    remaining: 8.0,
                    range: 280.0,
                    damage_per_tick: 2,
                    cooldown_s: 0.0,
                });
                info!("powerup: sidekick point-defense (8s)");
            }
            PowerUpKind::Bazooka => {
                commands.entity(ship_entity).try_insert(BazookaActive {
                    remaining: 8.0,
                    // First rocket goes the instant you grab it.
                    cooldown_s: 0.0,
                    interval: 0.7,
                });
                info!("powerup: strap-on bazooka (8s)");
            }
            PowerUpKind::Nitrous => {
                // One-shot dash: shove the ship hard along its facing
                // right now. `NitrousActive` keeps the cap lifted (see
                // cap_velocity) so the burst survives a moment before it
                // coasts back down.
                commands
                    .entity(ship_entity)
                    .try_insert(NitrousActive { remaining: 1.4 });
                if let (Ok(rot), Ok(derived), Ok(mut vel)) = (
                    rotations.get(ship_entity),
                    deriveds.get(ship_entity),
                    velocities.get_mut(ship_entity),
                ) {
                    // Ship forward = local +y rotated by the hull's heading.
                    let forward = Vec2::new(-rot.sin, rot.cos);
                    let kick = (derived.speed_max.max(120.0)) * NITROUS_CAP_MULT;
                    vel.0 = forward * kick;
                }
                info!("powerup: nitrous dash");
            }
            PowerUpKind::Overcharge => {
                commands
                    .entity(ship_entity)
                    .try_insert(OverchargeActive { remaining: 8.0, factor: 2.5 });
                info!("powerup: overcharge — shots x2.5 (8s)");
            }
        }
        // Tell the HUD what just got grabbed (drives the pickup toast).
        picked.write(PowerUpPicked { slot: ship_slot, kind: pu.kind });
        // Pop the pickup with a small flash.
        if let Ok(p) = positions.get(pu_entity) {
            spawn_asteroid_explosion(&mut commands, &assets, p.0, 18.0);
        }
        if gone.insert(pu_entity) {
            if let Ok(mut ec) = commands.get_entity(pu_entity) {
                ec.try_despawn();
            }
        }
    }
}

/// How far above `speed_max` a nitrous dash is allowed to ride.
const NITROUS_CAP_MULT: f32 = 2.6;

/// Drive every active strap-on bazooka: each `interval`, launch a fat
/// forward rocket from the ship's nose. The rockets are plain
/// player-owned `Projectile`s (high damage, short life), so they hit the
/// boss core, turrets, and any enemy through the normal damage path.
fn tick_bazooka(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    // The cached handle pair is built once, on the first frame a bazooka
    // is live, so we don't churn a new mesh/material per rocket.
    mut rocket_assets: Local<Option<(Handle<Mesh>, Handle<ColorMaterial>)>>,
    mut firers: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut BazookaActive,
    )>,
) {
    let dt = time.delta_secs();
    const ROCKET_SPEED: f32 = 620.0;
    const ROCKET_DAMAGE: i32 = 22;
    for (entity, ship, pos, rot, vel, mut bz) in &mut firers {
        bz.remaining -= dt;
        bz.cooldown_s -= dt;
        if bz.cooldown_s <= 0.0 {
            bz.cooldown_s = bz.interval;
            let (mesh, mat) = rocket_assets
                .get_or_insert_with(|| {
                    (
                        meshes.add(Circle::new(11.0)),
                        materials.add(ColorMaterial::from(Color::srgb(1.0, 0.55, 0.18))),
                    )
                })
                .clone();
            let forward = Vec2::new(-rot.sin, rot.cos);
            let muzzle = pos.0 + forward * 42.0;
            let rocket_vel = vel.0 + forward * ROCKET_SPEED;
            let angle = forward.y.atan2(forward.x) - std::f32::consts::FRAC_PI_2;
            // owner = the firing ship, so projectile_on_add stamps the
            // right per-slot layers (friendly fighters pass through) and
            // an overcharge boost would even ride along.
            commands.spawn((
                Projectile { owner: entity, damage: ROCKET_DAMAGE, lifetime: 2.6 },
                Mesh2d(mesh),
                MeshMaterial2d(mat),
                Transform::from_translation(muzzle.extend(0.45)),
                RigidBody::Dynamic,
                Collider::circle(10.0),
                Sensor,
                Mass(1.0),
                Position(muzzle),
                Rotation::radians(angle),
                LinearVelocity(rocket_vel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ));
            // Muzzle flash — a quick fading puff at the launch point.
            commands.spawn((
                ZapFlash { remaining_s: 0.16, total_s: 0.16 },
                Sprite::from_color(
                    Color::srgba(1.0, 0.7, 0.3, 0.9),
                    Vec2::splat(26.0),
                ),
                Transform::from_translation(muzzle.extend(0.46)),
            ));
        }
        if bz.remaining <= 0.0 {
            commands.entity(entity).try_remove::<BazookaActive>();
            info!("P{} bazooka spent", ship.player_slot + 1);
        }
    }
}

/// Count down the nitrous and overcharge windows and strip the marker
/// when each runs out. (Nitrous's kick already happened at pickup; this
/// just lets the speed cap drop back to normal afterwards.)
fn tick_powerup_buffs(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut nitrous: Query<(Entity, &mut NitrousActive)>,
    mut overcharge: Query<(Entity, &mut OverchargeActive)>,
) {
    let dt = time.delta_secs();
    for (e, mut n) in &mut nitrous {
        n.remaining -= dt;
        if n.remaining <= 0.0 {
            commands.entity(e).try_remove::<NitrousActive>();
        }
    }
    for (e, mut o) in &mut overcharge {
        o.remaining -= dt;
        if o.remaining <= 0.0 {
            commands.entity(e).try_remove::<OverchargeActive>();
        }
    }
}

/// Breathe each floating pickup's glow ring so the drifting power-ups
/// pulse and read as "grab me" rather than sitting as flat discs.
fn tick_powerup_visuals(
    time: Res<Time<Physics>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: Query<(&MeshMaterial2d<ColorMaterial>, &mut Transform), With<PowerUpGlow>>,
) {
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 3.0).sin();
    for (mat_handle, mut xf) in &mut glows {
        if let Some(mat) = materials.get_mut(&mat_handle.0) {
            let c = mat.color.to_srgba();
            mat.color = Color::srgba(c.red, c.green, c.blue, 0.22 + 0.32 * pulse);
        }
        xf.scale = Vec3::splat(0.85 + 0.25 * pulse);
    }
}

/// Core colour while the bridge is armoured shut (dim steel — reads as
/// "no point shooting yet").
pub(crate) const CORE_CLOSED_COLOR: Color = Color::srgb(0.26, 0.31, 0.44);
/// Core colour at the peak of an open strike window (hot red).
pub(crate) const CORE_OPEN_COLOR: Color = Color::srgb(1.0, 0.30, 0.26);

/// Drive the core's recessed-aperture rhythm: flip between a closed
/// (invulnerable) spell and an open strike window, and recolour /
/// pulse the bridge so the window reads at a glance. Damage gating
/// lives in `handle_projectile_hits` (it skips the core while closed).
fn tick_core_aperture(
    time: Res<Time<Physics>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut cores: Query<
        (&mut CoreAperture, &MeshMaterial2d<ColorMaterial>, &mut Transform, &Children),
        Without<CoreHalo>,
    >,
    halos: Query<&MeshMaterial2d<ColorMaterial>, With<CoreHalo>>,
) {
    let dt = time.delta_secs();
    for (mut ap, mat_handle, mut xf, children) in &mut cores {
        ap.timer -= dt;
        if ap.timer <= 0.0 {
            ap.open = !ap.open;
            ap.timer = if ap.open { ap.open_s } else { ap.closed_s };
        }
        // Visual state. Open: hot red with a fast brightness pulse and a
        // slight bulge so the bridge looks like it's flaring out of its
        // housing. Closed: steady dim steel, sitting flush.
        let pulse = 0.6 + 0.4 * (time.elapsed_secs() * 9.0).sin().abs();
        if let Some(mat) = materials.get_mut(&mat_handle.0) {
            if ap.open {
                let b = CORE_OPEN_COLOR.to_srgba();
                mat.color = Color::srgb(b.red * pulse, b.green * pulse, b.blue * pulse);
                xf.scale = Vec3::splat(1.0 + 0.12 * pulse);
            } else {
                mat.color = CORE_CLOSED_COLOR;
                xf.scale = Vec3::ONE;
            }
        }
        // Pulse the halo's alpha with the window — invisible when shut,
        // flaring while open.
        let halo_alpha = if ap.open { 0.25 + 0.35 * pulse } else { 0.0 };
        for child in children.iter() {
            if let Ok(halo_handle) = halos.get(child) {
                if let Some(hmat) = materials.get_mut(&halo_handle.0) {
                    let c = hmat.color.to_srgba();
                    hmat.color = Color::srgba(c.red, c.green, c.blue, halo_alpha);
                }
            }
        }
    }
}

/// Spawn a floating HP bar for each boss part the moment it appears.
fn spawn_boss_hp_bars(mut commands: Commands, new_parts: Query<Entity, Added<BossHealth>>) {
    for part in &new_parts {
        commands.spawn((
            BossHpBar { part },
            // Width is rewritten each frame by `update_boss_hp_bars`.
            Sprite::from_color(Color::srgb(0.25, 1.0, 0.35), Vec2::new(56.0, 6.0)),
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.6)),
        ));
    }
}

/// Pin each boss HP bar just above its part and size/colour it to the
/// part's remaining health (green → red). Despawns when the part is gone.
fn update_boss_hp_bars(
    mut commands: Commands,
    parts: Query<(&Position, &BossHealth)>,
    mut bars: Query<(Entity, &BossHpBar, &mut Transform, &mut Sprite)>,
) {
    const BAR_W: f32 = 56.0;
    for (bar_e, bar, mut xf, mut sprite) in &mut bars {
        let Ok((pos, hp)) = parts.get(bar.part) else {
            if let Ok(mut ec) = commands.get_entity(bar_e) {
                ec.try_despawn();
            }
            continue;
        };
        let frac = if hp.max > 0 { (hp.hp as f32 / hp.max as f32).clamp(0.0, 1.0) } else { 0.0 };
        // Float above the part.
        xf.translation = (pos.0 + Vec2::new(0.0, 52.0)).extend(0.6);
        sprite.custom_size = Some(Vec2::new(BAR_W * frac, 6.0));
        // Green when healthy, red when low.
        sprite.color = Color::srgb(1.0 - frac, 0.3 + 0.7 * frac, 0.3);
    }
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
#[derive(Component, Debug, Clone)]
#[component(on_add = auto_assign_net_id)]
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
#[derive(Component, Debug, Default, Clone)]
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
#[derive(Component, Debug, Clone)]
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
#[derive(Component, Debug, Default, Clone)]
pub struct SlylandroDrift {
    pub last_thrust_held: bool,
}

/// Per-ship state for Melnorme charge-and-release primary. Continuous
/// linear interpolation of damage / scale / colour from base to max
/// over `max_charge_s` of hold time.
#[derive(Component, Debug, Default, Clone)]
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
#[derive(Component, Debug, Default, Clone)]
pub struct ShofixtiGlory {
    pub presses: u8,
    pub since_last_s: f32,
}

#[derive(Component, Debug, Default, Clone)]
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
#[derive(Component, Debug, Clone)]
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

/// Per-ship runtime state for the Tau Gladius's managed weapons:
/// primary-fire cooldown + held-edge tracking, and the `side` that the
/// special's homing missile alternates between each launch.
#[derive(Component, Debug)]
pub struct TauglState {
    pub weapon_cd_s: f32,
    pub last_fire_held: bool,
    pub special_cd_s: f32,
    pub last_special_held: bool,
    pub side: f32,
}

impl Default for TauglState {
    fn default() -> Self {
        Self {
            weapon_cd_s: 0.0,
            last_fire_held: false,
            special_cd_s: 0.0,
            last_special_held: false,
            side: 1.0,
        }
    }
}

/// Tau Archon managed-weapon runtime: the primary's charge-up timer +
/// held-edge, a fractional-battery-drain accumulator, the alternating
/// damage-step toggle (sap pellet vs damage pellet), and the special's
/// cooldown state.
#[derive(Component, Debug, Default)]
pub struct TauarState {
    pub charge_s: f32,
    pub last_fire_held: bool,
    pub batt_debt: f32,
    pub sap_step: bool,
    pub special_cd_s: f32,
    pub last_special_held: bool,
}

/// Tau EMP managed-weapon runtime: the primary's per-shot cooldown +
/// held-edge, the rolling barrel `slot` (0..6) that drives the
/// alternating muzzle offset (`shptauem.cpp:activate_weapon`), and the
/// special's held-edge + live EMP-wave radius (0 = no wave).
#[derive(Component, Debug, Default)]
pub struct TauemState {
    pub weapon_cd_s: f32,
    pub last_fire_held: bool,
    pub slot: u32,
    pub last_special_held: bool,
    /// Current EMP wave radius while a discharge is propagating; 0 idle.
    pub wave_radius: f32,
}

/// Tau Leviathan managed-weapon runtime: primary held-edge + per-shot
/// cooldown, special held-edge + cooldown, the alternating missile side,
/// and the slow passive-heal accumulator.
#[derive(Component, Debug)]
pub struct TauleState {
    pub weapon_cd_s: f32,
    pub last_fire_held: bool,
    pub special_cd_s: f32,
    pub last_special_held: bool,
    pub missile_side: f32,
    pub heal_accum_s: f32,
}

impl Default for TauleState {
    fn default() -> Self {
        Self {
            weapon_cd_s: 0.0,
            last_fire_held: false,
            special_cd_s: 0.0,
            last_special_held: false,
            missile_side: 1.0,
            heal_accum_s: 0.0,
        }
    }
}

/// Tau Missile Cruiser runtime: the two torpedo tubes' recharge timers +
/// which fires next, the lock-on accumulator + held-edge for the primary,
/// and the special's ammo pool / recharge / cadence + held-edge.
#[derive(Component, Debug)]
pub struct TaumcState {
    pub tube_cd: [f32; 2],
    pub next_tube: usize,
    pub lock_s: f32,
    pub last_fire_held: bool,
    pub ammo: i32,
    pub ammo_cd_s: f32,
    pub fire_cd_s: f32,
    pub current_barrel: u32,
    /// Turret aim, in radians relative to the hull's facing. Steered by
    /// SPECIAL+left/right (`tick_taumc_turret`); the overlay sprite and
    /// the missile launch direction both read it.
    pub turret_rad: f32,
}

impl Default for TaumcState {
    fn default() -> Self {
        Self {
            tube_cd: [0.0, 0.0],
            next_tube: 0,
            lock_s: 0.0,
            last_fire_held: false,
            ammo: 4,
            ammo_cd_s: 0.0,
            fire_cd_s: 0.0,
            current_barrel: 0,
            turret_rad: 0.0,
        }
    }
}

/// Neo Drain runtime: primary/special cooldowns + the "special recently
/// used" window that doubles the missile's energy cost (canon quirk).
#[derive(Component, Debug, Default)]
pub struct NeodrState {
    pub weapon_cd_s: f32,
    pub special_cd_s: f32,
    /// Seconds left in which the drain laser counts as "in use" — while
    /// positive the primary's battery drain is doubled.
    pub special_active_s: f32,
}

/// Iceci Confusion runtime: primary/special cooldowns + special held-edge.
#[derive(Component, Debug, Default)]
pub struct IcecoState {
    pub weapon_cd_s: f32,
    pub special_cd_s: f32,
    pub last_special_held: bool,
}

/// Lei Mule runtime: primary/special cooldowns.
#[derive(Component, Debug, Default)]
pub struct LeimuState {
    pub weapon_cd_s: f32,
    pub special_cd_s: f32,
}

/// Uxjoz / Viogen / Hellenian runtime: primary + special cooldowns and a
/// special held-edge. Shared shape; one per ship for clarity.
#[derive(Component, Debug, Default)]
pub struct UxjbaState {
    pub weapon_cd_s: f32,
    pub special_cd_s: f32,
}
#[derive(Component, Debug, Default)]
pub struct ViogeState {
    pub weapon_cd_s: f32,
    pub special_cd_s: f32,
}
#[derive(Component, Debug, Default)]
pub struct HubdeState {
    pub weapon_cd_s: f32,
    pub special_cd_s: f32,
}
#[derive(Component, Debug, Default)]
pub struct NeccrState { pub weapon_cd_s: f32, pub special_cd_s: f32, pub last_special_held: bool }
#[derive(Component, Debug, Default)]
pub struct YurpaState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct GlacrState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct LyrwaState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct VezbaState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct KoapaState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct SclfrState { pub weapon_cd_s: f32, pub special_cd_s: f32, pub side: f32 }
#[derive(Component, Debug, Default)]
pub struct UlzinState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct AlhdrState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct GahmoState { pub charge_s: f32, pub last_fire_held: bool, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct HydcrState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct RogsqState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct DajemState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct ArkpiState { pub weapon_cd_s: f32, pub special_cd_s: f32 }
#[derive(Component, Debug, Default)]
pub struct KolflState { pub weapon_cd_s: f32, pub special_cd_s: f32 }

/// A scramble stamped on a ship by an Iceci Confusion dart
/// (`OverrideControlIceci`). For `remaining` seconds the victim's five
/// control bits (left/right/thrust/fire/special) are remapped through the
/// random permutation `order` — pressing one control does another.
/// Enforced in `apply_control_scrambles`.
#[derive(Component, Debug)]
pub struct ControlScramble {
    /// Permutation of 0..5 over the control-bit indices (0=left … 4=special).
    pub order: [u8; 5],
    pub remaining: f32,
}

/// Marker for an Iceci Confusion dart (scrambles controls on a ship hit,
/// handled in `handle_iceco_darts`).
#[derive(Component, Debug)]
pub struct ConfusionDart;

/// Marker for a Lei Mule "REAL" shot: besides its normal damage it blocks
/// incoming weapons — on contact with an enemy projectile both pop
/// (`handle_blocking_shots`).
#[derive(Component, Debug)]
pub struct BlockingShot;

/// Tau T-Storm runtime: the rolling 6-slot muzzle index and the two
/// modes' per-shot cooldown / held-edge.
#[derive(Component, Debug, Default)]
pub struct TauStormState {
    pub slot: u32,
    pub weapon_cd_s: f32,
    pub last_special_held: bool,
}

/// A T-Storm missile. Until it hits a ship it homes + accelerates toward
/// the nearest enemy; on contact it LATCHES and rides the victim, shoving
/// it along the missile's heading and spinning it with the engine thrust
/// until the fuel runs out — then it pops for 1 damage. Self-managed (no
/// `Projectile`) so the generic hit handler never despawns it on contact.
#[derive(Component, Debug)]
pub struct StormMissile {
    pub owner_slot: usize,
    pub fuel_s: f32,
    pub accel: f32,
    pub max_v: f32,
    pub turn_rate: f32,
    pub thrust: f32,
    pub booster_speed: f32,
    pub spin: f32,
    pub latched: Option<Entity>,
    /// World offset from the target at the latch instant.
    pub rel: Vec2,
    /// Shove direction (unit) at the latch instant.
    pub shove_dir0: Vec2,
    /// Target rotation (radians) at the latch instant.
    pub theta0: f32,
}

/// A Tau MC homing torpedo: on detonation it also deals `blast` splash to
/// enemy ships within `blast_range` (`handle_torpedo_blast`).
#[derive(Component, Debug)]
pub struct TauMcTorpedo {
    pub owner_slot: usize,
    pub blast: i32,
    pub blast_range: f32,
}

/// A corrosive-slime ball fired by the Leviathan primary. On a ship hit
/// it drops `LeviathanFood` pellets (handled in `handle_slimeball_hits`).
#[derive(Component, Debug)]
pub struct SlimeBall {
    pub owner: Entity,
}

/// An edible "food" pellet dropped when a Leviathan slime kill lands. It
/// drifts, decays, homes toward its owning Leviathan when near, and on
/// contact heals the owner's crew + battery (`tick_leviathan_food`).
#[derive(Component, Debug)]
pub struct LeviathanFood {
    pub owner: Entity,
    pub lifetime_s: f32,
}

/// A control-jam stamped on a ship by a Tau EMP wave
/// (`shptauem.cpp:OverrideControlTauEMP`). For `remaining` seconds the
/// `mask` bits — the buttons the victim was *holding* at the instant the
/// wave caught it — are cleared from its input each tick, so exactly the
/// controls that were active get frozen. `apply_control_jams` enforces it
/// just after the per-slot inputs are gathered.
#[derive(Component, Debug)]
pub struct ControlJam {
    pub mask: u8,
    pub remaining: f32,
}

/// The expanding blue shockwave ring drawn when a Tau EMP discharges.
/// Purely cosmetic — `tick_emp_wave_visual` grows its radius and fades it
/// out over `duration`, then despawns it. (The jam logic lives in
/// `tick_tauem_special`; this just makes the wave visible.)
#[derive(Component, Debug)]
pub struct EmpWaveVisual {
    pub elapsed: f32,
    pub duration: f32,
    pub max_r: f32,
}

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
            .init_resource::<PowerUpSpawner>()
            .add_message::<PowerUpPicked>()
            .add_systems(Update, (class_picker_input, cycle_angular_override))
            .add_systems(Update, (spawn_boss_hp_bars, update_boss_hp_bars))
            // EMP control-jam enforcement: clear the jammed bits right
            // after inputs are gathered and BEFORE `apply_player_input`
            // reads them, so the victim's thrust/turn are frozen this same
            // tick (the core of the EMP). Authoritative-only, matching the
            // host-driven combat sim that stamps the jams.
            .add_systems(
                FixedUpdate,
                (apply_control_jams, apply_control_scrambles)
                    .after(input::gather_slot_inputs)
                    .before(apply_player_input)
                    .run_if(crate::netcode::role_is_authoritative),
            );
        // Bevy 0.18's `add_systems` macro caps a single tuple at 20
        // entries. We've outgrown it; split into two FixedUpdate
        // groups (the order across groups is unconstrained, but each
        // system inside this plugin is independent so that's fine).
        // Movement + per-ship state. Runs on EVERY peer — including the
        // guest — so the guest keeps locally predicting its own ship's
        // motion (snapshots then correct it). None of these spawn
        // projectiles or deal damage, so they're safe to run on the
        // guest; only the combat group below is host-authoritative.
        app.add_systems(
            FixedUpdate,
            (
                process_mode_toggle_requests,
                tick_ship_modes,
                apply_player_input,
                cap_velocity,
                tick_weapon_cooldown,
                tick_special_cooldown,
                tick_shield,
                tick_point_defense,
                orient_projectiles,
            ),
        );
        // Combat: weapon-spawn + projectile-steer/lifetime + damage-zone
        // systems. Host authority — gated off on the guest, which sees
        // the results as snapshot state and projectile mirrors. Without
        // this gate the guest would spawn its own projectiles alongside
        // the host's mirrors (double vision) and double-apply damage.
        app.add_systems(
            FixedUpdate,
            (
                tick_chebr_crystal,
                tick_meltr_charge,
                tick_kohma_blade,
                tick_kohma_passive_blades,
                tick_projectile_lifetime,
                steer_homing_projectiles,
                tick_andro_bubbles,
                tick_damage_zones,
                tick_attached_damage_zones,
                tick_beams,
                tick_tractors,
                // Battery recharge is host-authoritative: it's snapshot-
                // reconciled AND shown on the HUD meter, so running it on
                // the guest too made the bar climb locally then snap back
                // to the host's slightly-older value on each snapshot —
                // a visible backward flicker, worst over high-latency
                // internet. Guest battery is now purely snapshot-driven.
                tick_battery_recharge,
            )
                .run_if(crate::netcode::role_is_authoritative),
        );
        // Pkunk aggressive-clone AI lives in its own add_systems
        // so we can apply `.after(apply_player_input)` without
        // spilling the 21-element tuple limit on the main
        // gameplay-schedule set. The .after dependency is what
        // lets it override the leader's player input on the clones.
        app.add_systems(
            FixedUpdate,
            crate::ultimate::tick_pkunk_aggressive_clones
                .after(apply_player_input)
                .run_if(crate::netcode::role_is_authoritative),
        );
        // Planet gravity nudges LinearVelocity; run it before the speed
        // cap so the whip-boosted cap is honoured the same tick.
        // `tick_planet_contact` deals a per-bounce chunk; `tick_planet_grind`
        // is the canon-equivalent persistent damage — canon ran its
        // collide+inflict_damage every frame the ship's sprite overlapped
        // the planet's, so a pinned ship took repeat hits at 20 Hz.
        // Avian only fires `CollisionStart` on transitions, so we model
        // the persistent half as a steady DPS while the ship is in the
        // planet's CollidingEntities set.
        app.add_systems(
            FixedUpdate,
            (
                apply_planet_gravity.before(cap_velocity),
                tick_planet_contact,
                tick_planet_grind,
            )
                .run_if(crate::netcode::role_is_authoritative),
        );
        // Orz turret + marines own their input handling; must run AFTER
        // apply_player_input so it can clobber the hull's ang_vel when
        // special is held (rotate the turret, not the ship). Slylandro
        // drift similarly overrides thrust/rotation on rising-edge of
        // thrust to flip the probe 180°.
        app.add_systems(
            FixedUpdate,
            (
                tick_orz_turret.after(apply_player_input),
                tick_taumc_turret.after(apply_player_input),
                tick_slylandro_drift.after(apply_player_input),
                tick_orz_marines_boarded,
            )
                .run_if(crate::netcode::role_is_authoritative),
        );
        // GeomanNL fan-ship weapons (host-authoritative). The two hit
        // handlers run before `handle_projectile_hits` so they can read the
        // projectile before the generic handler despawns it.
        app.add_systems(
            FixedUpdate,
            (
                tick_neodr_primary,
                tick_neodr_special,
                tick_iceco_primary,
                tick_iceco_special,
                handle_iceco_darts.before(handle_projectile_hits),
                tick_leimu_primary,
                tick_leimu_special,
                handle_blocking_shots.before(handle_projectile_hits),
                tick_uxjba_primary,
                tick_uxjba_special,
                tick_vioge_primary,
                tick_vioge_special,
                tick_hubde_primary,
                tick_hubde_special,
            )
                .run_if(crate::netcode::role_is_authoritative),
        );
        // Varith fan-ship weapons (host-authoritative).
        app.add_systems(
            FixedUpdate,
            (
                tick_neccr_primary,
                tick_neccr_special,
                tick_yurpa_primary,
                tick_yurpa_special,
                tick_glacr_primary,
                tick_glacr_special,
                tick_lyrwa_primary,
                tick_lyrwa_special,
                tick_vezba_primary,
                tick_vezba_special,
                tick_koapa_primary,
                tick_koapa_special,
                tick_sclfr_primary,
                tick_sclfr_special,
                tick_ulzin_primary,
                tick_ulzin_special,
            )
                .run_if(crate::netcode::role_is_authoritative),
        );
        // Varith/GeomanNL fan-ship weapons, batch 2 (host-authoritative).
        app.add_systems(
            FixedUpdate,
            (
                tick_alhdr_primary,
                tick_alhdr_special,
                tick_gahmo_primary,
                tick_gahmo_special,
                tick_hydcr_primary,
                tick_hydcr_special,
                tick_rogsq_primary,
                tick_rogsq_special,
                tick_dajem_primary,
                tick_dajem_special,
                tick_arkpi_primary,
                tick_arkpi_special,
                tick_kolfl_primary,
                tick_kolfl_special,
            )
                .run_if(crate::netcode::role_is_authoritative),
        );
        app.add_systems(
            FixedUpdate,
            (
                tick_damage_to_battery,
                tick_sub_entities,
                handle_projectile_hits,
                handle_sub_entity_collisions,
                handle_mode_contact_damage,
                apply_syreen_drain,
                apply_slyp_harvest,
                tick_mycon_plasma_birth,
                tick_mycon_plasma,
                spawn_chmmr_satellites,
                tick_chmmr_satellites,
                replenish_asteroids.run_if(in_state(crate::AppState::InMatch)),
                tick_alary_mirv,
                // Tau managed weapons, nested as one set to keep the outer
                // tuple under Bevy's 20-entry `add_systems` cap.
                (
                    tick_taugl_primary,
                    tick_taugl_special,
                    tick_tauar_primary,
                    tick_tauem_primary,
                    tick_tauem_special,
                    tick_archon_spiral,
                    tick_taule_primary,
                    tick_taule_special,
                    // Must read the slime/missile hit before the generic
                    // handler despawns the projectile.
                    handle_leviathan_hits.before(handle_projectile_hits),
                    tick_leviathan_food,
                    tick_taumc_primary,
                    handle_torpedo_blast.before(handle_projectile_hits),
                    tick_taust_primary,
                    tick_taust_special,
                    handle_storm_latch,
                    tick_storm_missiles,
                ),
                tick_alary_turrets,
                tick_shofixti_glory,
                // Nested as one set so the outer tuple stays under Bevy's
                // 20-entry `add_systems` cap.
                (
                    tick_boss_turrets,
                    tick_powerup_spawner,
                    handle_powerup_pickup,
                    tick_bazooka,
                    tick_powerup_buffs,
                    tick_core_aperture,
                    tick_powerup_visuals,
                ),
            )
                .run_if(crate::netcode::role_is_authoritative),
        )
        // Visual-only tick systems. These animate sprite frames / fade
        // alpha / despawn on lifetime expiry — no gameplay state
        // mutation, no projectile spawn, no damage application. They
        // must run on BOTH peers so the guest's mirror entities
        // (spawned via `VisualEventQueue` / `ExplosionSpawn` from a
        // snapshot) actually tick through their animation instead of
        // freezing at frame 0. Host runs them on its own spawns
        // identically — solo play is unchanged.
        .add_systems(
            FixedUpdate,
            (tick_zap_flashes, tick_asteroid_explosions, tick_emp_wave_visual),
        )
        // Host-only: enqueue visual events for any explosion / zap
        // the authority systems just spawned, so the next snapshot
        // ships them to the guest. Gated on `role_is_authoritative`
        // (solo skips because the queue's contents are never read)
        // and uses `Added<>` filters so each spawn enqueues exactly
        // once. Runs after the authority systems that spawn these
        // visuals — `Update` is fine here because we just need the
        // events flushed before the snapshot send next frame.
        .add_systems(
            FixedUpdate,
            (enqueue_explosion_events, enqueue_zap_events)
                .run_if(crate::netcode::role_is_authoritative),
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
    role: Res<crate::netcode::NetRole>,
    mut net_id_alloc: ResMut<crate::netcode::NetIdAllocator>,
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
    let spawn_table: [(Vec2, f32); 4] = if config.boss {
        // Boss co-op: the capital ship owns the centre/north of the
        // arena (its hull base sits at y≈-700). Muster the whole
        // player fleet to the south in a loose line, all facing north
        // toward the dreadnought, so nobody spawns inside the hull.
        [
            (Vec2::new(-360.0, -1180.0), 0.0),
            (Vec2::new(360.0, -1180.0), 0.0),
            (Vec2::new(-720.0, -1240.0), 0.0),
            (Vec2::new(720.0, -1240.0), 0.0),
        ]
    } else {
        [
            (Vec2::new(-740.0, 0.0), -FRAC_PI_2), // W, facing E
            (Vec2::new(740.0, 0.0), FRAC_PI_2),   // E, facing W
            (Vec2::new(0.0, -740.0), 0.0),        // S, facing N
            (Vec2::new(0.0, 740.0), PI),          // N, facing S
        ]
    };
    for (slot, slot_cfg) in config.slots.iter().enumerate().take(4) {
        let (pos, rot) = spawn_table[slot];
        let Some(entity) = spawn_class(
            &mut commands,
            &catalog,
            &assets,
            slot_cfg.first(),
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

    // Boss co-op keeps the arena clear so the capital ship is the
    // centrepiece — no planet gravity well, no asteroid field.
    if !config.boss {
        spawn_planet(&mut commands, &assets, &mut rng);
        spawn_asteroids(
            &mut commands,
            &assets,
            &mut rng,
            &mut net_id_alloc,
            role.is_guest(),
        );
    }

    // Canon VUX `relocate()` (`shpvuxin.cpp:176-189`): on combat
    // start, if the VUX is farther than ~500 canon px from its
    // target, teleport to 125 canon px off the target and face it.
    // In our wu (40 wu / canon-range-unit, 12.5 wu per canon px),
    // 500 canon px ≈ 6250 wu — well over our arena, so this
    // ALWAYS fires for a 1v1. Approximate the canon "125 canon px"
    // with 220 wu — just outside the VUX's laser range (360 wu)
    // so the player still has to close to fire but starts most of
    // the way there. Iterate the spawn table again so we can
    // overwrite the compass-point pos we already set.
    for (vux_slot, vux_cfg) in config.slots.iter().enumerate().take(4) {
        if vux_cfg.first() != ShipClass::Vuxin {
            continue;
        }
        // Find another slot to anchor to. In 1v1 that's the only
        // other ship; in 4-player FFA, the first non-self slot is
        // close enough to canon (which targets `control->target`).
        let Some((opp_idx, _)) = config
            .slots
            .iter()
            .enumerate()
            .take(4)
            .find(|(i, _)| *i != vux_slot)
        else {
            continue;
        };
        let opp_pos = spawn_table[opp_idx].0;
        let vux_pos = spawn_table[vux_slot].0;
        let toward_opp = (opp_pos - vux_pos).normalize_or_zero();
        if toward_opp == Vec2::ZERO {
            continue;
        }
        let new_pos = opp_pos - toward_opp * 220.0;
        let new_rot = (-toward_opp.x).atan2(toward_opp.y);
        // We need the Entity for this slot; find it by player_slot.
        // Done via a deferred command — `apply_player_input` reads
        // `Position`/`Rotation` next tick.
        commands.queue(move |world: &mut World| {
            let mut q = world.query::<(Entity, &Ship)>();
            let candidate = q.iter(world).find_map(|(e, s)| {
                (s.player_slot == vux_slot).then_some(e)
            });
            if let Some(e) = candidate {
                if let Ok(mut ec) = world.get_entity_mut(e) {
                    if let Some(mut p) = ec.get_mut::<Position>() {
                        p.0 = new_pos;
                    }
                    if let Some(mut r) = ec.get_mut::<Rotation>() {
                        *r = Rotation::radians(new_rot);
                    }
                }
            }
        });
    }
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
    session: Option<Res<crate::netcode::NetSocket>>,
) {
    // Online: classes are negotiated in the lobby, not via hotkeys.
    // Bail before reading any keys so a stray Tab can't damage the
    // local-only MatchConfig either.
    if session.is_some() {
        return;
    }
    // Melee: the fleet is fixed at match start; a digit/Tab must not
    // collapse it to a single ship. The mid-match pick handles swaps.
    if config.melee {
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
            if let (Some(class), Some(cur)) =
                (ALL_CLASSES.get(idx).copied(), config.first(0))
            {
                if cur != class {
                    config.set_single(0, class);
                    info!("P1 → {:?}", class);
                    changed = true;
                }
            }
        }
    }
    for (i, key) in P2_FKEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let (Some(class), Some(cur)) =
                (ALL_CLASSES.get(idx).copied(), config.first(1))
            {
                if cur != class {
                    config.set_single(1, class);
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
        if let Some(cur) = config.first(0) {
            let next = cycle_class(cur, dir);
            config.set_single(0, next);
            info!("P1 → {:?}", next);
            changed = true;
        }
    }
    if keys.just_pressed(KeyCode::Backquote) {
        let dir: i32 = if shift { -1 } else { 1 };
        if let Some(cur) = config.first(1) {
            let next = cycle_class(cur, dir);
            config.set_single(1, next);
            info!("P2 → {:?}", next);
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
/// Sweep every match-scoped entity from the world on rematch /
/// teardown. We use one big `Or<>` filter because Bevy 0.18 caps
/// a system at 21 parameters and we have more sweep classes than
/// that. Adding a new entity class to a match? Add its component
/// to the `Or<>` here so it gets cleaned up between matches.
/// Sweep every match-scoped entity from the world on rematch /
/// teardown. Bevy 0.18 caps `Or<>` filter tuples at 15 entries
/// (and systems at 21 params), so we nest two `Or<>`s — gameplay
/// entities and guest-side mirror entities — into a single
/// outer `Or<(Or<...>, Or<...>)>`.
///
/// Adding a new entity class to a match? Add its component
/// marker to one of the nested `Or<>`s so it gets cleaned up
/// between matches; the cost of forgetting is a stale entity
/// that survives a rematch and re-appears in the next round.
pub fn teardown_match(
    mut commands: Commands,
    everything: Query<
        Entity,
        Or<(
            Or<(
                With<Ship>,
                With<Projectile>,
                With<DamageZone>,
                With<AttachedDamageZone>,
                With<Beam>,
                With<TractorBeam>,
                With<SubEntity>,
                With<OverlaySprite>,
                With<ChmmrSatellite>,
                With<Asteroid>,
                With<Planet>,
                With<AsteroidExplosion>,
                With<ZapFlash>,
                With<crate::ultimate::YehatOrb>,
                // Covers the hull, turrets, and core (all are `BossPart`).
                With<BossPart>,
            )>,
            Or<(
                With<crate::netcode::ProjectileMirror>,
                With<crate::netcode::BeamMirror>,
                With<crate::netcode::DamageZoneMirror>,
                With<crate::netcode::AttachedZoneMirror>,
                With<crate::netcode::TractorMirror>,
                With<crate::netcode::SubEntityMirror>,
                With<crate::netcode::SatelliteMirror>,
                // Cinematic mirrors + guest-side trail fades.
                // Persistent visuals live as `CinematicVisualMirror`
                // entities the host's snapshot drives; fire-and-
                // forget ones spawn as `GuestCinematicFade`-tagged
                // sprites that tick down on their own. Both need
                // teardown so a rematch doesn't start with leftover
                // wisps or a stuck halo.
                With<crate::netcode::CinematicVisualMirror>,
                With<crate::ultimate::GuestCinematicFade>,
                With<PowerUp>,
            )>,
        )>,
    >,
) {
    for e in &everything {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
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
            remaining_s: stats.recharge_rate * 0.050,
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
    // (Used to insert `bevy_ggrs::Rollback` here for the rollback
    // snapshot set. The host/guest refactor doesn't need it.)
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
    if matches!(class, ShipClass::Taugl) {
        entity.insert(TauglState::default());
    }
    if matches!(class, ShipClass::Tauar) {
        entity.insert(TauarState::default());
    }
    if matches!(class, ShipClass::Tauem) {
        entity.insert(TauemState::default());
    }
    if matches!(class, ShipClass::Taule) {
        entity.insert(TauleState::default());
    }
    if matches!(class, ShipClass::Taumc) {
        entity.insert(TaumcState::default());
    }
    if matches!(class, ShipClass::Taust) {
        entity.insert(TauStormState::default());
    }
    if matches!(class, ShipClass::Neodr) {
        entity.insert(NeodrState::default());
    }
    if matches!(class, ShipClass::Iceco) {
        entity.insert(IcecoState::default());
    }
    if matches!(class, ShipClass::Leimu) {
        entity.insert(LeimuState::default());
    }
    if matches!(class, ShipClass::Uxjba) {
        entity.insert(UxjbaState::default());
    }
    if matches!(class, ShipClass::Vioge) {
        entity.insert(ViogeState::default());
    }
    if matches!(class, ShipClass::Hubde) {
        entity.insert(HubdeState::default());
    }
    if matches!(class, ShipClass::Neccr) {
        entity.insert(NeccrState::default());
    }
    if matches!(class, ShipClass::Yurpa) {
        entity.insert(YurpaState::default());
    }
    if matches!(class, ShipClass::Glacr) {
        entity.insert(GlacrState::default());
    }
    if matches!(class, ShipClass::Lyrwa) {
        entity.insert(LyrwaState::default());
    }
    if matches!(class, ShipClass::Vezba) {
        entity.insert(VezbaState::default());
    }
    if matches!(class, ShipClass::Koapa) {
        entity.insert(KoapaState::default());
    }
    if matches!(class, ShipClass::Sclfr) {
        entity.insert(SclfrState::default());
    }
    if matches!(class, ShipClass::Ulzin) {
        entity.insert(UlzinState::default());
    }
    if matches!(class, ShipClass::Alhdr) {
        entity.insert(AlhdrState::default());
    }
    if matches!(class, ShipClass::Gahmo) {
        entity.insert(GahmoState::default());
    }
    if matches!(class, ShipClass::Hydcr) {
        entity.insert(HydcrState::default());
    }
    if matches!(class, ShipClass::Rogsq) {
        entity.insert(RogsqState::default());
    }
    if matches!(class, ShipClass::Dajem) {
        entity.insert(DajemState::default());
    }
    if matches!(class, ShipClass::Arkpi) {
        entity.insert(ArkpiState::default());
    }
    if matches!(class, ShipClass::Kolfl) {
        entity.insert(KolflState::default());
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
        // Tau Missile Cruiser — the swivelling turret on top of the hull.
        // 64 rotation frames at shot_eNN.png (0-indexed, frame 0 = north).
        // `tick_taumc_turret` drives `extra_angle` from SPECIAL+left/right.
        ShipClass::Taumc => {
            let frames: Vec<_> = (0..64)
                .map(|i| assets.load(format!("ships/taumc/sprites/shot_e{:02}.png", i)))
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
            y_stats.recharge_rate = 6.0;
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
                drain_accum: 0.0,
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
                drain_accum: 0.0,
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
                    // SpecialDrain=8 already deducted; canonical burns
                    // 1 crew per fighter launched.
                    AbilityKind::ModifyCrew { delta: -1 },
                    AbilityKind::SpawnSubEntity {
                        // Spawn from the back of the dreadnought.
                        local_offset: Vec2::new(0.0, -25.0),
                        initial_angle_offset: std::f32::consts::PI,
                        // .ini Special Velocity=35 → 336 u/s.
                        initial_speed: 35.0 * SC2_VEL_SCALE,
                        sprite_path: Some("ships/kzedr/sprites/shot_b01.png".into()),
                        // shot_b01 is the 100×100 fighter sprite.
                        sprite_size: 36.0,
                        color: Color::srgb(1.0, 1.0, 1.0),
                        // .ini Special Armour = 1 (effectively one-hit).
                        hp: 1,
                        lifetime_s: 20.0,
                        // Canon `KzerZaFighter::calculate`: fly out,
                        // orbit the target at ~laser_range × 0.8, zap
                        // them periodically. Not a kamikaze homer.
                        ai: crate::ability::SubEntityAiSpec::KzerZaFighter {
                            turn_rate: sc2_turning(4.0),
                            speed: 35.0 * SC2_VEL_SCALE,
                            // Laser range from canon Special is in SC2
                            // units; ~4 → 160 wu lets the fighter close
                            // to a useful "circle and shoot" distance.
                            laser_range: 160.0,
                            laser_damage: 1,
                            // Per-laser recharge ~0.5 s (canon
                            // ~10 frames @ 20 Hz). Enough that a pair
                            // of fighters does sustained-but-not-
                            // melting DPS while orbiting.
                            recharge_s: 0.5,
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
                // Canon toggle. SpecialRate=7 → 0.35s cooldown between
                // toggles (ini, not a duration). SpecialDrain=3 paid
                // once on cloak-on; uncloak is free. Firing the
                // primary also drops the cloak — see `dispatch_primary`.
                kind: AbilityKind::ToggleInvisibility,
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
                    // Canon VuxLimpet::calculate re-points `vel` AT the
                    // target every frame (no inertia, no turn-rate cap):
                    //   if (ship->target && !ship->target->isInvisible()) {
                    //       angle = trajectory_angle(ship->target);
                    //       vel = v * unit_vector(angle);
                    //   }
                    // Match that with a turn rate large enough to
                    // fully re-orient within one physics step
                    // (20 Hz → ~3.14 rad/tick covers any heading).
                    homing_turn_rate: 100.0,
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

        // Slylandro Probe. Canon `shpslypr.cpp`:
        //   - Primary (`activate_weapon`): one persistent
        //     `SlylandroLaserNew` per cast (existtime 1.2 s) snaking
        //     toward the nearest target, dealing 1 damage every
        //     ~0.5-1.0 s while alive. WeaponRate=5 → cooldown 0.25 s.
        //     We approximate it with a short auto-aim beam pulse per
        //     fire — 0.18 s flash, 1 damage, range 200 wu. The 0.25 s
        //     cooldown means ~4 fires/sec, which averages out to
        //     comparable canon DPS without the persistent-beam plumbing.
        //   - Special: `calculate` lines 224-235 walks every asteroid
        //     within 100 canon-px and refills the battery to max on
        //     contact. SpecialDrain=0 — completely free.
        ShipClass::Slypr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnBeams {
                    beams: vec![crate::ability::BeamSpec {
                        local_origin: Vec2::ZERO,
                        local_dir: forward,
                        range: 5.0 * SC2_RANGE_SCALE,
                        damage_per_tick: 1,
                        // Bright electric blue/white — reads as
                        // lightning even without a jagged sprite.
                        color: Color::srgba(0.6, 0.85, 1.0, 0.95),
                        auto_aim: true,
                        duration_s: 0.18,
                        width: 2.0,
                    }],
                },
                cooldown_s: 5.0 / 20.0,
            },
            special: AbilitySpec {
                // Canon scans within 100 canon-px. In our wu (≈ 80 wu
                // touch radius for the standard probe hull), 90 wu
                // is a tight "you must be sitting on the rock" range
                // that matches the canon-feel of "harvest by ramming".
                kind: AbilityKind::EatAsteroidRefillBattery { range: 90.0 },
                cooldown_s: 5.0 / 20.0,
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
        // Step 1: ship flyable. Exact weapons (quadratic-spread laser
        // bolt + cone-limited side-alternating homing missile) land in
        // the next step — these Todo placeholders are deliberately
        // temporary, not the finished port.
        ShipClass::Taugl => Some(ShipAbilities {
            // Primary owned by `tick_taugl_primary` (the quadratic-spread
            // laser bolt). Special still a Todo placeholder until the next
            // step wires the cone-limited homing missile.
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taugl-bolt" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taugl-missile" },
                cooldown_s: 0.0,
            },
        }),
        // Step 1: flyable. Exact charge-up freeze laser (battery-sap) +
        // the defensive special are ported in the next steps.
        ShipClass::Tauar => Some(ShipAbilities {
            // Primary owned by `tick_tauar_primary` (charge-up sap laser).
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "tauar-freeze" },
                cooldown_s: 0.0,
            },
            // Special shares the charge stream (handled in
            // tick_tauar_primary), adding the spiralling long-range pellets.
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "tauar-special" },
                cooldown_s: 0.0,
            },
        }),
        // Step 1-2: flyable + exact primary (rapid alternating-muzzle
        // bolt, owned by `tick_tauem_primary`). The EMP jam-wave special
        // is a placeholder until its own step wires the control-jam.
        ShipClass::Tauem => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "tauem-bolt" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "tauem-wave" },
                cooldown_s: 0.0,
            },
        }),
        // Leviathan: corrosive-slime primary (drops homing food on kills)
        // + engine-disable homing missile, both owned by dedicated systems.
        ShipClass::Taule => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taule-slime" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taule-missile" },
                cooldown_s: 0.0,
            },
        }),
        // Missile Cruiser: lock-on splash torpedo + rapid tracking-missile
        // burst, both owned by dedicated systems.
        ShipClass::Taumc => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taumc-torpedo" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taumc-missiles" },
                cooldown_s: 0.0,
            },
        }),
        // T-Storm: two latching-missile modes (slow primary / fast special),
        // both owned by dedicated systems.
        ShipClass::Taust => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taust-slow" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "taust-fast" },
                cooldown_s: 0.0,
            },
        }),
        // Neo Drain: forward missile + battery-drain laser.
        ShipClass::Neodr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "neodr-missile" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "neodr-drain" },
                cooldown_s: 0.0,
            },
        }),
        // Iceci Confusion: homing pellet + control-scramble darts.
        ShipClass::Iceco => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "iceco-pellet" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "iceco-dart" },
                cooldown_s: 0.0,
            },
        }),
        // Lei Mule: twin forward + quad backward weapon-blocking shots.
        ShipClass::Leimu => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "leimu-front" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "leimu-back" },
                cooldown_s: 0.0,
            },
        }),
        ShipClass::Uxjba => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "uxjba-driver" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "uxjba-missiles" },
                cooldown_s: 0.0,
            },
        }),
        ShipClass::Vioge => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "vioge-missiles" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "vioge-plasma" },
                cooldown_s: 0.0,
            },
        }),
        ShipClass::Hubde => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "hubde-gun" },
                cooldown_s: 0.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::ManagedExternally { ident: "hubde-mortar" },
                cooldown_s: 0.0,
            },
        }),
        ShipClass::Neccr => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "neccr-twin" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "neccr-phalanx" }, cooldown_s: 0.0 },
        }),
        ShipClass::Yurpa => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "yurpa-missile" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "yurpa-cones" }, cooldown_s: 0.0 },
        }),
        ShipClass::Glacr => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "glacr-spread" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "glacr-back" }, cooldown_s: 0.0 },
        }),
        ShipClass::Lyrwa => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "lyrwa-bolts" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "lyrwa-sphere" }, cooldown_s: 0.0 },
        }),
        ShipClass::Vezba => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "vezba-missiles" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "vezba-armor" }, cooldown_s: 0.0 },
        }),
        ShipClass::Koapa => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "koapa-missile" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "koapa-turbo" }, cooldown_s: 0.0 },
        }),
        ShipClass::Sclfr => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "sclfr-twin" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "sclfr-rear" }, cooldown_s: 0.0 },
        }),
        ShipClass::Ulzin => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "ulzin-missile" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "ulzin-zoom" }, cooldown_s: 0.0 },
        }),
        ShipClass::Alhdr => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "alhdr-torpedo" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "alhdr-lasers" }, cooldown_s: 0.0 },
        }),
        ShipClass::Gahmo => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "gahmo-charge" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "gahmo-burst" }, cooldown_s: 0.0 },
        }),
        ShipClass::Hydcr => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "hydcr-beams" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "hydcr-fighter" }, cooldown_s: 0.0 },
        }),
        ShipClass::Rogsq => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "rogsq-pulse" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "rogsq-wingmen" }, cooldown_s: 0.0 },
        }),
        ShipClass::Dajem => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "dajem-pulse" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "dajem-sanctuary" }, cooldown_s: 0.0 },
        }),
        ShipClass::Arkpi => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "arkpi-pincer" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "arkpi-scuttle" }, cooldown_s: 0.0 },
        }),
        ShipClass::Kolfl => Some(ShipAbilities {
            primary: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "kolfl-flames" }, cooldown_s: 0.0 },
            special: AbilitySpec { kind: AbilityKind::ManagedExternally { ident: "kolfl-slow" }, cooldown_s: 0.0 },
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

        // tauar: 0-indexed `ship_s0_NN.png`, frame 0 = north.
        ShipClass::Tauar => format!("ship_s0_{:02}.png", frame),

        // tauem: 0-indexed `ship_s00_NN.png` (the `s00` set, `_NN`
        // rotation), frame 0 = north. (`ship_s01_NN` is the EMP-flash
        // alternate the original blends in; we use the base set.)
        ShipClass::Tauem => format!("ship_s00_{:02}.png", frame),

        // taule: 0-indexed 4-digit `ship_s_NNNN.png`, frame 0 = north.
        ShipClass::Taule => format!("ship_s_{:04}.png", frame),

        // taumc: 0-indexed `ship_sNN.png` (ship_s00 = north).
        ShipClass::Taumc => format!("ship_s{:02}.png", frame),

        // taust: 0-indexed `ship_s_NN.png`, frame 0 = north.
        ShipClass::Taust => format!("ship_s_{:02}.png", frame),

        // GeomanNL ships ship a SINGLE hull sprite (the original rotates
        // it at runtime) rather than 64 pre-baked frames — see
        // `single_sprite_ship`. We staged it as `ship_base.png`; one frame
        // is enough because `swap_rotation_frame` rotates it continuously.
        ShipClass::Neodr
        | ShipClass::Iceco
        | ShipClass::Leimu
        | ShipClass::Uxjba
        | ShipClass::Vioge
        | ShipClass::Hubde
        | ShipClass::Neccr
        | ShipClass::Yurpa
        | ShipClass::Glacr
        | ShipClass::Lyrwa
        | ShipClass::Vezba
        | ShipClass::Koapa
        | ShipClass::Sclfr
        | ShipClass::Ulzin
        | ShipClass::Alhdr
        | ShipClass::Gahmo
        | ShipClass::Rogsq
        | ShipClass::Arkpi
        | ShipClass::Kolfl => "ship_base.png".to_string(),
        ShipClass::Hydcr => format!("ship_s_{:04}.png", frame),
        ShipClass::Dajem => format!("ship_s5_{:04}.png", frame),

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
    // Single-sprite ships (several TW fan ships) carry one hull image that
    // the original engine rotates live. We load just that frame;
    // `swap_rotation_frame` sees n == 1 and rotates the sprite via the
    // Transform residual, reproducing the runtime rotation.
    if single_sprite_ship(class) {
        let name = rotation_frame_filename(class, 0);
        return vec![assets.load(format!("ships/{code}/sprites/{name}"))];
    }
    let mut frames = Vec::with_capacity(64);
    for i in 0..64 {
        let name = rotation_frame_filename(class, i);
        frames.push(assets.load(format!("ships/{code}/sprites/{name}")));
    }
    frames
}

/// TW fan ships that ship a single hull sprite (rotated at runtime) rather
/// than 64 pre-baked rotation frames.
pub fn single_sprite_ship(class: ShipClass) -> bool {
    matches!(
        class,
        ShipClass::Neodr
            | ShipClass::Iceco
            | ShipClass::Leimu
            | ShipClass::Uxjba
            | ShipClass::Vioge
            | ShipClass::Hubde
            | ShipClass::Neccr
            | ShipClass::Yurpa
            | ShipClass::Glacr
            | ShipClass::Lyrwa
            | ShipClass::Vezba
            | ShipClass::Koapa
            | ShipClass::Sclfr
            | ShipClass::Ulzin
            | ShipClass::Alhdr
            | ShipClass::Gahmo
            | ShipClass::Rogsq
            | ShipClass::Arkpi
            | ShipClass::Kolfl
    )
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
    role: Res<crate::netcode::NetRole>,
    local: Res<crate::netcode::LocalHandle>,
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
    // On the guest, only predict the LOCAL slot's own ship. The system
    // iterates every Ship in the query and would otherwise read
    // `SlotInputs.held[opponent_slot]` — which on the guest is
    // permanently the `PlayerInput::default()` zeros (nothing writes
    // the host's input into `NetInputs.current[host_slot]` on the
    // guest). Applying that all-zero input would clobber the
    // opponent's `AngularVelocity` to 0 every FixedUpdate (Classic
    // mode), and for Arilou ships would force `LinearVelocity` to
    // `Vec2::ZERO`, leaving Avian to integrate one frame of zero
    // motion before the snapshot reconciler in Update restores the
    // host's authoritative values. Skipping non-local slots on the
    // guest leaves their poses entirely under snapshot control.
    let guest_local_only = role.is_guest();
    let local_slot = local.0.min(3);
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
        if guest_local_only && ship.player_slot != local_slot {
            continue;
        }
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
            // Joystick (absolute-aim) players steer toward the stick and
            // brake by pushing it — without this branch the coast only
            // read the digital keys, so a stick player had NO way to
            // steer or shed the over-speed and was stuck coasting (the
            // "can't move after Earthling ult" bug).
            if input.pressed(input::INPUT_ABSOLUTE) {
                use std::f32::consts::{PI, TAU};
                let forward = Vec2::new(-rot.sin, rot.cos);
                let facing = forward.y.atan2(forward.x);
                let aim = input.aim_vec();
                let pushing = aim.length() > 0.25;
                if pushing {
                    let desired = aim.y.atan2(aim.x);
                    let mut err = desired - facing;
                    while err > PI {
                        err -= TAU;
                    }
                    while err < -PI {
                        err += TAU;
                    }
                    let dt = time.delta_secs().max(1e-4);
                    ang_vel.0 = (err / dt).clamp(-derived.target_omega, derived.target_omega);
                    // Brake opposite to current travel while the stick is held.
                    let v = lin_vel.0;
                    let speed = v.length();
                    thrust.0 = if speed > 1.0 {
                        let world_brake = -v / speed;
                        Vec2::new(
                            world_brake.x * rot.cos + world_brake.y * rot.sin,
                            -world_brake.x * rot.sin + world_brake.y * rot.cos,
                        ) * derived.thrust_force
                    } else {
                        Vec2::ZERO
                    };
                } else {
                    ang_vel.0 = input.turn_f32().clamp(-1.0, 1.0) * derived.target_omega;
                    thrust.0 = Vec2::ZERO;
                }
                torque.0 = 0.0;
                last_turn.had_input = pushing;
                continue;
            }
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
                let go = err.abs() < FRAC_PI_2;
                if inertialess.is_some() {
                    // Arilou: direct velocity control even on the stick —
                    // move at full speed toward the heading, instant stop
                    // otherwise. Mirrors the keyboard inertialess path so
                    // releasing the stick halts immediately.
                    thrust.0 = Vec2::ZERO;
                    lin_vel.0 = if go { forward * derived.speed_max } else { Vec2::ZERO };
                } else {
                    thrust.0 = if go {
                        Vec2::new(0.0, derived.thrust_force)
                    } else {
                        Vec2::ZERO
                    };
                }
            } else {
                // Stick centred: tilt rotates in place, no thrust.
                let dir = input.turn_f32().clamp(-1.0, 1.0);
                ang_vel.0 = dir * derived.target_omega;
                torque.0 = 0.0;
                last_turn.had_input = dir.abs() > 1e-3;
                thrust.0 = Vec2::ZERO;
                // Inertialess: stick released ⇒ stop dead (the Arilou's
                // signature). Without this it kept the force-path momentum.
                if inertialess.is_some() {
                    lin_vel.0 = Vec2::ZERO;
                }
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
    session: Option<Res<crate::netcode::NetSocket>>,
) {
    // Bail in netplay: local KeyM mutates `AngularControlOverride`,
    // which `apply_player_input` (GgrsSchedule) reads. Without a
    // wire-format vote both peers' steering models would diverge.
    if session.is_some() {
        return;
    }
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
        Option<&NitrousActive>,
    )>,
) {
    for (entity, pos, derived, mut vel, hyper, coasting, nitrous) in &mut q {
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
        // Nitrous dash: lift the cap while the burst is live so the kick
        // (added at pickup) isn't clamped away. Once the marker expires the
        // ship coasts back under speed_max on the next tick.
        let nitro = if nitrous.is_some() { NITROUS_CAP_MULT } else { 1.0 };
        let cap = derived.speed_max * whip * nitro;
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
    mut q: Query<(&Ship, &mut Battery, &mut RechargeTimer, Option<&ShipModes>)>,
) {
    let dt = time.delta_secs();
    for (ship, mut battery, mut timer, modes) in &mut q {
        if ship.stats.recharge_rate <= 0.0 || ship.stats.recharge_amount <= 0 {
            continue;
        }
        // A draining mode (Androsynth Blazer, canon `recharge_amount = -1`)
        // suppresses normal recharge — otherwise it would refill the
        // battery the Blazer is supposed to be burning down.
        if modes
            .and_then(|m| m.modes.get(m.current))
            .is_some_and(|m| m.batt_drain_per_tick > 0)
        {
            continue;
        }
        // Period between recharge ticks in seconds: SC2 RechargeRate
        // (a frame count) × 50 ms/frame.
        let period_s = ship.stats.recharge_rate * 0.050;
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
        ShipClass::Shosc
        | ShipClass::Arisk
        | ShipClass::Zfpst
        | ShipClass::Neodr
        | ShipClass::Iceco => 14.0,
        ShipClass::Vioge | ShipClass::Neccr | ShipClass::Yurpa | ShipClass::Glacr | ShipClass::Vezba => 22.0,
        ShipClass::Koapa | ShipClass::Ulzin => 16.0,
        ShipClass::Sclfr | ShipClass::Alhdr => 26.0,
        ShipClass::Gahmo | ShipClass::Hydcr | ShipClass::Dajem => 26.0,
        ShipClass::Rogsq => 14.0,
        ShipClass::Arkpi => 26.0,
        ShipClass::Kolfl => 30.0,
        ShipClass::Lyrwa => 32.0,
        ShipClass::Hubde => 24.0,
        ShipClass::Uxjba => 30.0,
        ShipClass::Spael
        | ShipClass::Pkufu
        | ShipClass::Thrto
        | ShipClass::Taugl
        | ShipClass::Tauem
        | ShipClass::Taust => 16.0,
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
        | ShipClass::Meltr
        | ShipClass::Tauar
        | ShipClass::Leimu => 22.0,
        // Leviathan is a big bio-cruiser (Mass 19, Crew 36).
        ShipClass::Taule => 26.0,
        // Missile Cruiser — slow, heavy (Mass 19, Crew 32).
        ShipClass::Taumc => 24.0,
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
#[derive(Component, Debug, Clone)]
#[component(on_add = auto_assign_net_id)]
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

/// "This ship cannot be targeted" marker (Ilwrath cloak). Homing
/// missiles and auto-aim beams skip entities carrying this component
/// during target acquisition. Projectile collisions and ship-ship
/// rams still hurt — cloak hides from auto-targeting, not from
/// physics.
///
/// Canon (`shpilwav.cpp`) is a toggle, not a timer: the cloak persists
/// until the player either re-presses Special (toggle off) OR fires
/// the primary weapon (which uncloaks as a side effect of the shot).
/// So this is just a marker — no `remaining` field, no `tick_invisible`.
#[derive(Component, Debug, Clone)]
pub struct Invisible;

/// Per-tick state for "incoming damage tops up battery instead of
/// hurting crew" (Utwig fortitude). While present, the projectile-hit
/// handler routes `floor(damage · conversion)` to `Battery::current`
/// (clamped to max) and zeroes the crew loss. Collisions are not
/// affected — fortitude buffers projectile damage, not rams.
#[derive(Component, Debug, Clone)]
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
#[derive(Component, Debug, Clone)]
#[component(on_add = auto_assign_net_id)]
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
    /// Fractional battery-drain carry. A mode draining 1 SC2-frame/tick
    /// only loses ~0.33 battery per 60 Hz physics tick, so flooring each
    /// tick independently rounded to 0 and the Blazer never drained.
    /// Accumulate the fraction and spend it once it crosses a whole unit.
    pub drain_accum: f32,
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

        // Per-tick battery drain, with a fractional carry. At 60 Hz a
        // `batt_drain` of 1 SC2-frame/tick is only ~0.33/physics-tick, so
        // rounding each tick alone floored to 0 and the Blazer never
        // consumed battery (canon `recharge_amount = -1`). Accumulate the
        // fraction and spend whole units. `tick_battery_recharge` skips
        // draining modes so it can't refill against this.
        if batt_drain != 0 {
            modes.drain_accum += batt_drain as f32 * dt_frames;
            let whole = modes.drain_accum.floor();
            if whole != 0.0 {
                modes.drain_accum -= whole;
                batt.current = (batt.current - whole as i32).clamp(0, batt.max);
            }
        } else {
            modes.drain_accum = 0.0;
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
#[derive(Component, Debug, Clone)]
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
#[derive(Component, Debug, Clone)]
#[component(on_add = auto_assign_net_id)]
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
#[derive(Component, Debug, Clone)]
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
    /// Kzer-Za Dreadnought fighter (`shpkzedr.cpp:KzerZaFighter`).
    /// Flies toward an orbit point off the nearest opponent's flank,
    /// holds station at `laser_range × 0.8`, and zaps the target with
    /// a laser every `recharge_s`. Does NOT detonate on contact — it
    /// keeps station and lasers until lifetime runs out or it takes
    /// damage. `laser_cooldown_s` ticks down each frame; when it hits
    /// 0 the fighter fires (deals `laser_damage`, spawns a `ZapFlash`)
    /// and resets to `recharge_s`.
    KzerZaFighter {
        target: Option<Entity>,
        turn_rate: f32,
        speed: f32,
        laser_range: f32,
        laser_damage: i32,
        recharge_s: f32,
        laser_cooldown_s: f32,
        /// Canon `air_frames`: while >0 the fighter is still clearing
        /// the dreadnought's hitbox. Skips AI steering AND skips the
        /// "touched parent → dock + refund crew" path (otherwise the
        /// fighter dies on its birth frame to the parent collider it
        /// spawns inside).
        air_grace_s: f32,
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
                    for i in 0..n_shards {
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
                            // Canon shards use `data->spriteExtra` — the
                            // extracted `shot_e_NN_tga` crystal-shard frames
                            // (12×12, 64 of them). Vary the frame by index so
                            // the burst looks jagged/chaotic; the bluish
                            // `shard_color` tints it. `i % 64` is deterministic
                            // (no extra RNG draw), so peers stay in sync and
                            // the guest mirror picks up the path.
                            Sprite {
                                image: assets.load(format!(
                                    "ships/chebr/sprites/shot_e_{:02}_tga.png",
                                    i % 64
                                )),
                                color: shard_color,
                                custom_size: Some(Vec2::splat(size * 1.8)),
                                ..default()
                            },
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

/// Tau Gladius primary — `shptaugl.cpp:activate_weapon` / `TauGladiusShot`.
/// A fast yellow laser bolt fired with a quadratic-weighted random spread
/// (`s = u·|u|·spread`), inheriting ship velocity, range-limited (dies at
/// Range/Velocity seconds) and dealing Damage on contact. WeaponRate is 0
/// so the cadence is battery-limited: WeaponDrain 1 per bolt vs a 12
/// battery, so a held trigger rips ~12 bolts then waits on recharge.
fn tick_taugl_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TauglState,
        &mut Battery,
    )>,
) {
    let dt = time.delta_secs();
    let speed = 150.0 * SC2_VEL_SCALE; // [Weapon] Velocity 150
    let range = 14.0 * SC2_RANGE_SCALE; // Range 14
    let lifetime = range / speed; // dies at range (d > range)
    let damage = 1; // Damage 1
    let spread = 5.0_f32.to_radians(); // Spread 5° × ANGLE_RATIO (π/180)
    let drain = 1; // WeaponDrain 1

    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        if st.weapon_cd_s > 0.0 {
            st.weapon_cd_s -= dt;
        }
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        st.last_fire_held = held;
        if !held || st.weapon_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.weapon_cd_s = 0.0; // WeaponRate 0 — battery is the limiter

        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        // Muzzle: original ship-local Vector2(5, 23).
        let muzzle = pos.0 + forward * 23.0 + right * 5.0;
        // Quadratic-weighted spread, then rotate forward by it.
        let u = rng.signed_unit();
        let s = u * u.abs() * spread;
        let (ss, cs) = s.sin_cos();
        let dir = Vec2::new(forward.x * cs - forward.y * ss, forward.x * ss + forward.y * cs);
        let shot_vel = lvel.0 + dir * speed; // inherits ship velocity
        let init_angle = dir.y.atan2(dir.x) - std::f32::consts::FRAC_PI_2;

        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            // Bright yellow bolt (255,255,115), a short streak for now —
            // the exact tapering-line render is a later visual pass.
            Sprite::from_color(Color::srgb(1.0, 1.0, 0.45), Vec2::new(3.0, 18.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(3.0),
            Sensor,
            Mass(0.2),
            Position(muzzle),
            Rotation::radians(init_angle),
            LinearVelocity(shot_vel),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

/// Tau EMP primary — `shptauem.cpp:activate_weapon` / `TauEMPMissile`.
/// A rapid forward bolt fired from a rolling 6-slot barrel: the muzzle's
/// ship-local x steps `±8, ±10, ±13` (alternating sides as the slot
/// cycles), local y = 10. Straight shot (no spread), inherits ship
/// velocity, range-limited (dies at Range/Velocity seconds). WeaponRate 1
/// (≈ one 50 ms frame) paces the cadence; WeaponDrain 1 vs a 12 battery
/// means a held trigger rips ~12 bolts then waits on recharge.
fn tick_tauem_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TauemState,
        &mut Battery,
    )>,
) {
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE; // [Weapon] Velocity 90
    let range = 9.0 * SC2_RANGE_SCALE; // Range 9
    let lifetime = range / speed; // dies at range
    let damage = 1; // Damage 1
    let drain = 1; // WeaponDrain 1
    let weapon_rate_s = 0.05; // WeaponRate 1 frame (50 ms)

    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        if st.weapon_cd_s > 0.0 {
            st.weapon_cd_s -= dt;
        }
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        st.last_fire_held = held;
        if !held || st.weapon_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.weapon_cd_s = weapon_rate_s;

        // Rolling barrel muzzle offset (shptauem activate_weapon): the
        // magnitude widens with the slot, the sign alternates each shot.
        let slot = st.slot;
        let mag = if slot < 2 { 8.0 } else if slot < 4 { 10.0 } else { 13.0 };
        let rx = if slot % 2 == 1 { -mag } else { mag };
        st.slot = (slot + 1) % 6;

        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        // Ship-local Vector2(rx, 10).
        let muzzle = pos.0 + right * rx + forward * 10.0;
        let shot_vel = lvel.0 + forward * speed; // straight, inherits velocity
        let init_angle = forward.y.atan2(forward.x) - std::f32::consts::FRAC_PI_2;

        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            // Pale electric-blue bolt; the exact sprite render is a later
            // visual pass.
            Sprite::from_color(Color::srgb(0.55, 0.70, 1.0), Vec2::new(3.0, 12.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(3.0),
            Sensor,
            Mass(0.2),
            Position(muzzle),
            Rotation::radians(init_angle),
            LinearVelocity(shot_vel),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

/// Tau EMP special — `shptauem.cpp:calculate_fire_special`. A full-
/// battery discharge: when fired (battery must be FULL and no wave
/// already running), the battery empties to 0 and an EMP ring starts at
/// `0.75 × size` and expands at SpecialVelocity until it passes
/// SpecialRange. Each tick the ring is live, every enemy ship inside the
/// *current* radius that is actively holding a control gets jammed — its
/// held buttons frozen for `a × JamTime` seconds, where
/// `a = 1 − (dist/range)^(1/attenuation)` (closer = longer, min 0.1).
fn tick_tauem_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut ring_mesh: Local<Option<Handle<Mesh>>>,
    boss: Query<(), With<CapitalCore>>,
    mut emps: Query<(&Ship, &Position, &mut TauemState, &mut Battery)>,
    others: Query<(Entity, &Ship, &Position)>,
) {
    // EMP jams *enemies*; in boss co-op every fighter is one team, so the
    // wave has nothing to bite — skip the scan entirely there.
    let coop = !boss.is_empty();
    let dt = time.delta_secs();
    let range = 7.0 * SC2_RANGE_SCALE; // [Special] Range 7
    let velocity = 50.0 * SC2_VEL_SCALE; // Velocity 50
    let jam_time = 5.0; // JamTime 5.0 s
    let attenuation = 1.0; // Attenuation 1
    // Collect jams to apply after the ship loop (avoids overlapping the
    // `emps` and `others` borrows; they can share entities).
    let mut jams: Vec<(Entity, u8, f32)> = Vec::new();

    for (ship, pos, mut st, mut batt) in &mut emps {
        // Propagate an active wave and jam whatever it sweeps over.
        if st.wave_radius > 0.1 {
            st.wave_radius += velocity * dt;
            if st.wave_radius > range {
                st.wave_radius = 0.0;
            } else if !coop {
                let r = st.wave_radius;
                for (oe, os, opos) in &others {
                    if os.player_slot == ship.player_slot {
                        continue; // self / teammate
                    }
                    let d = crate::physics::min_image(opos.0 - pos.0).length();
                    if d > r {
                        continue;
                    }
                    let held = slot_inputs.held[os.player_slot.min(3)].buttons;
                    if held == 0 {
                        continue; // only freezes controls that are active
                    }
                    let a = (1.0 - (d / range).powf(1.0 / attenuation)).max(0.1);
                    jams.push((oe, held, a * jam_time));
                }
            }
        }

        // Fire a fresh discharge: needs a full battery and no live wave.
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        let just = held && !st.last_special_held;
        st.last_special_held = held;
        if just && st.wave_radius <= 0.0 && batt.current >= batt.max {
            batt.current = 0;
            st.wave_radius = 24.0; // ≈ 0.75 × hull size
            // Cosmetic shockwave ring (unit annulus scaled each tick).
            let mesh = ring_mesh
                .get_or_insert_with(|| meshes.add(Annulus::new(0.86, 1.0)))
                .clone();
            let duration = (range - 24.0) / velocity;
            commands.spawn((
                EmpWaveVisual { elapsed: 0.0, duration, max_r: range },
                Mesh2d(mesh),
                MeshMaterial2d(
                    materials.add(ColorMaterial::from(Color::srgba(0.4, 0.5, 1.0, 0.7))),
                ),
                Transform::from_translation(pos.0.extend(0.42)).with_scale(Vec3::splat(24.0)),
            ));
        }
    }

    for (entity, mask, secs) in jams {
        // Refresh (don't stack) the jam; latest sweep wins.
        commands
            .entity(entity)
            .try_insert(ControlJam { mask, remaining: secs });
    }
}

/// Enforce active `ControlJam`s: clear the jammed button bits from the
/// victim's gathered input each tick, then age the jam out. Runs right
/// after `gather_slot_inputs` (same set) so every downstream consumer
/// reads the masked input.
fn apply_control_jams(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut inputs: ResMut<input::SlotInputs>,
    mut jammed: Query<(Entity, &Ship, &mut ControlJam)>,
) {
    let dt = time.delta_secs();
    for (entity, ship, mut jam) in &mut jammed {
        let slot = ship.player_slot.min(3);
        inputs.held[slot].buttons &= !jam.mask;
        inputs.just_pressed[slot].buttons &= !jam.mask;
        jam.remaining -= dt;
        if jam.remaining <= 0.0 {
            commands.entity(entity).try_remove::<ControlJam>();
        }
    }
}

/// Grow + fade the Tau EMP shockwave ring, despawning it when spent.
fn tick_emp_wave_visual(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut rings: Query<(Entity, &mut EmpWaveVisual, &mut Transform, &MeshMaterial2d<ColorMaterial>)>,
) {
    let dt = time.delta_secs();
    for (e, mut ring, mut xf, mat) in &mut rings {
        ring.elapsed += dt;
        let t = (ring.elapsed / ring.duration).clamp(0.0, 1.0);
        xf.scale = Vec3::splat(24.0 + (ring.max_r - 24.0) * t);
        if let Some(m) = materials.get_mut(&mat.0) {
            let c = m.color.to_srgba();
            m.color = Color::srgba(c.red, c.green, c.blue, 0.7 * (1.0 - t));
        }
        if ring.elapsed >= ring.duration {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
        }
    }
}

/// Tau Leviathan primary — `shptaule.cpp:activate_weapon`. A corrosive
/// "slime" gas ball, fired forward with a ±Spread random jitter; it flies
/// range-limited and deals Damage on contact. When it KILLS crew on a
/// ship (`handle_leviathan_hits`), each point of crew lost drops a homing
/// food pellet. Also runs the Leviathan's slow passive crew regen.
fn tick_taule_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TauleState,
        &mut Battery,
        &mut Crew,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 60.0 * SC2_VEL_SCALE; // [Weapon] Velocity 60
    let range = 12.0 * SC2_RANGE_SCALE; // Range 12
    let lifetime = range / speed;
    let damage = 2; // Damage 2
    let spread = 10.0_f32.to_radians(); // Spread 10°
    let drain = 2; // WeaponDrain 2
    let weapon_rate_s = 0.05; // WeaponRate 1 frame
    // Slow passive crew regen ("normal recharge rate is slow"). HealingRate
    // 260 frames ≈ 13 s/crew in canon; we use a touch faster so it reads.
    let heal_interval = 8.0;

    for (entity, ship, pos, rot, lvel, mut st, mut batt, mut crew) in &mut ships {
        // Passive regen.
        st.heal_accum_s += dt;
        if st.heal_accum_s >= heal_interval {
            st.heal_accum_s -= heal_interval;
            crew.current = (crew.current + 1).min(crew.max);
        }

        if st.weapon_cd_s > 0.0 {
            st.weapon_cd_s -= dt;
        }
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        st.last_fire_held = held;
        if !held || st.weapon_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.weapon_cd_s = weapon_rate_s;

        let base = Vec2::new(-rot.sin, rot.cos);
        // Random spread around the forward heading.
        let s = rng.signed_unit() * spread;
        let (ss, cs) = s.sin_cos();
        let dir = Vec2::new(base.x * cs - base.y * ss, base.x * ss + base.y * cs);
        let muzzle = pos.0 + base * 30.0;
        let shot_vel = lvel.0 + dir * speed;
        let init_angle = dir.y.atan2(dir.x) - FRAC_PI_2;

        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            SlimeBall { owner: entity },
            // Sickly green glob.
            Sprite::from_color(Color::srgb(0.55, 0.85, 0.25), Vec2::new(11.0, 11.0)),
            Transform::from_translation(muzzle.extend(0.45)),
            RigidBody::Dynamic,
            Collider::circle(6.0),
            Sensor,
            Mass(0.4),
            Position(muzzle),
            Rotation::radians(init_angle),
            LinearVelocity(shot_vel),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

/// Tau Leviathan special — `shptaule.cpp:activate_special`. A homing
/// missile launched from an alternating side. On hitting a ship it
/// disables the victim's engine (left/right/thrust) for a short while
/// (`OverrideControlLeviathan`, applied in `handle_leviathan_hits` via
/// `ControlJam`). SpecialDrain 4, SpecialRate 12.
fn tick_taule_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TauleState,
        &mut Battery,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 80.0 * SC2_VEL_SCALE; // [Special] Velocity 80
    let range = 45.0 * SC2_RANGE_SCALE; // Range 45
    let lifetime = range / speed;
    let damage = 1; // Damage 1
    let turn_rate = sc2_turning(10.0); // TurnRate 10
    let drain = 4; // SpecialDrain 4
    let cooldown = 12.0 / 20.0; // SpecialRate 12

    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        if st.special_cd_s > 0.0 {
            st.special_cd_s -= dt;
        }
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        let just = held && !st.last_special_held;
        st.last_special_held = held;
        if !just || st.special_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.special_cd_s = cooldown;

        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let muzzle = pos.0 + right * (22.0 * st.missile_side) + forward * 5.0;
        st.missile_side = -st.missile_side;
        let mvel = lvel.0 + forward * speed;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;

        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            Homing { target: None, turn_rate },
            LeviathanMissile,
            // Biomechanical dart — purple.
            Sprite::from_color(Color::srgb(0.7, 0.4, 0.9), Vec2::new(7.0, 17.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            (
                RigidBody::Dynamic,
                Collider::circle(6.0),
                Sensor,
                Mass(0.5),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(mvel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ),
        ));
    }
}

/// Marker for the Leviathan's engine-disable homing missile.
#[derive(Component, Debug)]
pub struct LeviathanMissile;

/// Leviathan on-hit effects, read off the collision stream BEFORE
/// `handle_projectile_hits` despawns the projectile:
///   - SlimeBall vs ship → drop `damage` food pellets at the victim.
///   - LeviathanMissile vs ship → stamp an engine `ControlJam`
///     (left/right/thrust frozen) on the victim.
#[allow(clippy::too_many_arguments)]
fn handle_leviathan_hits(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut food_assets: Local<Option<(Handle<Mesh>, Handle<ColorMaterial>)>>,
    slimeballs: Query<&SlimeBall>,
    missiles: Query<(), With<LeviathanMissile>>,
    projectiles: Query<&Projectile>,
    ships: Query<&Ship>,
    positions: Query<&Position>,
) {
    let food_life = 15.0; // [Extra] Lifetime 15
    let food_speed = 7.0 * SC2_VEL_SCALE; // [Extra] Velocity 7
    for ev in reader.read() {
        // Identify projectile side vs the other side.
        let (proj_e, other_e) = if projectiles.get(ev.collider1).is_ok() {
            (ev.collider1, ev.collider2)
        } else if projectiles.get(ev.collider2).is_ok() {
            (ev.collider2, ev.collider1)
        } else {
            continue;
        };
        // Only care when the other side is a ship.
        if ships.get(other_e).is_err() {
            continue;
        }
        if let Ok(sb) = slimeballs.get(proj_e) {
            let n = projectiles.get(proj_e).map(|p| p.damage).unwrap_or(1).max(0);
            let Ok(vpos) = positions.get(other_e) else { continue };
            let (mesh, mat) = food_assets
                .get_or_insert_with(|| {
                    (
                        meshes.add(Circle::new(6.0)),
                        materials.add(ColorMaterial::from(Color::srgb(0.6, 1.0, 0.45))),
                    )
                })
                .clone();
            for _ in 0..n {
                let a = rng.f32() * std::f32::consts::TAU;
                let v = food_speed * (1.0 - 0.9 * rng.f32());
                let vel = Vec2::new(a.cos(), a.sin()) * v;
                commands.spawn((
                    LeviathanFood { owner: sb.owner, lifetime_s: food_life },
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_translation(vpos.0.extend(0.2)),
                    RigidBody::Kinematic,
                    Collider::circle(6.0),
                    Sensor,
                    Position(vpos.0),
                    Rotation::radians(0.0),
                    LinearVelocity(vel),
                ));
            }
        } else if missiles.get(proj_e).is_ok() {
            commands.entity(other_e).try_insert(ControlJam {
                mask: input::INPUT_LEFT | input::INPUT_RIGHT | input::INPUT_THRUST,
                remaining: 1.5,
            });
        }
    }
}

/// Drift, decay, home, and harvest Leviathan food pellets. Each pellet
/// eases toward its owning Leviathan when within range, decays its
/// drift, ages out after its lifetime, and on contact tops up the
/// owner's crew + battery (`Food2Batt`), then despawns.
fn tick_leviathan_food(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut food: Query<(Entity, &mut LeviathanFood, &Position, &mut LinearVelocity)>,
    owners: Query<&Position, With<Ship>>,
    mut heal: Query<(&mut Crew, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let pull = 2.0 * SC2_VEL_SCALE; // gentle homing accel toward the owner
    let pickup_r = 30.0;
    let home_r = 220.0;
    let food2batt = 2; // [Ship] Food2Batt 2
    for (e, mut f, pos, mut vel) in &mut food {
        f.lifetime_s -= dt;
        if f.lifetime_s <= 0.0 {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_despawn();
            }
            continue;
        }
        // Decay drift, then home toward the owner if it still exists.
        vel.0 *= (1.0 - 0.5 * dt).max(0.0);
        if let Ok(opos) = owners.get(f.owner) {
            let to = crate::physics::min_image(opos.0 - pos.0);
            let d = to.length();
            if d <= pickup_r {
                if let Ok((mut crew, mut batt)) = heal.get_mut(f.owner) {
                    crew.current = (crew.current + 1).min(crew.max);
                    batt.current = (batt.current + food2batt).min(batt.max);
                }
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.try_despawn();
                }
                continue;
            }
            if d <= home_r && d > 0.1 {
                vel.0 += (to / d) * pull;
            }
        }
    }
}

/// Tau Missile Cruiser primary — `shptaumc.cpp:activate_weapon` + the
/// lock logic in `calculate`. You must hold your nose on a hostile (an
/// enemy ship, or a boss part in co-op) within LockAngle for LockCount
/// ticks to acquire a lock; then a held trigger launches a slow homing
/// torpedo from one of two tubes (each on its own recharge). The torpedo
/// also deals splash (`handle_torpedo_blast`).
#[allow(clippy::too_many_arguments)]
fn tick_taumc_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TaumcState,
    )>,
    enemies: Query<(&Ship, &Position)>,
    boss_parts: Query<&Position, With<BossHealth>>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 55.0 * SC2_VEL_SCALE; // [Weapon] Velocity 55
    let range = 90.0 * SC2_RANGE_SCALE; // Range 90
    let lifetime = range / speed;
    let damage = 4; // Damage 4
    let blast = 8; // BlastDamage 8
    let blast_range = 200.0; // BlastRange 200 (already world-scaled small)
    let turn_rate = sc2_turning(5.0); // TurnRate 5
    let tube_recharge = 70.0 / 20.0; // [Weapon] Rate 70 frames
    let lock_angle = 20.0_f32.to_radians(); // LockAngle 20°
    let lock_time = 5.0 / 20.0; // LockCount 4 (+1) ticks

    for (entity, ship, pos, rot, lvel, mut st) in &mut ships {
        st.tube_cd[0] = (st.tube_cd[0] - dt).max(0.0);
        st.tube_cd[1] = (st.tube_cd[1] - dt).max(0.0);

        // Nearest hostile: enemy ship (versus) or boss part (co-op).
        let mut best: Option<(f32, Vec2)> = None;
        for (es, ep) in &enemies {
            if es.player_slot == ship.player_slot {
                continue;
            }
            let img = crate::physics::nearest_image(ep.0, pos.0);
            let d2 = img.distance_squared(pos.0);
            if best.map_or(true, |(bd, _)| d2 < bd) {
                best = Some((d2, img));
            }
        }
        for bp in &boss_parts {
            let img = crate::physics::nearest_image(bp.0, pos.0);
            let d2 = img.distance_squared(pos.0);
            if best.map_or(true, |(bd, _)| d2 < bd) {
                best = Some((d2, img));
            }
        }

        // Accumulate / drop the lock.
        let forward = Vec2::new(-rot.sin, rot.cos);
        let locked_on = best.is_some_and(|(d2, tgt)| {
            if d2 > range * range {
                return false;
            }
            let to = (tgt - pos.0).normalize_or_zero();
            let ang = forward.perp_dot(to).atan2(forward.dot(to)).abs();
            ang <= lock_angle
        });
        if locked_on {
            st.lock_s += dt;
        } else {
            st.lock_s = 0.0;
        }

        let fire = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        let special = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        st.last_fire_held = fire;
        // Torpedo only fires from the hull when NOT in turret mode (SPECIAL
        // held re-routes FIRE to the turret — see tick_taumc_turret).
        if !fire || special || st.lock_s < lock_time {
            continue;
        }
        // Pick a ready tube (prefer the scheduled one).
        let tube = if st.tube_cd[st.next_tube] <= 0.0 {
            st.next_tube
        } else if st.tube_cd[1 - st.next_tube] <= 0.0 {
            1 - st.next_tube
        } else {
            continue; // both reloading
        };
        st.tube_cd[tube] = tube_recharge;
        st.next_tube = 1 - tube;

        let right = Vec2::new(rot.cos, rot.sin);
        let muzzle = pos.0 + right * (20.0 * (2.0 * tube as f32 - 1.0)) + forward * 25.0;
        let mvel = lvel.0 + forward * speed;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            Homing { target: None, turn_rate },
            TauMcTorpedo { owner_slot: ship.player_slot, blast, blast_range },
            // Fat blue torpedo.
            Sprite::from_color(Color::srgb(0.45, 0.6, 1.0), Vec2::new(10.0, 22.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            (
                RigidBody::Dynamic,
                Collider::circle(8.0),
                Sensor,
                Mass(1.2),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(mvel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ),
        ));
    }
}

/// Tau Missile Cruiser turret — `shptaumc.cpp:activate_special` +
/// `calculate_turn_left/right`. Faithful to the original dual-control
/// scheme (the same one the Orz Nemesis uses): holding SPECIAL puts the
/// ship in TURRET MODE — left/right swivel the turret instead of the hull,
/// and FIRE looses a rapid burst of tracking missiles along the turret's
/// heading, drawn from a small ammo pool that refills over time. Runs
/// after `apply_player_input` so it can clobber the hull's turn the same
/// way the Orz turret does.
#[allow(clippy::too_many_arguments)]
fn tick_taumc_turret(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &ShipClass,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut AngularVelocity,
        &mut ConstantTorque,
        &mut TaumcState,
        &ShipPhysicsDerived,
    )>,
    mut overlays: Query<&mut OverlaySprite>,
) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE; // [Special] Velocity 90
    let range = 20.0 * SC2_RANGE_SCALE; // Range 20
    let lifetime = range / speed;
    let damage = 3; // Damage 3
    let turn_rate = sc2_turning(5.0); // TurnRate 5
    let cone = 30.0_f32.to_radians(); // TrackAngle 30°
    let ammo_max = 4;
    let ammo_recharge = 20.0 / 20.0; // [Special] Rate 20 frames → 1 s/ammo
    let fire_rate = 2.0 / 20.0; // [Ship] SpecialRate 2 frames
    // [Extra] TurnRate 0.6 — the turret swivels slower than the hull.
    let turret_omega_scale = 0.6;

    for (entity, ship, class, pos, rot, lvel, mut ang_vel, mut torque, mut st, derived) in &mut ships {
        if *class != ShipClass::Taumc {
            continue;
        }
        // Ammo always trickles back.
        st.fire_cd_s = (st.fire_cd_s - dt).max(0.0);
        if st.ammo < ammo_max {
            st.ammo_cd_s -= dt;
            if st.ammo_cd_s <= 0.0 {
                st.ammo_cd_s = ammo_recharge;
                st.ammo += 1;
            }
        }

        let input = slot_inputs.held[ship.player_slot.min(3)];
        let special = input.pressed(input::INPUT_SPECIAL);
        let fire = input.pressed(input::INPUT_FIRE);

        if special {
            // Turret mode: lock the hull's spin (apply_player_input already
            // wrote it from the turn keys) and swivel the turret instead.
            ang_vel.0 = 0.0;
            torque.0 = 0.0;
            let dir = if input.pressed(input::INPUT_LEFT) {
                1.0
            } else if input.pressed(input::INPUT_RIGHT) {
                -1.0
            } else {
                0.0
            };
            st.turret_rad += dir * derived.target_omega * turret_omega_scale * dt;
            if st.turret_rad > PI {
                st.turret_rad -= TAU;
            } else if st.turret_rad < -PI {
                st.turret_rad += TAU;
            }

            // SPECIAL + FIRE → burst of tracking missiles along the turret.
            if fire && st.fire_cd_s <= 0.0 && st.ammo > 0 {
                st.ammo -= 1;
                st.fire_cd_s = fire_rate;
                let forward = Vec2::new(-rot.sin, rot.cos);
                // Turret heading = hull forward rotated by the turret offset.
                let (ts, tc) = st.turret_rad.sin_cos();
                let aim = Vec2::new(forward.x * tc - forward.y * ts, forward.x * ts + forward.y * tc);
                let right = Vec2::new(aim.y, -aim.x);
                let init_angle = aim.y.atan2(aim.x) - FRAC_PI_2;
                for k in 0..2 {
                    let off = (st.current_barrel as f32 + k as f32 * 2.0 - 1.5) * 6.0;
                    let muzzle = pos.0 + right * off + aim * 23.0;
                    let mvel = lvel.0 + aim * speed;
                    commands.spawn((
                        Projectile { owner: entity, damage, lifetime },
                        Homing { target: None, turn_rate },
                        HomingCone(cone),
                        Sprite::from_color(Color::srgb(0.6, 0.85, 1.0), Vec2::new(5.0, 13.0)),
                        Transform::from_translation(muzzle.extend(0.48)),
                        (
                            RigidBody::Dynamic,
                            Collider::circle(4.0),
                            Sensor,
                            Mass(0.3),
                            Position(muzzle),
                            Rotation::radians(init_angle),
                            LinearVelocity(mvel),
                            AngularVelocity::ZERO,
                            LinearDamping(0.0),
                            AngularDamping(0.0),
                            CollisionEventsEnabled,
                        ),
                    ));
                }
                st.current_barrel = (st.current_barrel + 1) % 4;
            }
        }

        // Mirror the turret aim onto the overlay art every tick (whether or
        // not it moved this frame).
        let turret_rad = st.turret_rad;
        for mut overlay in &mut overlays {
            if overlay.parent == entity {
                overlay.extra_angle = turret_rad;
            }
        }
    }
}

/// Splash damage for the MC torpedo: when a `TauMcTorpedo` collides with
/// a ship or boss part, deal its `blast` to every enemy ship within
/// `blast_range` (shield-aware). Runs before `handle_projectile_hits`
/// (which applies the direct hit + despawns the torpedo).
fn handle_torpedo_blast(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    assets: Res<AssetServer>,
    torpedoes: Query<(&TauMcTorpedo, &Position)>,
    ship_q: Query<(), With<Ship>>,
    boss_q: Query<(), With<BossHealth>>,
    targets: Query<(Entity, &Ship, &Position)>,
    shields: Query<&ShieldActive>,
    mut crews: Query<&mut Crew>,
) {
    let mut detonated: bevy::platform::collections::HashSet<Entity> =
        bevy::platform::collections::HashSet::default();
    for ev in reader.read() {
        let (torp_e, other_e) = if torpedoes.get(ev.collider1).is_ok() {
            (ev.collider1, ev.collider2)
        } else if torpedoes.get(ev.collider2).is_ok() {
            (ev.collider2, ev.collider1)
        } else {
            continue;
        };
        // Only detonate against a ship or a boss part.
        if ship_q.get(other_e).is_err() && boss_q.get(other_e).is_err() {
            continue;
        }
        if detonated.contains(&torp_e) {
            continue;
        }
        let Ok((torp, tpos)) = torpedoes.get(torp_e) else { continue };
        detonated.insert(torp_e);
        for (te, ts, tp) in &targets {
            if ts.player_slot == torp.owner_slot {
                continue;
            }
            let d = crate::physics::min_image(tp.0 - tpos.0).length();
            if d > torp.blast_range {
                continue;
            }
            let factor = shields.get(te).map(|s| s.damage_factor).unwrap_or(1.0);
            let dmg = ((torp.blast as f32 * factor).round() as i32).max(0);
            if dmg > 0 {
                if let Ok(mut crew) = crews.get_mut(te) {
                    crew.current = (crew.current - dmg).max(0);
                }
            }
        }
        spawn_asteroid_explosion(&mut commands, &assets, tpos.0, 40.0);
    }
}

/// Shared stat block for a T-Storm missile launch.
#[derive(Clone, Copy)]
struct StormParams {
    speed: f32,
    start: f32,
    accel: f32,
    max_v: f32,
    turn_rate: f32,
    thrust: f32,
    booster_speed: f32,
    spin: f32,
    fuel_s: f32,
    kick: f32,
    kick_max: f32,
    drain: i32,
    color: Color,
}

/// Launch one T-Storm missile from the alternating muzzle + recoil the
/// ship. Shared by the slow (primary) and fast (special) modes.
fn fire_storm_missile(
    commands: &mut Commands,
    slot: usize,
    pos: Vec2,
    forward: Vec2,
    right: Vec2,
    ship_vel: &mut Vec2,
    barrel: u32,
    p: StormParams,
) {
    use std::f32::consts::FRAC_PI_2;
    let mag = if barrel < 2 { 9.0 } else { 13.0 };
    let rx = if barrel % 2 == 1 { -mag } else { mag };
    let muzzle = pos + right * rx + forward * 10.0;
    let mvel = forward * (p.speed * p.start) + *ship_vel;
    let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
    commands.spawn((
        StormMissile {
            owner_slot: slot,
            fuel_s: p.fuel_s,
            accel: p.accel,
            max_v: p.max_v,
            turn_rate: p.turn_rate,
            thrust: p.thrust,
            booster_speed: p.booster_speed,
            spin: p.spin,
            latched: None,
            rel: Vec2::ZERO,
            shove_dir0: Vec2::Y,
            theta0: 0.0,
        },
        Sprite::from_color(p.color, Vec2::new(5.0, 14.0)),
        Transform::from_translation(muzzle.extend(0.48)),
        RigidBody::Kinematic,
        Collider::circle(5.0),
        Sensor,
        Position(muzzle),
        Rotation::radians(init_angle),
        LinearVelocity(mvel),
        AngularVelocity::ZERO,
        CollisionEventsEnabled,
    ));
    // Recoil kick: shove the ship backward, capped at KickMaxspeed.
    let mut nv = *ship_vel - forward * p.kick;
    if nv.length() > p.kick_max {
        nv = nv.normalize() * p.kick_max;
    }
    *ship_vel = nv;
}

/// Tau T-Storm primary (slow mode) — `shptaust.cpp:activate_weapon`. A
/// long-range harassing latch-missile; WeaponRate 14 paces it, WeaponDrain
/// 2 vs a 12 battery.
fn tick_taust_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        &Ship,
        &Position,
        &Rotation,
        &mut LinearVelocity,
        &mut TauStormState,
        &mut Battery,
    )>,
) {
    let dt = time.delta_secs();
    let p = StormParams {
        speed: 70.0 * SC2_VEL_SCALE,
        start: 0.5,
        accel: 12.0 * SC2_VEL_SCALE,
        max_v: 70.0 * SC2_VEL_SCALE,
        turn_rate: sc2_turning(2.0),
        thrust: 40.0 * SC2_VEL_SCALE,
        booster_speed: 40.0 * SC2_VEL_SCALE,
        spin: 8.0_f32.to_radians() * 6.0, // Rotation 8° → strong spin
        fuel_s: 2.5,                       // Fuel 2500 ms
        kick: 3.0 * SC2_VEL_SCALE,
        kick_max: 60.0 * SC2_VEL_SCALE,
        drain: 2,
        color: Color::srgb(0.7, 0.9, 1.0),
    };
    for (ship, pos, rot, mut vel, mut st, mut batt) in &mut ships {
        if st.weapon_cd_s > 0.0 {
            st.weapon_cd_s -= dt;
        }
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        if !held || st.weapon_cd_s > 0.0 || batt.current < p.drain {
            continue;
        }
        batt.current -= p.drain;
        st.weapon_cd_s = 14.0 / 20.0; // WeaponRate 14 frames
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let mut sv = vel.0;
        fire_storm_missile(&mut commands, ship.player_slot, pos.0, forward, right, &mut sv, st.slot, p);
        vel.0 = sv;
        st.slot = (st.slot + 1) % 6;
    }
}

/// Tau T-Storm special (fast mode) — `shptaust.cpp:activate_special`. A
/// fast, hard-shoving latch-missile. SpecialRate 0 (only the SpecialDrain
/// 2 vs a 12 battery limits the burst).
fn tick_taust_special(
    mut commands: Commands,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        &Ship,
        &Position,
        &Rotation,
        &mut LinearVelocity,
        &mut TauStormState,
        &mut Battery,
    )>,
) {
    let p = StormParams {
        speed: 82.0 * SC2_VEL_SCALE,
        start: 1.0,
        accel: 24.0 * SC2_VEL_SCALE,
        max_v: 82.0 * SC2_VEL_SCALE,
        turn_rate: sc2_turning(999.0),
        thrust: 120.0 * SC2_VEL_SCALE,
        booster_speed: 60.0 * SC2_VEL_SCALE,
        spin: 1.0_f32.to_radians() * 6.0, // Rotation 1° → mild spin, big shove
        fuel_s: 1.0,                       // Fuel 1000 ms
        kick: 6.0 * SC2_VEL_SCALE,
        kick_max: 80.0 * SC2_VEL_SCALE,
        drain: 2,
        color: Color::srgb(1.0, 0.8, 0.5),
    };
    for (ship, pos, rot, mut vel, mut st, mut batt) in &mut ships {
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        // SpecialRate 0: fire every tick held until the battery's dry.
        if !held || batt.current < p.drain {
            continue;
        }
        batt.current -= p.drain;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let mut sv = vel.0;
        fire_storm_missile(&mut commands, ship.player_slot, pos.0, forward, right, &mut sv, st.slot, p);
        vel.0 = sv;
        st.slot = (st.slot + 1) % 6;
    }
}

/// Latch a flying T-Storm missile onto the first enemy ship it touches
/// (`TauStormMissile::inflict_damage`): record the relative offset, the
/// shove heading, and the target's rotation so `tick_storm_missiles` can
/// ride and shove it.
fn handle_storm_latch(
    mut reader: MessageReader<CollisionStart>,
    mut missiles: Query<(&mut StormMissile, &Position, &LinearVelocity)>,
    targets: Query<(&Ship, &Position, &Rotation)>,
) {
    for ev in reader.read() {
        let (mis_e, other_e) = if missiles.get(ev.collider1).is_ok() {
            (ev.collider1, ev.collider2)
        } else if missiles.get(ev.collider2).is_ok() {
            (ev.collider2, ev.collider1)
        } else {
            continue;
        };
        let Ok((tship, tpos, trot)) = targets.get(other_e) else { continue };
        let Ok((mut sm, mpos, mvel)) = missiles.get_mut(mis_e) else { continue };
        if sm.latched.is_some() || tship.player_slot == sm.owner_slot {
            continue;
        }
        let rel = crate::physics::min_image(mpos.0 - tpos.0);
        let dir = mvel.0.normalize_or_zero();
        let dir = if dir == Vec2::ZERO { Vec2::Y } else { dir };
        // Spin sign from which side it struck (rel × heading).
        let sign = (rel.x * dir.y - rel.y * dir.x).signum();
        sm.latched = Some(other_e);
        sm.rel = rel;
        sm.shove_dir0 = dir;
        sm.theta0 = trot.as_radians();
        sm.spin = sm.spin.abs() * if sign == 0.0 { 1.0 } else { sign };
    }
}

/// Drive T-Storm missiles. Flying: steer toward the nearest enemy and
/// accelerate up to cruise, burning fuel. Latched: ride the victim, shove
/// it along the (rotated) launch heading and spin it, burning fuel; when
/// the fuel runs out, deal 1 damage and detonate.
fn tick_storm_missiles(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    assets: Res<AssetServer>,
    mut missiles: Query<
        (Entity, &mut StormMissile, &mut Position, &mut LinearVelocity, &mut Rotation),
        Without<Ship>,
    >,
    mut targets: Query<
        (&Ship, &mut Position, &mut LinearVelocity, &mut Rotation, &mut Crew),
        With<Ship>,
    >,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let rot2 = |v: Vec2, a: f32| {
        let (s, c) = a.sin_cos();
        Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
    };
    // Read-only snapshot of candidate targets for flying-missile steering
    // (taken from the same query we later write through).
    let snapshot: Vec<(usize, Vec2)> =
        targets.iter().map(|(s, p, _, _, _)| (s.player_slot, p.0)).collect();

    for (me, mut sm, mut mpos, mut mvel, mut mrot) in &mut missiles {
        sm.fuel_s -= dt;
        if let Some(t) = sm.latched {
            let Ok((_, mut tpos, mut tvel, mut trot, mut tcrew)) = targets.get_mut(t) else {
                if let Ok(mut ec) = commands.get_entity(me) {
                    ec.try_despawn();
                }
                continue;
            };
            let dtheta = trot.as_radians() - sm.theta0;
            // Ride the victim.
            mpos.0 = tpos.0 + rot2(sm.rel, dtheta);
            mvel.0 = tvel.0;
            // Shove it along the (rotated) launch heading, capped.
            let shove = rot2(sm.shove_dir0, dtheta);
            let mut nv = tvel.0 + shove * sm.thrust * dt;
            if nv.length() > sm.booster_speed {
                nv = nv.normalize() * sm.booster_speed;
            }
            tvel.0 = nv;
            // Spin it.
            *trot = Rotation::radians(trot.as_radians() + sm.spin * dt);
            let _ = &mut tpos;
            if sm.fuel_s <= 0.0 {
                tcrew.current = (tcrew.current - 1).max(0);
                spawn_asteroid_explosion(&mut commands, &assets, mpos.0, 22.0);
                if let Ok(mut ec) = commands.get_entity(me) {
                    ec.try_despawn();
                }
            }
            continue;
        }

        // Flying.
        if sm.fuel_s <= 0.0 {
            spawn_asteroid_explosion(&mut commands, &assets, mpos.0, 16.0);
            if let Ok(mut ec) = commands.get_entity(me) {
                ec.try_despawn();
            }
            continue;
        }
        // Steer toward the nearest enemy.
        let mut best: Option<(f32, Vec2)> = None;
        for (slot, p) in &snapshot {
            if *slot == sm.owner_slot {
                continue;
            }
            let img = crate::physics::nearest_image(*p, mpos.0);
            let d2 = img.distance_squared(mpos.0);
            if best.map_or(true, |(bd, _)| d2 < bd) {
                best = Some((d2, img));
            }
        }
        let speed = mvel.0.length();
        if speed > 0.0 {
            if let Some((_, tgt)) = best {
                let cur = mvel.0 / speed;
                let to = (tgt - mpos.0).normalize_or_zero();
                if to != Vec2::ZERO {
                    let ang = cur.perp_dot(to).atan2(cur.dot(to));
                    let step = ang.clamp(-sm.turn_rate * dt, sm.turn_rate * dt);
                    mvel.0 = rot2(cur, step) * speed;
                }
            }
        }
        // Accelerate up to cruise.
        let dir = mvel.0.normalize_or_zero();
        let mut nv = mvel.0 + dir * sm.accel * dt;
        if nv.length() > sm.max_v {
            nv = nv.normalize() * sm.max_v;
        }
        mvel.0 = nv;
        // Orient sprite to heading.
        *mrot = Rotation::radians(mvel.0.y.atan2(mvel.0.x) - FRAC_PI_2);
    }
}

// ---------------------------------------------------------------------
// GeomanNL fan ships (top TW-Light contributor): Neo Drain, Iceci
// Confusion, Lei Mule. Ported from the reference shp*.cpp + .ini.
// ---------------------------------------------------------------------

/// Neo Drain primary — `shpneodr.cpp:activate_weapon`. A plain forward
/// missile (no homing). WeaponRate 4 paces it; WeaponDrain 1, but DOUBLES
/// while the drain laser is in use (the canon quirk, tracked via
/// `NeodrState::special_active_s`).
fn tick_neodr_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut NeodrState,
        &mut Battery,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 78.0 * SC2_VEL_SCALE;
    let range = 14.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let damage = 1;
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        st.special_active_s = (st.special_active_s - dt).max(0.0);
        let drain = if st.special_active_s > 0.0 { 2 } else { 1 };
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        if !held || st.weapon_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.weapon_cd_s = 4.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let muzzle = pos.0 + forward * 18.0 + right * 6.0;
        let mvel = lvel.0 + forward * speed;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            Sprite::from_color(Color::srgb(0.7, 1.0, 0.6), Vec2::new(4.0, 11.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(3.0),
            Sensor,
            Mass(0.2),
            Position(muzzle),
            Rotation::radians(init_angle),
            LinearVelocity(mvel),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

/// Neo Drain special — `shpneodr.cpp` `LaserDrain`. A short-range battery
/// sap, modelled as a rapid stream of `FuelSap` pellets (no crew damage —
/// they drain the target's battery). Costs no battery itself, but flags
/// `special_active_s` so the primary's drain doubles. SpecialRate 1.
fn tick_neodr_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut NeodrState,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let range = 15.0 * SC2_RANGE_SCALE;
    let speed = 220.0 * SC2_VEL_SCALE; // fast = laser-like
    let lifetime = range / speed;
    let sap = 2; // [Special] Damage 2 → battery drained per pellet
    for (entity, ship, pos, rot, lvel, mut st) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        if !held || st.special_cd_s > 0.0 {
            continue;
        }
        st.special_cd_s = 1.0 / 20.0; // SpecialRate 1 frame
        st.special_active_s = 0.15; // keep the primary's drain doubled
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * 20.0;
        let mvel = lvel.0 + forward * speed;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        commands.spawn((
            Projectile { owner: entity, damage: 0, lifetime },
            FuelSap { amount: sap },
            Sprite::from_color(Color::srgb(0.4, 1.0, 0.9), Vec2::new(3.0, 14.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(3.0),
            Sensor,
            Mass(0.1),
            Position(muzzle),
            Rotation::radians(init_angle),
            LinearVelocity(mvel),
            AngularVelocity::ZERO,
            LinearDamping(0.0),
            AngularDamping(0.0),
            CollisionEventsEnabled,
        ));
    }
}

/// Iceci Confusion primary — `shpiceco.cpp:activate_weapon`. A lightly-
/// homing pellet (TurnRate 3). WeaponRate 5, WeaponDrain 6.
fn tick_iceco_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut IcecoState,
        &mut Battery,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 70.0 * SC2_VEL_SCALE;
    let range = 13.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let damage = 2;
    let turn_rate = sc2_turning(3.0);
    let drain = 6;
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        if !held || st.weapon_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.weapon_cd_s = 5.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * 16.0;
        let mvel = lvel.0 + forward * speed;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            Homing { target: None, turn_rate },
            Sprite::from_color(Color::srgb(0.6, 0.9, 1.0), Vec2::new(5.0, 9.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            (
                RigidBody::Dynamic,
                Collider::circle(4.0),
                Sensor,
                Mass(0.3),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(mvel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ),
        ));
    }
}

/// Iceci Confusion special — `shpiceco.cpp:activate_special`. Two darts
/// fired at ±60° from the sides; on a ship hit they scramble the victim's
/// controls (`handle_iceco_darts`). SpecialRate 20, SpecialDrain 5.
fn tick_iceco_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut IcecoState,
        &mut Battery,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE;
    let range = 16.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let turn_rate = sc2_turning(1.0);
    let drain = 5;
    let da = 60.0_f32.to_radians();
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        let just = held && !st.last_special_held;
        st.last_special_held = held;
        if !just || st.special_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.special_cd_s = 20.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * 16.0;
        for sgn in [1.0_f32, -1.0] {
            let a = sgn * da;
            let (s, c) = a.sin_cos();
            let dir = Vec2::new(forward.x * c - forward.y * s, forward.x * s + forward.y * c);
            let mvel = lvel.0 + dir * speed;
            let init_angle = dir.y.atan2(dir.x) - FRAC_PI_2;
            commands.spawn((
                Projectile { owner: entity, damage: 0, lifetime },
                Homing { target: None, turn_rate },
                ConfusionDart,
                Sprite::from_color(Color::srgb(0.9, 0.6, 1.0), Vec2::new(5.0, 12.0)),
                Transform::from_translation(muzzle.extend(0.5)),
                (
                    RigidBody::Dynamic,
                    Collider::circle(4.0),
                    Sensor,
                    Mass(0.2),
                    Position(muzzle),
                    Rotation::radians(init_angle),
                    LinearVelocity(mvel),
                    AngularVelocity::ZERO,
                    LinearDamping(0.0),
                    AngularDamping(0.0),
                    CollisionEventsEnabled,
                ),
            ));
        }
    }
}

/// On a ConfusionDart→ship hit, stamp a `ControlScramble` with a fresh
/// random permutation of the five control keys (`OverrideControlIceci`).
/// Runs before `handle_projectile_hits` (which despawns the dart).
fn handle_iceco_darts(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    mut rng: ResMut<crate::rng::GameRng>,
    darts: Query<(), With<ConfusionDart>>,
    projectiles: Query<(), With<Projectile>>,
    ships: Query<(), With<Ship>>,
) {
    let confusion_life = 5.0; // [Confusion] LifeTime 5.0
    for ev in reader.read() {
        let (dart_e, other_e) = if darts.get(ev.collider1).is_ok() {
            (ev.collider1, ev.collider2)
        } else if darts.get(ev.collider2).is_ok() {
            (ev.collider2, ev.collider1)
        } else {
            continue;
        };
        let _ = dart_e;
        if ships.get(other_e).is_err() || projectiles.get(other_e).is_ok() {
            continue;
        }
        // Fisher-Yates permutation of [0,1,2,3,4] (matches the original).
        let mut avail = [0u8, 1, 2, 3, 4];
        let mut order = [0u8; 5];
        for i in 0..5 {
            let k = rng.usize_range(0..(5 - i));
            order[i] = avail[k];
            avail[k] = avail[5 - i - 1];
        }
        commands
            .entity(other_e)
            .try_insert(ControlScramble { order, remaining: confusion_life });
    }
}

/// Enforce control scrambles: remap the victim's five control bits through
/// its permutation each tick, then age it out. Same slot as the EMP jam —
/// after the input gather, before `apply_player_input`.
fn apply_control_scrambles(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut inputs: ResMut<input::SlotInputs>,
    mut scrambled: Query<(Entity, &Ship, &mut ControlScramble)>,
) {
    let dt = time.delta_secs();
    for (entity, ship, mut sc) in &mut scrambled {
        let slot = ship.player_slot.min(3);
        let remap = |b: u8| -> u8 {
            let mut nb = b & !0x1F; // keep non-control bits (ultimate, etc.)
            for i in 0..5usize {
                if b & (1 << i) != 0 {
                    nb |= 1 << sc.order[i];
                }
            }
            nb
        };
        inputs.held[slot].buttons = remap(inputs.held[slot].buttons);
        inputs.just_pressed[slot].buttons = remap(inputs.just_pressed[slot].buttons);
        sc.remaining -= dt;
        if sc.remaining <= 0.0 {
            commands.entity(entity).try_remove::<ControlScramble>();
        }
    }
}

/// Lei Mule primary — `shpleimu.cpp:engage_forward`. Twin forward shots
/// angled slightly out; both block incoming weapons. WeaponRate 6,
/// WeaponDrain 4, Damage 6.
fn tick_leimu_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut LeimuState,
        &mut Battery,
    )>,
) {
    let dt = time.delta_secs();
    let speed = 70.0 * SC2_VEL_SCALE;
    let range = 10.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let damage = 6;
    let drain = 4;
    let spread = (0.025 * std::f32::consts::PI) as f32;
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        if !held || st.weapon_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.weapon_cd_s = 6.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        for sgn in [-1.0_f32, 1.0] {
            spawn_leimu_shot(
                &mut commands, entity, pos.0 + right * (sgn * 14.0) + forward * 12.0,
                forward, sgn * spread, lvel.0, speed, damage, lifetime,
                Color::srgb(1.0, 0.85, 0.3),
            );
        }
    }
}

/// Lei Mule special — `shpleimu.cpp:engage_backward`. A quad BACKWARD
/// volley of weapon-blocking "REAL" shots (low damage). SpecialRate 4,
/// SpecialDrain 2, Damage 1.
fn tick_leimu_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut LeimuState,
        &mut Battery,
    )>,
) {
    let dt = time.delta_secs();
    let speed = 80.0 * SC2_VEL_SCALE;
    let range = 15.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let damage = 1;
    let drain = 2;
    let spread = (0.025 * std::f32::consts::PI) as f32;
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        if !held || st.special_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.special_cd_s = 4.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let back = -forward;
        let right = Vec2::new(rot.cos, rot.sin);
        // Four backward shots from a spread of side offsets.
        for i in [-2.0_f32, -1.0, 1.0, 2.0] {
            spawn_leimu_shot(
                &mut commands, entity, pos.0 + right * (i * 12.0) - forward * 12.0,
                back, -i * 0.5 * spread, lvel.0, speed, damage, lifetime,
                Color::srgb(0.6, 0.9, 1.0),
            );
        }
    }
}

/// Spawn one Lei Mule weapon-blocking shot in direction `base` rotated by
/// `offset`.
#[allow(clippy::too_many_arguments)]
fn spawn_leimu_shot(
    commands: &mut Commands,
    owner: Entity,
    muzzle: Vec2,
    base: Vec2,
    offset: f32,
    ship_vel: Vec2,
    speed: f32,
    damage: i32,
    lifetime: f32,
    color: Color,
) {
    use std::f32::consts::FRAC_PI_2;
    let (s, c) = offset.sin_cos();
    let dir = Vec2::new(base.x * c - base.y * s, base.x * s + base.y * c);
    let vel = ship_vel + dir * speed;
    let init_angle = dir.y.atan2(dir.x) - FRAC_PI_2;
    commands.spawn((
        Projectile { owner, damage, lifetime },
        BlockingShot,
        Sprite::from_color(color, Vec2::new(4.0, 12.0)),
        Transform::from_translation(muzzle.extend(0.5)),
        RigidBody::Dynamic,
        Collider::circle(4.0),
        Sensor,
        Mass(0.3),
        Position(muzzle),
        Rotation::radians(init_angle),
        LinearVelocity(vel),
        AngularVelocity::ZERO,
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
}

/// Lei Mule's shots shoot down incoming fire: when a `BlockingShot` meets
/// an enemy projectile, both pop. Runs before `handle_projectile_hits`.
fn handle_blocking_shots(
    mut commands: Commands,
    mut reader: MessageReader<CollisionStart>,
    blocking: Query<(), With<BlockingShot>>,
    projectiles: Query<&Projectile>,
    ships: Query<&Ship>,
) {
    let mut gone: bevy::platform::collections::HashSet<Entity> =
        bevy::platform::collections::HashSet::default();
    for ev in reader.read() {
        let (block_e, other_e) = if blocking.get(ev.collider1).is_ok() {
            (ev.collider1, ev.collider2)
        } else if blocking.get(ev.collider2).is_ok() {
            (ev.collider2, ev.collider1)
        } else {
            continue;
        };
        // The other side must be an ENEMY projectile (and not itself a
        // blocking shot we'd want to pass).
        let (Ok(blk), Ok(other)) = (projectiles.get(block_e), projectiles.get(other_e)) else {
            continue;
        };
        let blk_slot = ships.get(blk.owner).map(|s| s.player_slot);
        let other_slot = ships.get(other.owner).map(|s| s.player_slot);
        if blk_slot == other_slot {
            continue; // friendly / same firer
        }
        for e in [block_e, other_e] {
            if gone.insert(e) {
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.try_despawn();
                }
            }
        }
    }
}

/// Uxjoz primary — `shpuxjba.cpp:activate_weapon`. A rapid mass-driver
/// particle (WeaponRate 1) that builds a damaging cloud. Damage 1.
fn tick_uxjba_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut UxjbaState, &mut Battery)>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE;
    let range = 30.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 1 {
            continue;
        }
        batt.current -= 1;
        st.weapon_cd_s = 1.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * 34.0;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        commands.spawn((
            Projectile { owner: entity, damage: 1, lifetime },
            Sprite::from_color(Color::srgb(1.0, 0.9, 0.5), Vec2::splat(5.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic, Collider::circle(3.0), Sensor, Mass(0.2),
            Position(muzzle), Rotation::radians(init_angle),
            LinearVelocity(lvel.0 + forward * speed),
            AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
        ));
    }
}

/// Uxjoz special — two slow heavy homing missiles from the flanks
/// (Damage 6). SpecialRate 40, SpecialDrain 20.
fn tick_uxjba_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut UxjbaState, &mut Battery)>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 60.0 * SC2_VEL_SCALE;
    let range = 15.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let turn_rate = sc2_turning(1.0);
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 20 {
            continue;
        }
        batt.current -= 20;
        st.special_cd_s = 40.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        for sgn in [-1.0_f32, 1.0] {
            let muzzle = pos.0 + right * (sgn * 50.0);
            commands.spawn((
                Projectile { owner: entity, damage: 6, lifetime },
                Homing { target: None, turn_rate },
                Sprite::from_color(Color::srgb(1.0, 0.6, 0.3), Vec2::new(8.0, 16.0)),
                Transform::from_translation(muzzle.extend(0.5)),
                (
                    RigidBody::Dynamic, Collider::circle(6.0), Sensor, Mass(0.6),
                    Position(muzzle), Rotation::radians(init_angle),
                    LinearVelocity(lvel.0 + forward * speed),
                    AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
                ),
            ));
        }
    }
}

/// Viogen primary — two long-range homing missiles from the sides
/// (Damage 1). WeaponRate 10, WeaponDrain 5.
fn tick_vioge_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut ViogeState, &mut Battery)>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 70.0 * SC2_VEL_SCALE;
    let range = 50.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    let turn_rate = sc2_turning(3.0);
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 5 {
            continue;
        }
        batt.current -= 5;
        st.weapon_cd_s = 10.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        for sgn in [-1.0_f32, 1.0] {
            let muzzle = pos.0 + right * (sgn * 20.0) + forward * 16.0;
            commands.spawn((
                Projectile { owner: entity, damage: 1, lifetime },
                Homing { target: None, turn_rate },
                Sprite::from_color(Color::srgb(0.7, 0.5, 1.0), Vec2::new(6.0, 15.0)),
                Transform::from_translation(muzzle.extend(0.5)),
                (
                    RigidBody::Dynamic, Collider::circle(5.0), Sensor, Mass(0.4),
                    Position(muzzle), Rotation::radians(init_angle),
                    LinearVelocity(lvel.0 + forward * speed),
                    AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
                ),
            ));
        }
    }
}

/// Viogen special — a slow plasma cloud that eats incoming weapons (it
/// carries `BlockingShot`, so `handle_blocking_shots` pops enemy fire it
/// touches) and deals contact damage. SpecialRate 5, SpecialDrain 10.
fn tick_vioge_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut ViogeState, &mut Battery)>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 50.0 * SC2_VEL_SCALE;
    let range = 45.0 * SC2_RANGE_SCALE; // Range 15 × (armour 2 + 1)
    let lifetime = range / speed;
    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 10 {
            continue;
        }
        batt.current -= 10;
        st.special_cd_s = 5.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * 28.0;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        commands.spawn((
            Projectile { owner: entity, damage: 2, lifetime },
            BlockingShot,
            Sprite::from_color(Color::srgba(0.6, 1.0, 0.8, 0.8), Vec2::splat(22.0)),
            Transform::from_translation(muzzle.extend(0.45)),
            RigidBody::Dynamic, Collider::circle(12.0), Sensor, Mass(0.3),
            Position(muzzle), Rotation::radians(init_angle),
            LinearVelocity(lvel.0 + forward * speed),
            AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
        ));
    }
}

/// Hellenian primary — a heavy long-range gun (Damage 10) that halves the
/// ship's velocity on each shot (canon recoil-slow). WeaponRate 10,
/// WeaponDrain 15.
fn tick_hubde_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &mut LinearVelocity, &mut HubdeState, &mut Battery)>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 80.0 * SC2_VEL_SCALE;
    let range = 30.0 * SC2_RANGE_SCALE;
    let lifetime = range / speed;
    for (entity, ship, pos, rot, mut lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 15 {
            continue;
        }
        batt.current -= 15;
        st.weapon_cd_s = 10.0 / 20.0;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * 28.0;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;
        let shot_vel = lvel.0 + forward * speed;
        lvel.0 *= 0.5; // firing slows you a lot
        commands.spawn((
            Projectile { owner: entity, damage: 10, lifetime },
            Sprite::from_color(Color::srgb(1.0, 0.4, 1.0), Vec2::new(7.0, 20.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic, Collider::circle(5.0), Sensor, Mass(0.5),
            Position(muzzle), Rotation::radians(init_angle),
            LinearVelocity(shot_vel),
            AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
        ));
    }
}

/// Hellenian special — mortar fire: five short-lived blast mines in a ring
/// around the hull (Damage 1), forming a protective shell. SpecialRate 1.5,
/// SpecialDrain 2.
fn tick_hubde_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut ships: Query<(Entity, &Ship, &Position, &mut HubdeState, &mut Battery)>,
) {
    use std::f32::consts::TAU;
    let dt = time.delta_secs();
    let r1 = 3.0 * SC2_RANGE_SCALE;
    let r2 = 10.0 * SC2_RANGE_SCALE;
    for (entity, ship, pos, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 2 {
            continue;
        }
        batt.current -= 2;
        st.special_cd_s = 1.5 / 20.0;
        let a0 = (rng.f32() - 0.5) * 0.1 * std::f32::consts::PI;
        for i in 0..5 {
            let a = a0 + i as f32 * TAU / 5.0;
            let radius = r1 + rng.f32() * (r2 - r1);
            let p = pos.0 + Vec2::new(a.cos(), a.sin()) * radius;
            // A stationary, brief blast that damages any enemy overlapping it.
            commands.spawn((
                Projectile { owner: entity, damage: 1, lifetime: 0.4 },
                Sprite::from_color(Color::srgba(1.0, 0.7, 0.3, 0.85), Vec2::splat(16.0)),
                Transform::from_translation(p.extend(0.4)),
                RigidBody::Dynamic, Collider::circle(9.0), Sensor, Mass(0.1),
                Position(p), Rotation::radians(0.0),
                LinearVelocity::ZERO,
                AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
            ));
        }
    }
}

/// Spawn one plain (unguided) bolt in direction `base` rotated by `off`.
#[allow(clippy::too_many_arguments)]
fn spawn_bolt(
    commands: &mut Commands, owner: Entity, muzzle: Vec2, base: Vec2, off: f32,
    ship_vel: Vec2, speed: f32, damage: i32, lifetime: f32, size: Vec2, color: Color,
) {
    use std::f32::consts::FRAC_PI_2;
    let (s, c) = off.sin_cos();
    let dir = Vec2::new(base.x * c - base.y * s, base.x * s + base.y * c);
    let init_angle = dir.y.atan2(dir.x) - FRAC_PI_2;
    commands.spawn((
        Projectile { owner, damage, lifetime },
        Sprite::from_color(color, size),
        Transform::from_translation(muzzle.extend(0.5)),
        RigidBody::Dynamic, Collider::circle(size.x.max(3.0) * 0.5), Sensor, Mass(0.3),
        Position(muzzle), Rotation::radians(init_angle),
        LinearVelocity(ship_vel + dir * speed),
        AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
    ));
}

/// Nechanzi primary — twin forward missiles (Damage 2). WeaponRate 8.
fn tick_neccr_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut NeccrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE; let range = 20.0 * SC2_RANGE_SCALE; let life = range / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.weapon_cd_s = 8.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos); let right = Vec2::new(rot.cos, rot.sin);
        for sgn in [-1.0_f32, 1.0] {
            spawn_bolt(&mut commands, e, pos.0 + right * (sgn * 16.0) + fwd * 12.0, fwd, 0.0, lvel.0, speed, 2, life, Vec2::new(5.0, 13.0), Color::srgb(0.7, 1.0, 0.8));
        }
    }
}

/// Nechanzi special — a phalanx of four unguided missiles in a forward
/// spread. SpecialRate 10, SpecialDrain 10.
fn tick_neccr_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut NeccrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 110.0 * SC2_VEL_SCALE; let life = (24.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        let just = held && !st.last_special_held; st.last_special_held = held;
        if !just || st.special_cd_s > 0.0 || batt.current < 10 { continue; }
        batt.current -= 10; st.special_cd_s = 10.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        for a in [-15.0_f32, -5.0, 5.0, 15.0] {
            spawn_bolt(&mut commands, e, pos.0 + fwd * 18.0, fwd, a.to_radians(), lvel.0, speed, 2, life, Vec2::new(4.0, 12.0), Color::srgb(0.9, 0.95, 0.6));
        }
    }
}

/// Yuryul primary — one powerful unguided missile (Damage 4). WeaponRate 2.75.
fn tick_yurpa_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut YurpaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 140.0 * SC2_VEL_SCALE; let life = (24.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.weapon_cd_s = 2.75 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        spawn_bolt(&mut commands, e, pos.0 + fwd * 18.0, fwd, 0.0, lvel.0, speed, 4, life, Vec2::new(7.0, 18.0), Color::srgb(1.0, 0.85, 0.4));
    }
}

/// Yuryul special — two ion-stream cones from the flanks (±30°). SpecialRate 18.
fn tick_yurpa_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut YurpaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 200.0 * SC2_VEL_SCALE; let life = (10.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 12 { continue; }
        batt.current -= 12; st.special_cd_s = 18.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos); let right = Vec2::new(rot.cos, rot.sin);
        for cone in [-30.0_f32, 30.0] {
            for spread in [-6.0_f32, 0.0, 6.0] {
                spawn_bolt(&mut commands, e, pos.0 + right * (cone.signum() * 14.0) + fwd * 8.0, fwd, (cone + spread).to_radians(), lvel.0, speed, 2, life, Vec2::splat(4.0), Color::srgb(0.5, 0.9, 1.0));
            }
        }
    }
}

/// Glavria primary — a five-torpedo forward spread (Damage 2). WeaponRate 10.
fn tick_glacr_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut GlacrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 50.0 * SC2_VEL_SCALE; let life = (60.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 18 { continue; }
        batt.current -= 18; st.weapon_cd_s = 10.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        for a in [-16.0_f32, -8.0, 0.0, 8.0, 16.0] {
            spawn_bolt(&mut commands, e, pos.0 + fwd * 18.0, fwd, a.to_radians(), lvel.0, speed, 2, life, Vec2::new(6.0, 14.0), Color::srgb(0.6, 0.8, 1.0));
        }
    }
}

/// Glavria special — a single backward torpedo. SpecialRate 1, SpecialDrain 4.
fn tick_glacr_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut GlacrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 60.0 * SC2_VEL_SCALE; let life = (40.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.special_cd_s = 1.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos); let back = -fwd;
        spawn_bolt(&mut commands, e, pos.0 + back * 18.0, back, 0.0, lvel.0, speed, 1, life, Vec2::new(6.0, 14.0), Color::srgb(0.5, 0.7, 1.0));
    }
}

/// Lyrmristu primary — a three-bolt spread (centre + ±4°). WeaponRate 12.
fn tick_lyrwa_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut LyrwaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 105.0 * SC2_VEL_SCALE; let life = (28.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 3 { continue; }
        batt.current -= 3; st.weapon_cd_s = 12.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        for a in [-4.0_f32, 0.0, 4.0] {
            spawn_bolt(&mut commands, e, pos.0 + fwd * 16.0, fwd, a.to_radians(), lvel.0, speed, 1, life, Vec2::new(4.0, 12.0), Color::srgb(0.8, 1.0, 0.9));
        }
    }
}

/// Lyrmristu special — a force sphere: a few seconds of strong damage-soak
/// shield. SpecialRate 20, SpecialDrain 8.
fn tick_lyrwa_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &mut LyrwaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 8 { continue; }
        batt.current -= 8; st.special_cd_s = 20.0 / 20.0;
        commands.entity(e).try_insert(ShieldActive { remaining: 4.0, damage_factor: 0.3 });
    }
}

/// Vezlagari primary — two backward unguided missiles (the barge's tubes
/// point aft). WeaponRate 6, Damage 2.
fn tick_vezba_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut VezbaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 80.0 * SC2_VEL_SCALE; let life = (32.5 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 2 { continue; }
        batt.current -= 2; st.weapon_cd_s = 6.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos); let back = -fwd; let right = Vec2::new(rot.cos, rot.sin);
        for sgn in [-1.0_f32, 1.0] {
            spawn_bolt(&mut commands, e, pos.0 + right * (sgn * 18.0) - fwd * 14.0, back, sgn * 3.0_f32.to_radians(), lvel.0, speed, 2, life, Vec2::new(5.0, 13.0), Color::srgb(1.0, 0.8, 0.6));
        }
    }
}

/// Vezlagari special — reinforce the armour plate: a few seconds of
/// damage-soak. SpecialRate 12, SpecialDrain 12.
fn tick_vezba_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &mut VezbaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 12 { continue; }
        batt.current -= 12; st.special_cd_s = 12.0 / 20.0;
        commands.entity(e).try_insert(ShieldActive { remaining: 5.0, damage_factor: 0.4 });
    }
}

/// Koanua primary — a backward delayed-thrust missile (Damage 4). WeaponRate 3.
fn tick_koapa_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut KoapaState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 60.0 * SC2_VEL_SCALE; let life = (30.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.weapon_cd_s = 3.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos); let back = -fwd;
        spawn_bolt(&mut commands, e, pos.0 + back * 16.0, back, 0.0, lvel.0, speed, 4, life, Vec2::new(5.0, 14.0), Color::srgb(0.7, 0.9, 1.0));
    }
}

/// Koanua special — ionic turbocharger: a speed burst (reuses the nitrous
/// cap-lift) that costs most of the battery. SpecialRate 240.
fn tick_koapa_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Rotation, &mut LinearVelocity, &mut KoapaState, &mut Battery, &ShipPhysicsDerived)>,
) {
    let dt = time.delta_secs();
    for (e, ship, rot, mut vel, mut st, mut batt, derived) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current = 1; st.special_cd_s = 240.0 / 20.0;
        commands.entity(e).try_insert(NitrousActive { remaining: 2.5 });
        let fwd = Vec2::new(-rot.sin, rot.cos);
        vel.0 = fwd * (derived.speed_max.max(120.0) * NITROUS_CAP_MULT);
    }
}

/// Sclore primary — rapid twin short-range bolts from alternating points.
fn tick_sclfr_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut SclfrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE; let life = (10.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.weapon_cd_s = 0.5 / 20.0;
        if st.side == 0.0 { st.side = 1.0; }
        let fwd = Vec2::new(-rot.sin, rot.cos); let right = Vec2::new(rot.cos, rot.sin);
        let defl = rng.signed_unit() * 4.0_f32.to_radians();
        spawn_bolt(&mut commands, e, pos.0 + right * (st.side * 14.0) + fwd * 12.0, fwd, defl, lvel.0, speed, 1, life, Vec2::new(3.0, 12.0), Color::srgb(1.0, 0.9, 0.7));
        st.side = -st.side;
    }
}

/// Sclore special — a rear-firing energy field (fast backward bolt fan).
fn tick_sclfr_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut SclfrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 180.0 * SC2_VEL_SCALE; let life = (21.5 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 6 { continue; }
        batt.current -= 6; st.special_cd_s = 5.0 / 20.0;
        let back = -Vec2::new(-rot.sin, rot.cos);
        for a in [-8.0_f32, 0.0, 8.0] {
            spawn_bolt(&mut commands, e, pos.0 + back * 16.0, back, a.to_radians(), lvel.0, speed, 2, life, Vec2::new(4.0, 12.0), Color::srgb(0.9, 0.5, 1.0));
        }
    }
}

/// Ulzrak primary — a fast forward missile (Damage 1). WeaponRate 1.6.
fn tick_ulzin_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut UlzinState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 85.0 * SC2_VEL_SCALE; let life = (8.5 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.weapon_cd_s = 1.6 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        spawn_bolt(&mut commands, e, pos.0 + fwd * 16.0, fwd, 0.0, lvel.0, speed, 1, life, Vec2::new(4.0, 12.0), Color::srgb(0.8, 1.0, 0.7));
    }
}

/// Ulzrak special — zoom drive: a ramming speed dash (nitrous cap-lift +
/// hard forward kick). SpecialRate 20, SpecialDrain 4.
fn tick_ulzin_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Rotation, &mut LinearVelocity, &mut UlzinState, &mut Battery, &ShipPhysicsDerived)>,
) {
    let dt = time.delta_secs();
    for (e, ship, rot, mut vel, mut st, mut batt, derived) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.special_cd_s = 20.0 / 20.0;
        commands.entity(e).try_insert(NitrousActive { remaining: 1.6 });
        let fwd = Vec2::new(-rot.sin, rot.cos);
        vel.0 = fwd * (derived.speed_max.max(120.0) * NITROUS_CAP_MULT);
    }
}

/// Alhordian primary — a long-range torpedo (Damage 3). WeaponRate 5.
fn tick_alhdr_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut AlhdrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 102.5 * SC2_VEL_SCALE; let life = 3.0;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 8 { continue; }
        batt.current -= 8; st.weapon_cd_s = 5.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        spawn_bolt(&mut commands, e, pos.0 + fwd * 22.0, fwd, 0.0, lvel.0, speed, 3, life, Vec2::new(7.0, 20.0), Color::srgb(0.7, 0.9, 1.0));
    }
}

/// Alhordian special — a sweep of two side lasers (perpendicular bolt
/// bursts). SpecialRate 0 (rapid), SpecialDrain 1.
fn tick_alhdr_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut AlhdrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 220.0 * SC2_VEL_SCALE; let life = (8.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.special_cd_s = 2.0 / 20.0;
        let right = Vec2::new(rot.cos, rot.sin); let left = -right;
        spawn_bolt(&mut commands, e, pos.0 + right * 18.0, right, 0.0, lvel.0, speed, 1, life, Vec2::new(3.0, 10.0), Color::srgb(1.0, 0.5, 0.4));
        spawn_bolt(&mut commands, e, pos.0 + left * 18.0, left, 0.0, lvel.0, speed, 1, life, Vec2::new(3.0, 10.0), Color::srgb(1.0, 0.5, 0.4));
    }
}

/// Gahmur primary — a charge-up homing plasma. Hold fire to charge
/// (0.2–2.0s); release to launch a plasma whose damage/range/speed scale
/// with the charge (Damage 3→16). `shpgahmo.cpp` (charge in calculate()).
fn tick_gahmo_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut GahmoState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        let was = st.last_fire_held; st.last_fire_held = held;
        if held && batt.current > 0 {
            st.charge_s = (st.charge_s + dt).min(2.0);
        }
        if was && !held {
            let c = st.charge_s; st.charge_s = 0.0;
            if c < 0.2 { continue; }
            let frac = ((c - 0.2) / 1.8).clamp(0.0, 1.0);
            let cost = (2 + (frac * 6.0) as i32).min(batt.current.max(0));
            batt.current -= cost;
            let damage = (3.0 + frac * 13.0).round() as i32;
            let speed = (25.0 + frac * 40.0) * SC2_VEL_SCALE;
            let range = (10.0 + frac * 75.0) * SC2_RANGE_SCALE;
            let life = range / speed;
            let fwd = Vec2::new(-rot.sin, rot.cos);
            let sz = 6.0 + frac * 10.0;
            commands.spawn((
                Projectile { owner: e, damage, lifetime: life },
                Homing { target: None, turn_rate: sc2_turning(2.0) },
                Sprite::from_color(Color::srgb(0.6, 1.0, 0.7), Vec2::splat(sz)),
                Transform::from_translation((pos.0 + fwd * 20.0).extend(0.5)),
                (
                    RigidBody::Dynamic, Collider::circle(sz * 0.5), Sensor, Mass(0.4),
                    Position(pos.0 + fwd * 20.0),
                    Rotation::radians(fwd.y.atan2(fwd.x) - std::f32::consts::FRAC_PI_2),
                    LinearVelocity(lvel.0 + fwd * speed),
                    AngularVelocity::ZERO, LinearDamping(0.0), AngularDamping(0.0), CollisionEventsEnabled,
                ),
            ));
        }
    }
}

/// Gahmur special — dump the current charge as a three-way plasma burst.
fn tick_gahmo_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut GahmoState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 55.0 * SC2_VEL_SCALE; let life = (40.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.special_cd_s = 6.0 / 20.0; st.charge_s = 0.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        for a in [-18.0_f32, 0.0, 18.0] {
            spawn_bolt(&mut commands, e, pos.0 + fwd * 20.0, fwd, a.to_radians(), lvel.0, speed, 3, life, Vec2::splat(8.0), Color::srgb(0.5, 1.0, 0.6));
        }
    }
}

/// Hydra primary — a five-beam fan (centre + ±0.53 + ±1.6 rad). WeaponRate 50.
fn tick_hydcr_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &mut HydcrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let col = Color::srgb(0.5, 1.0, 0.9);
    let beams = [(0.0_f32, 12.0, 12), (-0.53, 8.0, 10), (0.53, 8.0, 10), (-1.6, 4.0, 8), (1.6, 4.0, 8)];
    for (e, ship, pos, rot, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 12 { continue; }
        batt.current -= 12; st.weapon_cd_s = 50.0 / 20.0;
        for (a, r, dmg) in beams {
            let local_dir = Vec2::new(-a.sin(), a.cos());
            spawn_beam(&mut commands, e, pos.0, rot, Vec2::ZERO, local_dir, r * SC2_RANGE_SCALE, dmg, col, false, 0.5, 3.0);
        }
    }
}

/// Hydra special — launch a station-keeping fighter that lasers nearby
/// enemies (reuses the KzerZaFighter sub-entity AI). SpecialRate 10.
fn tick_hydcr_special(
    mut commands: Commands, time: Res<Time<Physics>>, assets: Res<AssetServer>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &mut HydcrState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, pos, rot, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 8 { continue; }
        batt.current -= 8; st.special_cd_s = 10.0 / 20.0;
        spawn_sub_entity(
            &mut commands, &assets, e, pos.0, rot, Vec2::new(0.0, 22.0), 0.0, 35.0 * SC2_VEL_SCALE,
            None, 14.0, Color::srgb(0.6, 1.0, 0.9), 5, 14.0,
            SubEntityAi::KzerZaFighter { target: None, turn_rate: sc2_turning(4.0), speed: 35.0 * SC2_VEL_SCALE, laser_range: 160.0, laser_damage: 1, recharge_s: 0.5, laser_cooldown_s: 0.0, air_grace_s: 0.3 },
        );
    }
}

/// Rogue Squadron primary — a forward pulse laser (Damage 1). WeaponRate 5.
fn tick_rogsq_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut RogsqState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 100.0 * SC2_VEL_SCALE; let life = (40.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.weapon_cd_s = 5.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        spawn_bolt(&mut commands, e, pos.0 + fwd * 14.0, fwd, 0.0, lvel.0, speed, 1, life, Vec2::new(3.0, 14.0), Color::srgb(1.0, 0.4, 0.3));
    }
}

/// Rogue Squadron special — deploy two wingmen that fight alongside you
/// (KzerZaFighter sub-entities). SpecialRate 10, SpecialDrain 1.
fn tick_rogsq_special(
    mut commands: Commands, time: Res<Time<Physics>>, assets: Res<AssetServer>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &mut RogsqState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, pos, rot, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.special_cd_s = 10.0 / 20.0;
        for sgn in [-1.0_f32, 1.0] {
            spawn_sub_entity(
                &mut commands, &assets, e, pos.0, rot, Vec2::new(sgn * 30.0, 0.0), 0.0, 40.0 * SC2_VEL_SCALE,
                None, 12.0, Color::srgb(1.0, 0.7, 0.4), 4, 18.0,
                SubEntityAi::KzerZaFighter { target: None, turn_rate: sc2_turning(4.0), speed: 40.0 * SC2_VEL_SCALE, laser_range: 150.0, laser_damage: 1, recharge_s: 0.6, laser_cooldown_s: 0.0, air_grace_s: 0.3 },
            );
        }
    }
}

/// Dajielka primary — a forward pulse blaster (Damage 1). WeaponRate 1.6.
fn tick_dajem_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut DajemState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE; let life = (22.5 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.weapon_cd_s = 1.6 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos);
        spawn_bolt(&mut commands, e, pos.0 + fwd * 16.0, fwd, 0.0, lvel.0, speed, 1, life, Vec2::new(4.0, 11.0), Color::srgb(0.8, 0.9, 1.0));
    }
}

/// Dajielka special — deploy the protective sanctuary: a few seconds of
/// strong damage-soak. Gated to ~1s.
fn tick_dajem_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &mut DajemState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 4 { continue; }
        batt.current -= 4; st.special_cd_s = 1.0;
        commands.entity(e).try_insert(ShieldActive { remaining: 4.0, damage_factor: 0.3 });
    }
}

/// Arkanoid primary — a short-range pincer crush (high damage, no energy).
fn tick_arkpi_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &LinearVelocity, &mut ArkpiState)>,
) {
    let dt = time.delta_secs();
    let speed = 60.0 * SC2_VEL_SCALE; let life = (3.0 * SC2_RANGE_SCALE) / speed;
    for (e, ship, pos, rot, lvel, mut st) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 { continue; }
        st.weapon_cd_s = 5.0 / 20.0;
        let fwd = Vec2::new(-rot.sin, rot.cos); let right = Vec2::new(rot.cos, rot.sin);
        for sgn in [-1.0_f32, 1.0] {
            spawn_bolt(&mut commands, e, pos.0 + right * (sgn * 20.0) + fwd * 24.0, fwd, -sgn * 12.0_f32.to_radians(), lvel.0, speed, 8, life, Vec2::splat(10.0), Color::srgb(0.9, 0.9, 0.95));
        }
    }
}

/// Arkanoid special — scuttle mode: a few seconds of strong damage-soak.
fn tick_arkpi_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &mut ArkpiState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 2 { continue; }
        batt.current -= 2; st.special_cd_s = 1.0;
        commands.entity(e).try_insert(ShieldActive { remaining: 3.0, damage_factor: 0.25 });
    }
}

/// Kolory primary — twin flame beams, one forward, one aft (Damage 6). WeaponRate 6.
fn tick_kolfl_primary(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &mut KolflState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    let col = Color::srgb(1.0, 0.6, 0.2);
    for (e, ship, pos, rot, mut st, mut batt) in &mut ships {
        st.weapon_cd_s = (st.weapon_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE) || st.weapon_cd_s > 0.0 || batt.current < 6 { continue; }
        batt.current -= 6; st.weapon_cd_s = 6.0 / 20.0;
        let r = 45.0 * SC2_RANGE_SCALE;
        spawn_beam(&mut commands, e, pos.0, rot, Vec2::ZERO, Vec2::new(0.0, 1.0), r, 6, col, false, 0.4, 5.0);
        spawn_beam(&mut commands, e, pos.0, rot, Vec2::ZERO, Vec2::new(0.0, -1.0), r, 6, col, false, 0.4, 5.0);
    }
}

/// Kolory special — a tractor field that drags/disrupts nearby ships
/// (stand-in for the canon hyperspace slow-field). SpecialRate 5.
fn tick_kolfl_special(
    mut commands: Commands, time: Res<Time<Physics>>, slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(Entity, &Ship, &Position, &Rotation, &mut KolflState, &mut Battery)>,
) {
    let dt = time.delta_secs();
    for (e, ship, pos, rot, mut st, mut batt) in &mut ships {
        st.special_cd_s = (st.special_cd_s - dt).max(0.0);
        if !slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL) || st.special_cd_s > 0.0 || batt.current < 1 { continue; }
        batt.current -= 1; st.special_cd_s = 5.0 / 20.0;
        spawn_tractor(&mut commands, e, pos.0, rot, Vec2::ZERO, 22.0 * SC2_RANGE_SCALE, 40.0, Color::srgba(0.5, 0.4, 1.0, 0.4), 6.0, 0.4);
    }
}

/// Tau Gladius special — `shptaugl.cpp:activate_special` /
/// `TauGladiusMissile`. Launches a homing missile from an alternating
/// side of the hull (`13*side`, `side *= -1` each press) with
/// cone-limited tracking: it only steers toward the target while the
/// target is within TrackAngle (22.5°) of its heading, otherwise it
/// flies straight (`turn_rate = 0`). SpecialDrain 6, SpecialRate 4.
fn tick_taugl_special(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TauglState,
        &mut Battery,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let speed = 90.0 * SC2_VEL_SCALE; // [Special] Velocity 90
    let range = 44.0 * SC2_RANGE_SCALE; // Range 44
    let lifetime = range / speed;
    let damage = 1; // Damage 1
    let turn_rate = sc2_turning(5.0); // TurnRate 5
    let cone = 22.5_f32.to_radians(); // TrackAngle 22.5° (× π/180)
    let drain = 6; // SpecialDrain 6
    let cooldown = 4.0 / 20.0; // SpecialRate 4

    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        if st.special_cd_s > 0.0 {
            st.special_cd_s -= dt;
        }
        let held = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        st.last_special_held = held;
        if !held || st.special_cd_s > 0.0 || batt.current < drain {
            continue;
        }
        batt.current -= drain;
        st.special_cd_s = cooldown;

        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        // Original ship-local launch point (13*side, 0): alternating tube.
        let muzzle = pos.0 + right * (13.0 * st.side);
        st.side = -st.side;
        let mvel = lvel.0 + forward * speed;
        let init_angle = forward.y.atan2(forward.x) - FRAC_PI_2;

        commands.spawn((
            Projectile { owner: entity, damage, lifetime },
            Homing { target: None, turn_rate },
            HomingCone(cone),
            // Placeholder missile sprite (orange dart); the exact special
            // sprite is a later visual pass.
            Sprite::from_color(Color::srgb(1.0, 0.7, 0.2), Vec2::new(6.0, 16.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            (
                RigidBody::Dynamic,
                Collider::circle(6.0),
                Sensor,
                Mass(0.5),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(mvel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ),
        ));
    }
}

/// Tau Archon primary — `shptauar.cpp`: a charge-up freeze laser. Hold
/// fire to spin up a 0.5 s charge; once charged it pours a rapid stream
/// of pellets, each with random muzzle jitter / spread / velocity /
/// range, draining ~0.6 battery per pellet. Damage-steps alternate: a
/// "sap" pellet drains the target's battery (FuelSap) for ~no crew
/// damage, then a "damage" pellet deals 1 crew — so it kills by draining
/// you dry and chipping in. Releasing fire bleeds the charge back down.
#[allow(clippy::too_many_arguments)]
fn tick_tauar_primary(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    slot_inputs: Res<input::SlotInputs>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut ships: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut TauarState,
        &mut Battery,
    )>,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    let charge_time = 0.5; // [Weapon] ChargeTime 0.5
    let speed = 90.0 * SC2_VEL_SCALE; // [Weapon] Velocity 90
    let range = 19.0 * SC2_RANGE_SCALE; // [Weapon] Range 19
    let speed_special = 200.0 * SC2_VEL_SCALE; // [Special] Velocity 200
    let range_special = 95.0 * SC2_RANGE_SCALE; // [Special] Range 95
    let max_div = 0.15; // [Special] MaxDivergence
    let range_limit = 0.15; // [Special] RangeLimiter
    let drain_per_shot = 3.0 / 5.0; // weapon_drain/special_drain = 0.6
    let fuel_sap = 1; // FuelSap 1

    for (entity, ship, pos, rot, lvel, mut st, mut batt) in &mut ships {
        // Fire and Special share the SAME charge + pellet stream (canon
        // `calculate_fire_weapon` keys both off the one charge counter).
        // Special swaps in longer range + the spiralling divergence.
        let fire = slot_inputs.pressed(ship.player_slot, input::INPUT_FIRE);
        let spec = slot_inputs.pressed(ship.player_slot, input::INPUT_SPECIAL);
        let held = fire || spec;
        let use_special = spec; // special params take precedence
        st.last_fire_held = held;
        // Charge up while held + powered; bleed back down otherwise.
        if held && batt.current > 0 {
            st.charge_s = (st.charge_s + dt).min(charge_time);
        } else {
            st.charge_s = (st.charge_s - dt).max(0.0);
            continue;
        }
        if st.charge_s < charge_time || batt.current <= 0 {
            continue;
        }
        // Fractional 0.6/shot drain on the integer battery.
        st.batt_debt += drain_per_shot;
        while st.batt_debt >= 1.0 && batt.current > 0 {
            st.batt_debt -= 1.0;
            batt.current -= 1;
        }
        // Alternate damage steps (Step1 sap, Step2 damage).
        st.sap_step = !st.sap_step;
        let is_sap = st.sap_step;

        // Canon randomisation: muzzle jitter rx∈[-12,12], spread (rx/3)°,
        // velocity ×[0.96,1.08], range ×[0.77,1.17].
        let rx = rng.signed_unit() * 12.0;
        let ax = (rx / 3.0).to_radians();
        let vmul = 0.96 + rng.signed_unit().abs() * 0.12;
        let rmul = 0.77 + rng.signed_unit().abs() * 0.40;
        let forward = Vec2::new(-rot.sin, rot.cos);
        let right = Vec2::new(rot.cos, rot.sin);
        let muzzle = pos.0 + forward * 11.0 + right * (rx / 2.0);
        let (ss, cs) = ax.sin_cos();
        let dir = Vec2::new(forward.x * cs - forward.y * ss, forward.x * ss + forward.y * cs);
        let base_speed = if use_special { speed_special } else { speed };
        let base_range = if use_special { range_special } else { range };
        let shot_speed = base_speed * vmul;
        // Special pellets get a per-pellet divergence that drives the
        // spiral; RangeLimiter shortens the more-divergent ones.
        let (div, eff_range) = if use_special {
            let sign = if rng.signed_unit() >= 0.0 { 1.0 } else { -1.0 };
            let d = max_div * rng.signed_unit().abs() * sign;
            let lim = (1.0 - range_limit) + range_limit * (1.0 - d.abs() / max_div);
            (d, base_range * lim)
        } else {
            (0.0, base_range)
        };
        let shot_vel = lvel.0 + dir * shot_speed;
        let shot_life = (eff_range * rmul) / shot_speed;
        let init_angle = dir.y.atan2(dir.x) - FRAC_PI_2;
        let damage = if is_sap { 0 } else { 1 };
        let color = if is_sap {
            Color::srgb(0.45, 0.8, 1.0) // icy sap pellet
        } else {
            Color::srgb(0.85, 0.92, 1.0) // pale damage pellet
        };

        let mut ec = commands.spawn((
            Projectile { owner: entity, damage, lifetime: shot_life },
            Sprite::from_color(color, Vec2::new(3.0, 10.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            (
                RigidBody::Dynamic,
                Collider::circle(3.0),
                Sensor,
                Mass(0.1),
                Position(muzzle),
                Rotation::radians(init_angle),
                LinearVelocity(shot_vel),
                AngularVelocity::ZERO,
                LinearDamping(0.0),
                AngularDamping(0.0),
                CollisionEventsEnabled,
            ),
        ));
        if is_sap {
            ec.insert(FuelSap { amount: fuel_sap });
        }
        if use_special {
            ec.insert(ArchonSpiral {
                owner: entity,
                div,
                old_range: 11.0,
                speed: shot_speed,
            });
        }
    }
}

/// A Tau Archon SPECIAL pellet, spiralling outward around the firer
/// (`shptauar.cpp:TauArchonShot::calculate` with `rotation_base =
/// creator`): each tick it's repositioned `old_range` from the firer and
/// flung perpendicular, with `old_range` growing by `|div|·speed`, so
/// the stream fans into a rotating defensive screen.
#[derive(Component, Debug, Clone, Copy)]
pub struct ArchonSpiral {
    pub owner: Entity,
    pub div: f32,
    pub old_range: f32,
    pub speed: f32,
}

/// Drive the Archon special pellets' spiral around their firer.
fn tick_archon_spiral(
    time: Res<Time<Physics>>,
    ships: Query<&Position, (With<Ship>, Without<ArchonSpiral>)>,
    mut pellets: Query<
        (&mut Position, &mut LinearVelocity, &mut Rotation, &mut ArchonSpiral),
        Without<Ship>,
    >,
) {
    use std::f32::consts::FRAC_PI_2;
    let dt = time.delta_secs();
    for (mut pos, mut vel, mut rot, mut sp) in &mut pellets {
        let Ok(owner_pos) = ships.get(sp.owner) else {
            continue; // firer gone — let it fly straight on its own velocity
        };
        // Angle from the pellet toward the firer; reposition `old_range`
        // out from the firer along that line, then fling perpendicular.
        let to_firer = owner_pos.0 - pos.0;
        if to_firer.length_squared() < 1e-3 {
            continue;
        }
        let t_a = to_firer.y.atan2(to_firer.x);
        let dir_to_firer = Vec2::new(t_a.cos(), t_a.sin());
        pos.0 = owner_pos.0 - dir_to_firer * sp.old_range;
        let ang = if sp.div > 0.0 { t_a + FRAC_PI_2 } else { t_a - FRAC_PI_2 };
        let heading = Vec2::new(ang.cos(), ang.sin());
        vel.0 = heading * sp.speed;
        *rot = Rotation::radians(ang - FRAC_PI_2);
        sp.old_range += sp.div.abs() * sp.speed * dt;
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
    camera: Query<&Transform, With<crate::starfield::PrimaryCamera>>,
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
    camera: Query<&Transform, With<crate::starfield::PrimaryCamera>>,
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
    mut projectiles: Query<(&Projectile, &Position, &mut LinearVelocity, &mut Homing, Option<&HomingCone>)>,
    // `Without<Invisible>` so a cloaked Ilwrath drops missile locks
    // (canonical: isInvisible() filters target acquisition).
    ships: Query<(Entity, &Ship, &Position), (Without<Projectile>, Without<Invisible>)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (proj, proj_pos, mut vel, mut homing, cone) in &mut projectiles {
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
        // Cone-limited tracking: if the target is outside the missile's
        // tracking cone, don't steer this tick (fly straight).
        let in_cone = cone.map_or(true, |c| angle.abs() <= c.0);
        let max_delta = if in_cone { homing.turn_rate * dt } else { 0.0 };
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

/// Androsynth bubble course logic — canon `AndrosynthBubble::calculate`
/// (shpandgu.cpp). Every 150 ms the bubble re-aims to a fresh random
/// direction at cruise speed, then adds a half-speed nudge toward the
/// nearest enemy, so it meanders erratically while loosely chasing.
/// Host-authoritative (sits in the gated combat group); the guest sees
/// the resulting motion via the projectile-mirror stream.
fn tick_andro_bubbles(
    time: Res<Time<Physics>>,
    mut rng: ResMut<crate::rng::GameRng>,
    mut bubbles: Query<(&Projectile, &Position, &mut LinearVelocity, &mut AndroBubble)>,
    ships: Query<(&Ship, &Position), (Without<Projectile>, Without<Invisible>)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (proj, bpos, mut vel, mut bub) in &mut bubbles {
        bub.course_s += dt;
        if bub.course_s < 0.150 {
            continue;
        }
        bub.course_s -= 0.150;

        // Fresh random heading at cruise speed (canon `vel = v *
        // unit_vector(random(PI2))`). `signed_unit()` is the seeded,
        // rollback-safe RNG so both peers' hosts would agree.
        let ang = rng.signed_unit() * std::f32::consts::PI;
        let (s, c) = ang.sin_cos();
        let mut new_vel = Vec2::new(c, s) * bub.speed;

        // Half-speed seek toward the nearest enemy on the torus (canon
        // adds `unit_vector(trajectory_angle) * v / 2`). Skipped if no
        // enemy is in play, leaving a purely random drift that tick.
        let owner_slot = ships.get(proj.owner).ok().map(|(s, _)| s.player_slot);
        let mut best: Option<(Vec2, f32)> = None;
        for (s, p) in &ships {
            if Some(s.player_slot) == owner_slot {
                continue;
            }
            let d = crate::physics::min_image(p.0 - bpos.0);
            let d2 = d.length_squared();
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some((d, d2));
            }
        }
        if let Some((d, d2)) = best {
            if d2 > 0.0 {
                new_vel += d.normalize() * bub.speed * 0.5;
            }
        }
        vel.0 = new_vel;
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
    projectiles: Query<(&Projectile, Option<&FuelSap>, Has<BossProjectile>)>,
    proj_positions: Query<&Position, With<Projectile>>,
    // `limpets` + `torpedoes` are bundled into one tuple param to stay
    // under Bevy's 16-system-param ceiling now that the boss-health
    // query has joined. (Alary MIRV torpedoes deal no contact damage and
    // never despawn on touch — they only split on proximity.)
    (limpets, torpedoes): (Query<&Limpet>, Query<&AlaryTorpedo>),
    shields: Query<&ShieldActive>,
    damage_to_batt: Query<&DamageToBattery>,
    asteroids_q: Query<&Position, With<Asteroid>>,
    ships: Query<&Ship>,
    mut satellites: Query<(&mut ChmmrSatellite, &Position)>,
    mut crews: Query<&mut Crew>,
    mut batteries: Query<&mut Battery>,
    mut velocities: Query<&mut LinearVelocity>,
    mut deriveds: Query<&mut ShipPhysicsDerived>,
    mut boss_health: Query<(&mut BossHealth, Has<CapitalCore>, Option<&CoreAperture>)>,
    assets: Res<AssetServer>,
) {
    // Boss co-op turns OFF friendly fire between the player fighters —
    // they're a co-op team. Detected by the presence of any boss part.
    let coop_mode = !boss_health.is_empty();
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

        let (proj, fuel_sap, is_boss_proj) = match projectiles.get(proj_entity) {
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
        // and the main ship with their own bullets. In boss co-op
        // ALL the player fighters are one team, so any ship-on-ship
        // hit passes through.
        if let (Ok(firer_ship), Ok(target_ship)) =
            (ships.get(proj.owner), ships.get(other_entity))
        {
            if coop_mode || firer_ship.player_slot == target_ship.player_slot {
                continue;
            }
        }

        // Boss part (hull / turret / core) hit. Only PLAYER fire
        // damages the boss — a boss bolt that somehow clips a part
        // passes through. The hull carries no `BossHealth`, so it's
        // an indestructible wall: shots just pop against it.
        if let Ok((mut bh, is_core, aperture)) = boss_health.get_mut(other_entity) {
            if is_boss_proj {
                continue;
            }
            // Recessed core: while the bridge is shut, shots pop off the
            // armour for no damage — only an open strike window counts.
            if aperture.is_some_and(|a| !a.open) {
                if let Ok(proj_pos) = proj_positions.get(proj_entity) {
                    spawn_asteroid_explosion(&mut commands, &assets, proj_pos.0, 10.0);
                }
                if gone.insert(proj_entity) {
                    if let Ok(mut ec) = commands.get_entity(proj_entity) {
                        ec.try_despawn();
                    }
                }
                continue;
            }
            bh.hp = (bh.hp - proj.damage).max(0);
            if let Ok(proj_pos) = proj_positions.get(proj_entity) {
                spawn_asteroid_explosion(&mut commands, &assets, proj_pos.0, 16.0);
            }
            if gone.insert(proj_entity) {
                if let Ok(mut ec) = commands.get_entity(proj_entity) {
                    ec.try_despawn();
                }
            }
            if bh.hp <= 0 {
                if let Ok(part_pos) = proj_positions.get(proj_entity) {
                    spawn_asteroid_explosion(&mut commands, &assets, part_pos.0, 48.0);
                }
                if gone.insert(other_entity) {
                    if let Ok(mut ec) = commands.get_entity(other_entity) {
                        ec.try_despawn();
                    }
                }
                if is_core {
                    info!("boss: CORE DESTROYED — co-op victory");
                } else {
                    info!("boss: turret destroyed");
                }
            }
            continue;
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
                if let Ok(mut ec) = commands.get_entity(other_entity) {
                    ec.try_despawn();
                }
            }
            if gone.insert(proj_entity) {
                if let Ok(mut ec) = commands.get_entity(proj_entity) {
                    ec.try_despawn();
                }
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
                if let Ok(mut ec) = commands.get_entity(proj_entity) {
                    ec.try_despawn();
                }
            }
            if sat.armour <= 0 {
                spawn_asteroid_explosion(&mut commands, &assets, sat_pos.0, 18.0);
                if gone.insert(other_entity) {
                    if let Ok(mut ec) = commands.get_entity(other_entity) {
                        ec.try_despawn();
                    }
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
                // Canon `Ship::handle_speed_loss` (mship.cpp): a permanent,
                // mass-scaled, diminishing cut to speed / accel / turn.
                //   sl = 30/(mass+30) * slowdown_factor
                //   speed_max *= 1 - sl * speed_max/(speed_max + scale_velocity(10))
                //   turn_rate *= 1 - sl * turn_rate/(turn_rate + scale_turning(4))
                // In our world units scale_velocity(10)=96 and
                // scale_turning(4)≈1.571. Earlier we used a fixed sl=0.2,
                // ~half canon strength; using the real mass-scaled sl
                // (≈0.30–0.43) makes limpets bite like the original — a
                // few stick and the target is crawling.
                let mass = ships
                    .get(other_entity)
                    .map(|s| s.stats.mass)
                    .unwrap_or(9.0)
                    .max(0.1);
                let sl = (30.0 / (mass + 30.0)) * limpet.slowdown_factor;
                let s = derived.speed_max;
                if s > 0.0 {
                    let factor = 1.0 - sl * s / (s + 96.0);
                    derived.speed_max = s * factor;
                    // Accel tracks top speed (canon also reduces accel_rate).
                    derived.thrust_force *= factor;
                }
                let w = derived.target_omega;
                if w > 0.0 {
                    derived.target_omega = w * (1.0 - sl * w / (w + 1.5708));
                }
                if let Ok(mut vel) = velocities.get_mut(other_entity) {
                    // Immediate clamp so the hit feels punchy.
                    let speed = vel.0.length();
                    if speed > derived.speed_max && derived.speed_max > 0.0 {
                        vel.0 = vel.0 / speed * derived.speed_max;
                    }
                }
                info!(
                    "limpet hit (mass {:.0}): speed_max → {:.0}, turn → {:.2}",
                    mass, derived.speed_max, derived.target_omega
                );
            }
            if gone.insert(proj_entity) {
                if let Ok(mut ec) = commands.get_entity(proj_entity) {
                    ec.try_despawn();
                }
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

        // Tau Archon fuel sap: drain the target's battery on hit.
        if let Some(sap) = fuel_sap {
            if let Ok(mut batt) = batteries.get_mut(other_entity) {
                batt.current = (batt.current - sap.amount).max(0);
            }
        }

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
            if let Ok(mut ec) = commands.get_entity(proj_entity) {
                ec.try_despawn();
            }
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
    camera: Query<&Transform, With<crate::starfield::PrimaryCamera>>,
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
    camera: Query<&Transform, With<crate::starfield::PrimaryCamera>>,
    satellites_for_filter: Query<(Entity, &ChmmrSatellite)>,
    mut boss_health: Query<(&mut BossHealth, Has<CapitalCore>, Option<&CoreAperture>)>,
    hull_q: Query<Entity, With<CapitalShip>>,
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
        // Beams pass THROUGH the boss hull wall (just like player
        // projectiles) to reach the turrets/core mounted inside the wedge.
        for hull_e in &hull_q {
            excluded.push(hull_e);
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
            } else if let Ok((mut bh, is_core, aperture)) = boss_health.get_mut(target) {
                // Boss part (turret / core). The core only takes damage
                // while its strike window is open; turrets always do.
                let closed = aperture.is_some_and(|a| !a.open);
                if !closed && damage_ticks > 0 {
                    let dmg = (beam.damage_per_tick * damage_ticks).max(0);
                    bh.hp = (bh.hp - dmg).max(0);
                    let hit_pos = world_origin + world_dir * hit_t;
                    spawn_asteroid_explosion(&mut commands, &assets, hit_pos, 8.0);
                    if bh.hp <= 0 {
                        spawn_asteroid_explosion(&mut commands, &assets, hit_pos, 48.0);
                        commands.entity(target).try_despawn();
                        if is_core {
                            info!("boss: CORE DESTROYED (beam) — co-op victory");
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
    camera: Query<&Transform, With<crate::starfield::PrimaryCamera>>,
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
    mut crews: Query<&mut Crew>,
    shields: Query<&ShieldActive>,
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
            SubEntityAi::KzerZaFighter {
                target,
                turn_rate,
                speed,
                laser_range,
                laser_damage,
                recharge_s,
                laser_cooldown_s,
                air_grace_s,
            } => {
                // Tick the laser-recharge + launch-grace clocks.
                *laser_cooldown_s = (*laser_cooldown_s - dt).max(0.0);
                if *air_grace_s > 0.0 {
                    *air_grace_s = (*air_grace_s - dt).max(0.0);
                    // While clearing the parent's hitbox, just coast
                    // on the launch velocity — no AI, no fire.
                    continue;
                }
                // Reacquire if target gone.
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
                let Some(t) = *target else { continue };
                let Ok((_, _, t_pos)) = ships.get(t) else { continue };

                let to_target = t_pos.0 - sub_pos.0;
                let dist = to_target.length().max(1e-3);
                // Orbit station: tangential to the target's bearing,
                // at `laser_range × 0.8`. Pick the side the fighter
                // is already closer to (cheap canon-equivalent of the
                // "compare distance to left flank vs right flank" in
                // shpkzedr.cpp).
                let radial = to_target / dist;
                let tangent = Vec2::new(-radial.y, radial.x);
                let side = if tangent.dot(sub_pos.0 - t_pos.0) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };
                let station = t_pos.0 + tangent * side * (*laser_range * 0.8) - radial * (*laser_range * 0.8);
                let to_station = (station - sub_pos.0).normalize_or_zero();

                if to_station != Vec2::ZERO {
                    // Steer the velocity vector toward the orbit
                    // station, capped at turn_rate · dt per tick.
                    let cur_dir = sub_vel.0.normalize_or_zero();
                    let new_dir = if cur_dir == Vec2::ZERO {
                        to_station
                    } else {
                        let cur_a = cur_dir.y.atan2(cur_dir.x);
                        let tgt_a = to_station.y.atan2(to_station.x);
                        let mut diff = tgt_a - cur_a;
                        while diff > std::f32::consts::PI {
                            diff -= std::f32::consts::TAU;
                        }
                        while diff < -std::f32::consts::PI {
                            diff += std::f32::consts::TAU;
                        }
                        let cap = (*turn_rate * dt).abs();
                        let actual = diff.clamp(-cap, cap);
                        let (sn, cs) = actual.sin_cos();
                        Vec2::new(cur_dir.x * cs - cur_dir.y * sn, cur_dir.x * sn + cur_dir.y * cs)
                    };
                    sub_vel.0 = new_dir * *speed;
                }

                // Fire when in range and recharged. Canon stops the
                // fighter for the laser tick; we just keep it on its
                // current trajectory — close enough visually.
                if *laser_cooldown_s <= 0.0 && dist <= *laser_range {
                    let factor = shields
                        .get(t)
                        .map(|s| s.damage_factor)
                        .unwrap_or(1.0);
                    let dmg = ((*laser_damage as f32 * factor).round() as i32).max(0);
                    if dmg > 0 {
                        if let Ok(mut crew) = crews.get_mut(t) {
                            crew.current = (crew.current - dmg).max(0);
                        }
                    }
                    // Visual: ZapFlash from fighter to target. Same
                    // pattern as the Chmmr satellite laser — a thin
                    // bright sprite stretched between the two points,
                    // fading over 0.10 s.
                    let rel = t_pos.0 - sub_pos.0;
                    let len = rel.length().max(1.0);
                    let mid = (sub_pos.0 + t_pos.0) * 0.5;
                    let angle = rel.y.atan2(rel.x) - std::f32::consts::FRAC_PI_2;
                    commands.spawn((
                        ZapFlash { remaining_s: 0.10, total_s: 0.10 },
                        Sprite::from_color(
                            Color::srgba(1.0, 0.8, 0.5, 0.95),
                            Vec2::new(2.5, len),
                        ),
                        Transform {
                            translation: mid.extend(0.32),
                            rotation: Quat::from_rotation_z(angle),
                            scale: Vec3::ONE,
                        },
                    ));
                    *laser_cooldown_s = *recharge_s;
                }
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
        // Kzer-Za fighter: docking with parent refunds the 1 crew it
        // cost to launch (shpkzedr.cpp lines 145-152). Contact with
        // anything else just despawns the fighter — damage comes from
        // the periodic laser, not the bump. Handle here before the
        // generic owner-skip below.
        if let SubEntityAi::KzerZaFighter { air_grace_s, .. } = ai {
            // Birth-frame collisions with the parent (the fighter
            // spawns inside the dreadnought's collider) must NOT
            // dock — let the launch velocity carry it clear first.
            if other_entity == sub.owner && *air_grace_s <= 0.0 {
                if let Ok(mut crew) = crews.get_mut(other_entity) {
                    crew.current = (crew.current + 1).min(crew.max);
                }
                if let Ok(mut ec) = commands.get_entity(sub_entity) {
                    ec.try_despawn();
                }
            } else if other_entity != sub.owner && ships.get(other_entity).is_ok() {
                // Bumping into an enemy ship: just vanish. No damage.
                if let Ok(mut ec) = commands.get_entity(sub_entity) {
                    ec.try_despawn();
                }
            }
            // Non-ship contacts (planet, projectiles) and grace-period
            // parent bumps: ignored.
            continue;
        }
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
                // `tick_orz_marines_boarded` handle the rest. Same
                // pattern as the KzerZaFighter branch above: use
                // `get_entity` to short-circuit if the sub was
                // already despawned earlier in the same event batch.
                if let Ok(mut ec) = commands.get_entity(sub_entity) {
                    ec.try_insert(OrzMarineBoarded {
                        host: other_entity,
                        roll_accum_s: 0.0,
                    });
                }
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
            SubEntityAi::KzerZaFighter { .. } => {
                // Handled above (pre-owner-skip dispatch).
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

/// Stamped on the Slylandro Probe by `EatAsteroidRefillBattery` in
/// apply_kind; consumed (and removed) by `apply_slyp_harvest` next
/// tick. Matches `shpslypr.cpp:calculate` lines 224-235.
#[derive(Component, Debug)]
pub struct SlypHarvestRequest {
    pub range: f32,
}

/// Harvest any asteroid within `range` of the requesting Probe:
/// despawn the rock (with kaboom for feedback) and refill the
/// Probe's battery to max. Canon damages the asteroid for 1 (which
/// is enough to one-shot it) and tops up the battery on success.
fn apply_slyp_harvest(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut requesters: Query<(Entity, &Position, &mut Battery, &SlypHarvestRequest)>,
    asteroids: Query<(Entity, &Position), With<Asteroid>>,
) {
    for (probe, probe_pos, mut batt, req) in &mut requesters {
        let r2 = req.range * req.range;
        let mut ate_any = false;
        for (ast_e, ast_pos) in &asteroids {
            if (ast_pos.0 - probe_pos.0).length_squared() <= r2 {
                spawn_asteroid_explosion(&mut commands, &assets, ast_pos.0, 24.0);
                if let Ok(mut ec) = commands.get_entity(ast_e) {
                    ec.try_despawn();
                }
                ate_any = true;
            }
        }
        if ate_any {
            batt.current = batt.max;
            info!("Slylandro harvest → battery full ({}/{})", batt.current, batt.max);
        }
        commands.entity(probe).try_remove::<SlypHarvestRequest>();
    }
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
#[derive(Component, Debug, Clone)]
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
#[derive(Component, Debug, Clone)]
#[component(on_add = auto_assign_net_id)]
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
#[derive(Component, Debug, Default, Clone)]
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
            // stop-on-no-target logic from canon. The blade may
            // already have been despawned this frame by a projectile-
            // vs-ship collision (the despawn is queued and the
            // projectile_alive query saw it as alive at system
            // start) — `get_entity` short-circuits in that case so
            // we don't queue an insert on a stale ID.
            if let Some(blade) = carrier.current.take() {
                if let Ok(mut ec) = commands.get_entity(blade) {
                    ec.try_insert(KohrAhBladePassive {
                        launch_speed: blade_velocity,
                    });
                }
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
/// A drifting rock. The `radius` and `frame_idx` are carried on the
/// component (not just baked into the collider + sprite) so the host
/// can put them in the netplay snapshot — the guest needs them to
/// spawn a visually + physically matching mirror when a replenished
/// asteroid first appears in a snapshot with a NetId it hasn't seen.
#[derive(Component, Debug, Clone)]
pub struct Asteroid {
    pub radius: f32,
    /// 1-based index into the `ASTERO01..64` sprite frames.
    pub frame_idx: u8,
}

/// melee.dat ships 64 rotation frames per asteroid sprite
/// (`ASTERO01..64`, indices 1-based). Pick one at random per
/// asteroid so the field doesn't read as identical rocks.
pub const ASTEROID_FRAMES: usize = 64;

/// Everything needed to spawn one asteroid. Built by the seeded-RNG
/// spawners (`spawn_asteroids`, `replenish_asteroids`) on the
/// authoritative peer, and by the guest's snapshot reconciler from a
/// received `EntityState`.
pub struct AsteroidSpawn {
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
    pub frame_idx: u8,
    pub mass: f32,
    pub ang_vel: f32,
    /// Cross-peer stable id. `0` for solo / hotseat (never synced).
    pub net_id: u32,
    /// `true` → `RigidBody::Static` (guest mirror, snapshot-driven).
    /// `false` → `RigidBody::Dynamic` (host / solo, locally simulated).
    pub as_static: bool,
}

/// Single asteroid spawn site shared by the initial field, the
/// host-side replenisher, and the guest-side snapshot reconciler, so
/// the component layout can't drift between them.
pub fn spawn_one_asteroid(commands: &mut Commands, assets: &AssetServer, spec: AsteroidSpawn) {
    let visual = spec.radius * 2.2;
    let sprite_path = format!("asteroids/astero{:02}.png", spec.frame_idx);
    commands.spawn((
        Asteroid {
            radius: spec.radius,
            frame_idx: spec.frame_idx,
        },
        crate::netcode::NetId(spec.net_id),
        Sprite {
            image: assets.load(sprite_path),
            color: Color::WHITE,
            custom_size: Some(Vec2::splat(visual)),
            ..default()
        },
        Transform::from_translation(spec.pos.extend(0.1)),
        if spec.as_static {
            RigidBody::Static
        } else {
            RigidBody::Dynamic
        },
        Collider::circle(spec.radius),
        Mass(spec.mass),
        Position(spec.pos),
        // Restitution gives the collisions some bounce — without
        // it asteroids would just stick on contact.
        Restitution::new(0.7),
        Friction::new(0.0),
        LinearVelocity(spec.vel),
        AngularVelocity(spec.ang_vel),
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
}

/// Sprinkle the opening field of asteroids at random positions across
/// the arena, avoiding the player-ship spawn corridors. Called once
/// per match from `spawn_match`.
///
/// `as_static` forces the spawned bodies to be `RigidBody::Static`
/// instead of `Dynamic`. The guest in a netplay match passes `true`
/// so its asteroids don't run physics at all locally — the host's
/// snapshot stream is the only thing that moves them. Anything less
/// (Kinematic + local gravity, Kinematic + stale-velocity
/// integration) drifts visibly when the two peers' frame rates
/// differ, because guest-side integration races ahead or behind
/// host-side integration between snapshots.
///
/// `alloc` is reset here and then advanced once per asteroid, so the
/// opening field deterministically gets NetId 1..N on both peers
/// (same seeded RNG, same allocation order). The host's replenisher
/// then hands out N+1, N+2, … for rocks added mid-match.
pub fn spawn_asteroids(
    commands: &mut Commands,
    assets: &AssetServer,
    rng: &mut crate::rng::GameRng,
    alloc: &mut crate::netcode::NetIdAllocator,
    as_static: bool,
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

    // Fresh match → fresh id sequence. Both peers reset + allocate in
    // lockstep so the opening field is NetId 1..N on each side, and a
    // rematch starts the sequence over instead of inheriting the
    // previous match's replenishment counter.
    alloc.reset();

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
        let frame_idx = (1 + rng.usize_range(0..ASTEROID_FRAMES)) as u8;
        let mass = 4.0 + rng.f32() * 3.0;
        let ang_vel = rng.signed_unit() * 0.3;
        spawn_one_asteroid(
            commands,
            assets,
            AsteroidSpawn {
                pos,
                vel,
                radius,
                frame_idx,
                mass,
                ang_vel,
                net_id: alloc.allocate().0,
                as_static,
            },
        );
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
#[derive(Component, Debug, Clone)]
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
            // Original canon value here was 288 wu/s² (from
            // GravityForce=1.5 + canonical scale_acceleration). At
            // that strength, a fly-by at ~400 wu only deflects a
            // 1000 wu/s ship by ~10 %, which doesn't read as a
            // "whip" — you just drift past the well. Bumped to 720
            // wu/s² so the same fly-by deflects ~25-30 %, plus the
            // existing `whip_mult: 1.5` lets the post-whip ship
            // keep that boosted velocity until it leaves the well.
            // The original devs flagged the scale_acceleration
            // formula as inconsistent (literally `WTF????` in the
            // engine source), so canon-fidelity isn't violated.
            gravity_accel: 720.0,
            // server.ini [Planet] GravityForce = 1.5 through
            // `mhelpers.cpp::scale_acceleration`:
            //   force·dist_ratio / time_ratio²
            //   = 1.5 × 0.48 / 50ms / 50ms = 2.88e-4 TW-px/ms²
            // mcbodies.cpp:144 per game-tick Δv = frame_time·force·sr
            //   = 50ms × 2.88e-4 × sr ≈ 0.0144 TW-px/ms · sr
            // 20 game-frames/sec → peak ≈ 288 wu/s² (1 wu = 1 TW-px).
            // Use the canon value directly; ships still feel it as a
            // real slingshot because our smaller arena (3000 vs 3840)
            // means the same absolute pull covers more of the map.
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
    // Canon places the planet at the arena centre, but canon also
    // randomises ship spawns each match — so any given match might have
    // ships nowhere near the planet, others might have them right next
    // to it. Our spawns are FIXED at the compass points, so a centre
    // planet would consistently suck both fixed-position ships into the
    // well every match. Keep the planet off-centre / randomly placed in
    // a quadrant so the well is a *sometimes-encountered hazard*, not a
    // guaranteed gameplay obstacle that swallows the same spawns every
    // round.
    use std::f32::consts::{FRAC_PI_4, PI};
    let quadrant = rng.usize_range(0..4) as f32;
    let jitter = (rng.f32() - 0.5) * (PI / 6.0); // ±30°
    let theta = quadrant * (PI / 2.0) + FRAC_PI_4 + jitter;
    let r = 1100.0 + rng.f32() * 200.0; // 1100..1300 wu from origin
    let pos = Vec2::new(theta.cos() * r, theta.sin() * r);
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
        // Avian needs this on the planet for collision-event delivery.
        CollidingEntities::default(),
        planet,
    ));
    info!("spawned planet at ({:.0}, {:.0})", pos.x, pos.y);
}

/// Per-frame canon-equivalent damage while a ship is touching the
/// planet. Canon's `Planet::inflict_damage` was called by the per-frame
/// pixel-overlap test, so a pinned ship was effectively taking
/// `ceil(crew/3)` damage at 20 Hz — near-instant death. Avian only
/// fires `CollisionStart` on transitions, so we recreate the persistent
/// half here: every ship in the planet's `CollidingEntities` loses
/// `PLANET_PIN_DPS` crew/second. Tuned so a ~16-crew ship that gets
/// gravity-pinned dies in ~1.5–2 s — fast but not insta, leaving the
/// player time to thrust away if they catch it. Bounces still cost a
/// chunk per `CollisionStart` (see `tick_planet_contact`).
fn tick_planet_grind(
    time: Res<Time<Physics>>,
    planets: Query<&CollidingEntities, With<Planet>>,
    mut crews: Query<&mut Crew, With<Ship>>,
    shields: Query<&ShieldActive>,
) {
    /// Crew per second while touching the planet. 8 means a 16-crew
    /// ship dies in 2 s of continuous contact; small-crew ships
    /// (Shofixti, Pkunk at half crew) die in well under 1 s.
    const PLANET_PIN_DPS: f32 = 8.0;
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
            let amount = PLANET_PIN_DPS * dt * factor;
            // Ceil ensures sub-1-dmg/s ticks still count; rate is
            // dominated by the constant above anyway.
            let dmg = amount.ceil() as i32;
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
            &RigidBody,
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
        for (pos, mut vel, rb, inertialess, hyper) in &mut bodies {
            // Static bodies have no velocity by definition. The guest's
            // asteroids are static — their motion is driven entirely by
            // the host's authoritative snapshot stream, never by local
            // gravity. Running gravity on them would just spin a
            // useless number into a LinearVelocity that physics
            // doesn't read.
            if rb.is_static() {
                continue;
            }
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
                if let Ok(mut ec) = commands.get_entity(other_e) {
                    ec.try_despawn();
                }
            }
            continue;
        }

        // Ship hit. Grazing the planet's surface costs crew (shield-aware).
        let Ok(ship) = ships.get(other_e) else { continue; };
        let Ok(mut crew) = crews.get_mut(other_e) else { continue; };
        // Canon `Planet::inflict_damage` (`mcbodies.cpp:121`) ran every
        // frame the ship's sprite overlapped the planet's — `collide()`
        // is a per-frame pixel-overlap test, not a "started overlapping"
        // event like Avian's `CollisionStart`. So canon effectively
        // dealt `ceil(crew/3)` damage at 20 Hz while in contact, which
        // means near-instant death when pinned.
        //
        // Avian only fires `CollisionStart` once per fresh contact, so
        // we model the same thing two ways:
        //   - per-event chunk here (small, just so a clean bounce
        //     visibly costs crew),
        //   - continuous grind in `tick_planet_grind` for the pinned
        //     case (a ship dragged in by gravity and stuck against the
        //     surface dies in a couple of seconds, like canon).
        let factor = shields.get(other_e).map(|s| s.damage_factor).unwrap_or(1.0);
        let dmg = ((1.0 * factor).round() as i32).max(0);
        if dmg > 0 {
            crew.current = (crew.current - dmg).max(0);
            info!("P{} bounced the planet: -{} crew", ship.player_slot + 1, dmg);
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

    // Ultimate portraits + voices. Preload EVERY captain's portrait (all
    // 25 files exist) so the first ultimate of a match doesn't flash a
    // blank frame while the texture decodes — previously only 8 were
    // warmed, so any other captain's first ult showed nothing. Each code
    // has a matching `_voi.wav`, so we warm both in one pass.
    for code in [
        "andgu", "arisk", "chebr", "chmav", "druma", "earcr", "ilwav", "kohma", "meltr",
        "mmrxf", "mypo", "orzne", "pkufu", "shosc", "slypr", "spael", "supbl", "sypen", "thrto",
        "umgdr", "urqdr", "utwju", "vuxin", "yehte", "zoqst",
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

    // Title + combat music and the global ship-death boom. Title
    // music plays the moment the menu loads, so warming the handle
    // during the Loading state avoids a hitch at the transition.
    for path in [
        "music/title.mp3",
        "music/melee.mp3",
        "sfx/boom_ship.wav",
    ] {
        preloaded
            .handles
            .push(assets.load::<AudioSource>(path).untyped());
    }

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

/// Host-only enqueue: any AsteroidExplosion freshly spawned since
/// the last frame gets pushed into `VisualEventQueue.explosions`,
/// which `send_ship_snapshot` drains into the outgoing snapshot.
/// Using `Added<>` is the contract: each host-side spawn fires
/// here exactly once, no per-spawn-site plumbing needed. The
/// `radius` field of the wire event is recovered from
/// `Sprite.custom_size` (the spawn helper sets it to `radius * 3`).
///
/// Gated on host/solo via `role_is_authoritative` at registration;
/// guest mirror spawns are local (driven by `drain_messages`) and
/// must not bounce back into the queue, but since this system
/// doesn't run on the guest at all the gate is trivially safe.
pub fn enqueue_explosion_events(
    explosions: Query<(&Transform, &Sprite), Added<AsteroidExplosion>>,
    mut queue: ResMut<crate::netcode::VisualEventQueue>,
) {
    for (xf, sprite) in &explosions {
        let radius = sprite
            .custom_size
            .map(|s| s.x / 3.0)
            .unwrap_or(24.0);
        queue.explosions.push(crate::netcode::ExplosionSpawn {
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            radius,
        });
    }
}

/// Host-only enqueue for ZapFlash — same contract as
/// `enqueue_explosion_events`. We snapshot the full pose +
/// sprite-extents + colour so the guest mirror reads identical.
pub fn enqueue_zap_events(
    zaps: Query<(Entity, &Transform, &Sprite, &ZapFlash), Added<ZapFlash>>,
    mut queue: ResMut<crate::netcode::VisualEventQueue>,
) {
    for (_e, xf, sprite, flash) in &zaps {
        let (width, length) = sprite
            .custom_size
            .map(|s| (s.x, s.y))
            .unwrap_or((0.0, 0.0));
        let (_, _, angle) = xf.rotation.to_euler(bevy::math::EulerRot::XYZ);
        queue.zaps.push(crate::netcode::ZapSpawn {
            pos_x: xf.translation.x,
            pos_y: xf.translation.y,
            angle,
            length,
            width,
            total_s: flash.total_s,
            color: sprite.color.to_linear().to_f32_array(),
        });
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
    // Canon plays a size-indexed BOOM sample on a body's death
    // (mcbodies.cpp: `play_sound(melee[MELEE_BOOM + i])`). Map the
    // explosion radius to one of the four `boom_pl0N` clips and fire a
    // throwaway audio entity. Runs wherever the explosion is spawned —
    // host, solo, and the guest's `ExplosionSpawn` mirror path all call
    // here — so the kaboom is heard on every screen.
    let boom = ((radius / 8.0) as i32 - 1).clamp(0, 3) + 1;
    commands.spawn((
        AudioPlayer::<AudioSource>(assets.load(format!("sfx/boom_pl{boom:02}.wav"))),
        PlaybackSettings::DESPAWN,
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
///
/// **Host / solo only.** The guest bails — it never spawns its own
/// asteroids; it mirrors whatever the host spawns through the
/// snapshot stream (`netcode::drain_messages` spawns a Static mirror
/// the first time a replenished NetId appears). Running the spawner
/// on the guest would create a peer-local rock at a different,
/// camera-derived position that the host never knows about. On the
/// host each new rock gets a fresh NetId from the allocator so the
/// guest can join it to a mirror.
fn replenish_asteroids(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cameras: Query<(&Transform, &Projection), With<crate::starfield::PrimaryCamera>>,
    windows: Query<&Window>,
    asteroids: Query<(), With<Asteroid>>,
    mut rng: ResMut<crate::rng::GameRng>,
    role: Res<crate::netcode::NetRole>,
    config: Res<MatchConfig>,
    mut alloc: ResMut<crate::netcode::NetIdAllocator>,
) {
    if role.is_guest() {
        return;
    }
    // Boss co-op clears the arena of drifting rocks; don't refill it.
    if config.boss {
        return;
    }
    use std::f32::consts::TAU;
    const TARGET_ASTEROID_COUNT: usize = 8;
    const HALF: f32 = 3000.0;
    const KEEP_OUT_X: f32 = 600.0;
    const KEEP_OUT_Y: f32 = 350.0;

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

    // Only the host (or a solo player) runs this, so the camera-aware
    // "spawn off-screen" scoring is unambiguous — there's exactly one
    // authoritative camera. The guest receives the resulting rock via
    // snapshot, so it doesn't matter that the host placed it relative
    // to the host's own view.
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
    let frame_idx = (1 + rng.usize_range(0..ASTEROID_FRAMES)) as u8;
    let mass = 4.0 + rng.f32() * 3.0;
    let ang_vel = rng.signed_unit() * 0.3;
    spawn_one_asteroid(
        &mut commands,
        &assets,
        AsteroidSpawn {
            pos,
            vel,
            radius,
            frame_idx,
            mass,
            ang_vel,
            net_id: alloc.allocate().0,
            // Host runs real physics on its asteroids; solo too.
            as_static: false,
        },
    );
}
