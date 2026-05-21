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
        })
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
];

#[derive(Resource, Debug, Default)]
pub struct ShipCatalog {
    pub ships: HashMap<String, ShipStats>,
}

/// What `spawn_match` should use the next time the scene rebuilds.
/// Mutated by the class-picker keys; read in `spawn_match`.
#[derive(Resource, Debug, Clone, Copy)]
pub struct MatchConfig {
    pub p1_class: ShipClass,
    pub p2_class: ShipClass,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            p1_class: ShipClass::Earcr,
            p2_class: ShipClass::Spael,
        }
    }
}

/// Stable order — picker keys (Digit1..0 for P1, F1..F10 for P2) map to
/// `ALL_CLASSES[i]` by index. Don't reorder existing entries without
/// updating the README key table.
pub const ALL_CLASSES: [ShipClass; 25] = [
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
        }
    }
}

/// Live, mutable crew count for a ship. Decoupled from `ShipStats` (which
/// is static class data) so respawns and replays can reset cleanly.
#[derive(Component, Debug)]
pub struct Crew {
    pub current: i32,
    pub max: i32,
}

/// Live battery state. Spent by firing (`WeaponDrain`) and triggering
/// specials (`SpecialDrain`); regenerates via `RechargeTimer` per the
/// ship's `.ini` `RechargeAmount` + `RechargeRate`. Like `Crew`, this
/// is *runtime* state, not a class stat.
#[derive(Component, Debug)]
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
    pub remaining_frames: i32,
}

/// Time (in seconds) until the ship's primary weapon can fire again.
#[derive(Component, Debug, Default)]
pub struct WeaponCooldown(pub f32);

/// Time (in seconds) until the ship's special ability can be used again.
#[derive(Component, Debug, Default)]
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

        // For a constant thrust to stabilise at speed_max:
        //   F = damping · m · v   →   damping = F / (m · v) = a / v
        let linear_damping = if speed_max > 0.0 { accel / speed_max } else { 0.0 };
        let thrust_force = stats.mass * accel;

        // Disk moment of inertia: I = ½ m r²
        let inertia = 0.5 * stats.mass * collider_radius * collider_radius;

        // Inertial-mode tuning. We pick a per-class rise-time that grows
        // with TurnRate so nimble ships feel sharp and bulky ones lumber.
        // K_p ≈ 3·I / rise_time gives ~95% of target omega after rise_time.
        let rise_time = 0.15 * (stats.turn_rate + 1.0); // Earcr (TR=1) → 0.30 s
        let inertial_torque_gain = 3.0 * inertia / rise_time;

        // Angular damping: bleed-off rate when no input. Same inverse
        // relationship with TurnRate as the controller — nimble ships
        // are also better at self-righting once player releases input.
        let angular_damping = 5.0 / (stats.turn_rate + 1.0);

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
    /// Damage applied to each enemy ship still in range each tick.
    /// Enemy *projectiles* in range are unconditionally despawned.
    pub damage_per_tick: i32,
}

/// In-flight projectile. Owner is tracked so we can ignore self-hits.
#[derive(Component, Debug)]
pub struct Projectile {
    pub owner: Entity,
    pub damage: i32,
    pub lifetime: f32,
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
pub struct DamageZone {
    pub radius: f32,
    pub damage_per_sec: f32,
    pub lifetime: f32,
    pub source: Option<Entity>,
}

/// Spawns a damage zone as a sprite entity (no rigid body — pure
/// gameplay marker). The sprite is a semi-transparent filled square
/// the diameter of the zone; replace with a proper circle-outline
/// shader in M7 polish.
pub(crate) fn spawn_damage_zone(
    commands: &mut Commands,
    source: Option<Entity>,
    pos: Vec2,
    radius: f32,
    damage_per_sec: f32,
    lifetime: f32,
    color: Color,
) {
    commands.spawn((
        DamageZone {
            radius,
            damage_per_sec,
            lifetime,
            source,
        },
        Sprite::from_color(color, Vec2::splat(radius * 2.0)),
        Transform::from_translation(pos.extend(0.2)),
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
    local_offset: Vec2,
    radius: f32,
    damage_per_sec: f32,
    lifetime: f32,
    color: Color,
) {
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
        Transform::from_translation(Vec3::ZERO),
    ));
}

/// All 64 rotation frames preloaded so the renderer can pick by heading
/// without hitting the asset server hot path.
#[derive(Component)]
pub struct ShipFrames {
    pub frames: Vec<Handle<Image>>,
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
        app.add_systems(
            FixedUpdate,
            (
                apply_player_input,
                tick_weapon_cooldown,
                tick_special_cooldown,
                tick_shield,
                tick_point_defense,
                tick_battery_recharge,
                tick_projectile_lifetime,
                steer_homing_projectiles,
                orient_projectiles,
                tick_damage_zones,
                tick_attached_damage_zones,
                tick_beams,
                handle_projectile_hits,
                handle_ship_collisions,
            ),
        )
        .add_systems(Update, swap_rotation_frame);
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
) {
    // Rotation convention: 0 rad = ship facing +Y (up); positive
    // rotation is CCW. To face right (+X) we want -π/2 (CW 90°), and
    // to face left (-X) we want +π/2. P1 sits on the left and faces
    // right at the enemy; P2 sits on the right and faces left.
    spawn_class(
        &mut commands,
        &catalog,
        &assets,
        config.p1_class,
        Vec2::new(-300.0, 0.0),
        -std::f32::consts::FRAC_PI_2,
        0,
    );
    spawn_class(
        &mut commands,
        &catalog,
        &assets,
        config.p2_class,
        Vec2::new(300.0, 0.0),
        std::f32::consts::FRAC_PI_2,
        1,
    );
}

/// Class-picker hotkeys. Each player has 10 key slots (digits for P1,
/// F-keys for P2) plus modifier-based bank selection: no modifier picks
/// classes 0..9, `Shift` picks 10..19, `Ctrl` picks 20..24. Changes
/// apply on the next rematch.
fn class_picker_input(keys: Res<ButtonInput<KeyCode>>, mut config: ResMut<MatchConfig>) {
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

    for (i, key) in P1_DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let Some(class) = ALL_CLASSES.get(idx).copied() {
                config.p1_class = class;
                info!("P1 → {:?} (takes effect next rematch)", config.p1_class);
            }
        }
    }
    for (i, key) in P2_FKEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let Some(class) = ALL_CLASSES.get(idx).copied() {
                config.p2_class = class;
                info!("P2 → {:?} (takes effect next rematch)", config.p2_class);
            }
        }
    }
}

/// Despawn every gameplay entity from the previous round so the next
/// OnEnter(InMatch) can spawn a clean scene. Camera, HUD nodes, and
/// the catalog resource survive.
pub fn teardown_match(
    mut commands: Commands,
    ships: Query<Entity, With<Ship>>,
    projectiles: Query<Entity, With<Projectile>>,
    damage_zones: Query<Entity, With<DamageZone>>,
) {
    for e in &ships {
        commands.entity(e).despawn();
    }
    for e in &projectiles {
        commands.entity(e).despawn();
    }
    for e in &damage_zones {
        commands.entity(e).despawn();
    }
}

fn spawn_class(
    commands: &mut Commands,
    catalog: &ShipCatalog,
    assets: &AssetServer,
    class: ShipClass,
    position: Vec2,
    rotation_rad: f32,
    slot: usize,
) {
    let code = class.code();
    let Some(stats) = catalog.ships.get(code).cloned() else {
        error!("{code} missing from catalog");
        return;
    };
    let frames = load_rotation_frames(assets, code);
    if frames.is_empty() {
        error!("no rotation frames found for {code}");
        return;
    }
    spawn_ship(commands, class, &stats, &frames, position, rotation_rad, slot);
    info!("spawned P{} as {}", slot + 1, stats.name);
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
    class: ShipClass,
    stats: &ShipStats,
    frames: &[Handle<Image>],
    position: Vec2,
    rotation_rad: f32,
    slot: usize,
) {
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
            current: stats.crew_max,
            max: stats.crew_max,
        },
        Battery {
            current: stats.batt_max,
            max: stats.batt_max,
        },
        RechargeTimer {
            remaining_frames: stats.recharge_rate,
        },
        WeaponCooldown::default(),
        SpecialCooldown::default(),
        ShipFrames {
            frames: frames.to_vec(),
        },
        derived,
    );
    let visual = (
        Sprite::from_image(initial),
        Transform::from_translation(position.extend(0.0)),
    );
    let physics = (
        RigidBody::Dynamic,
        Collider::circle(phys.collider_radius),
        Mass(stats.mass),
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
    // Attach the data-driven ability manifest. Every class has one
    // today; the dispatcher in `src/ability.rs` reads it and produces
    // the right ECS spawns.
    if let Some(abilities) = abilities_for(class) {
        entity.insert(abilities);
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
                    // .ini Special Frames=500 → 25 s at 20 Hz.
                    duration_s: 500.0 / 20.0,
                    damage_factor: 0.0,
                },
                // SpecialRate=2 → 2/20 = 0.1 s.
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
                kind: AbilityKind::Todo { ident: "Chmmr tractor beam (shpchmav.cpp:87)" },
                cooldown_s: 2.0,
            },
        }),

        // Ur-Quan Kzer-Za Dreadnought — fusion bolt + launch fighters.
        // Fighters need SubEntity primitive (TODO shpkzedr.cpp:55).
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
                kind: AbilityKind::Todo { ident: "Kzer-Za fighters (shpkzedr.cpp:55)" },
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
                // .ini Special Range=13 → 520 u, Damage=20, Scale=2.5.
                // Canonical: 3 presses → blast everything in range and
                // damage self by 999. We don't have the 3-press
                // confirmation yet (TODO: shpshosc.cpp:52); for now,
                // single-press → instant blast.
                kind: AbilityKind::SpawnDamageZone {
                    offset: Vec2::ZERO,
                    radius: 13.0 * SC2_RANGE_SCALE,
                    damage_per_sec: 1_000_000.0,
                    duration_s: 0.1,
                    source_self: false,
                    color: Color::srgba(1.0, 0.6, 0.2, 0.55),
                },
                cooldown_s: 999.0,
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
                kind: AbilityKind::RefillBattery,
                cooldown_s: 16.0 / 20.0,
            },
        }),

        // Ilwrath Avenger — short-range hit + cloak. Real cloak
        // (invisibility) needs InvisibleTo primitive — TODO; the
        // placeholder is a brief shield, matching the prior behaviour.
        ShipClass::Ilwav => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 30.0),
                    random_spread_rad: 0.0,
                    speed: 28.0 * SC2_VEL_SCALE,
                    lifetime: (2.8 * SC2_RANGE_SCALE) / (28.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 0.6, 0.3),
                    sprite_size: 8.0,
                    sprite_path: Some("ships/ilwav/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::GrantShield { duration_s: 2.5, damage_factor: 0.0 },
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
                    // ThraddashFlame: Damage=2, Frames=39 ≈ 3.9 s.
                    AbilityKind::SpawnDamageZone {
                        offset: Vec2::new(0.0, -18.0),
                        radius: 18.0,
                        damage_per_sec: 8.0,
                        duration_s: 3.9,
                        source_self: true,
                        color: Color::srgba(1.0, 0.5, 0.1, 0.5),
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
                kind: AbilityKind::Todo { ident: "Supox held-strafe (shpsupbl.cpp:calculate_thrust)" },
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
                fried_barrels.push(Barrel { local_pos: dir * 16.0, direction: dir });
            }
            Some(ShipAbilities {
                primary: AbilitySpec {
                    kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                        barrels: single_barrel(forward, 34.0),
                        random_spread_rad: 0.0,
                        speed: 64.0 * SC2_VEL_SCALE,
                        lifetime: (12.0 * SC2_RANGE_SCALE) / (64.0 * SC2_VEL_SCALE),
                        color: Color::srgb(1.0, 1.0, 1.0),
                        sprite_size: 12.0,
                        sprite_path: Some("ships/kohma/sprites/shot_a01.png".into()),
                        homing_turn_rate: 0.0,
                        is_limpet: false,
                        recoil_impulse: 0.0,
                    }]},
                    cooldown_s: 6.0 / 20.0,
                },
                special: AbilitySpec {
                    kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                        barrels: fried_barrels,
                        random_spread_rad: 0.0,
                        speed: fried_speed,
                        lifetime: fried_life,
                        color: Color::srgb(1.0, 1.0, 1.0),
                        sprite_size: 8.0,
                        sprite_path: Some("ships/kohma/sprites/shot_b01.png".into()),
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
                kind: AbilityKind::Sequence(vec![
                    AbilityKind::BrakeImpulse { max_dv: 8000.0 / 10.0 }, // ≈ 800 m/s on mass 10
                    AbilityKind::Todo { ident: "Syreen siren song (shpsyrpe.cpp:661)" },
                ]),
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
                kind: AbilityKind::Todo { ident: "Androsynth Blazer mode (shpandgu.cpp:906)" },
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Chenjesu Broodhome — crystal (TODO shatter-on-release) +
        // DOGI sub-entity (TODO).
        ShipClass::Chebr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 32.0),
                    random_spread_rad: 0.0,
                    speed: 64.0 * SC2_VEL_SCALE,
                    lifetime: 4.0,
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 14.0,
                    sprite_path: Some("ships/chebr/sprites/shot_a_01_tga.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::Todo { ident: "Chenjesu DOGI (shpchebr.cpp:1017)" },
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
                // Real fortitude turns damage into battery — needs
                // damage-to-battery primitive (TODO shputwju.cpp:96).
                kind: AbilityKind::GrantShield { duration_s: 2.0, damage_factor: 0.0 },
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
                kind: AbilityKind::Todo { ident: "Mmrnmhrm T/Y form toggle (shpmmrxf.cpp:435)" },
                cooldown_s: 1.0 / 20.0,
            },
        }),

        // Orz Nemesis — turret cannon + marines. Turret aim + marine
        // sub-entity both TODO (shporzne.cpp:549, 564).
        ShipClass::Orzne => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 28.0),
                    random_spread_rad: 0.0,
                    speed: 120.0 * SC2_VEL_SCALE,
                    lifetime: (20.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 8.0,
                    sprite_path: Some("ships/orzne/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 4.0 / 20.0,
            },
            special: AbilitySpec {
                // Marine spawning costs 1 crew in canon — model the
                // cost; the sub-entity AI is the missing piece.
                kind: AbilityKind::Sequence(vec![
                    AbilityKind::ModifyCrew { delta: -1 },
                    AbilityKind::Todo { ident: "Orz marine sub-entity (shporzne.cpp:564)" },
                ]),
                cooldown_s: 12.0 / 20.0,
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
        ShipClass::Meltr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 28.0),
                    random_spread_rad: 0.0,
                    speed: 112.0 * SC2_VEL_SCALE,
                    lifetime: (21.0 * SC2_RANGE_SCALE) / (112.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 10.0,
                    sprite_path: Some("ships/meltr/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
                }]},
                cooldown_s: 1.0 / 20.0,
            },
            special: AbilitySpec {
                kind: AbilityKind::Todo { ident: "Melnorme confusion ray (shpmeltr.cpp:1215)" },
                cooldown_s: 20.0 / 20.0,
            },
        }),
    }
}

fn load_rotation_frames(assets: &AssetServer, code: &str) -> Vec<Handle<Image>> {
    // Every salvaged ship has 64 rotation frames named ship_s01..ship_s64
    // (VUX is the lone exception — its rotation uses ship_x## with a
    // _bmp suffix; left as-is for now, VUX will look static until we
    // ship a per-class sprite-prefix lookup). Issuing 64 loads
    // unconditionally avoids needing `std::fs::exists`, which doesn't
    // work in the browser sandbox. AssetServer.load() is fire-and-
    // forget on both native and WASM: missing files just don't render.
    let mut frames = Vec::with_capacity(64);
    for i in 1..=64 {
        let name = format!("ship_s{i:02}.png");
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
fn apply_player_input(
    keys: Res<ButtonInput<KeyCode>>,
    angular_override: Res<AngularControlOverride>,
    mut q: Query<(
        &Ship,
        &ShipClass,
        &ShipPhysicsDerived,
        &mut ConstantLocalForce,
        &mut ConstantTorque,
        &mut AngularVelocity,
    )>,
) {
    for (ship, class, derived, mut thrust, mut torque, mut ang_vel) in &mut q {
        let input = input::read_local_input(&keys, ship.player_slot);

        let dir = if input.pressed(input::INPUT_LEFT) {
            1.0
        } else if input.pressed(input::INPUT_RIGHT) {
            -1.0
        } else {
            0.0
        };
        let target_omega = dir * derived.target_omega;

        let mode = angular_override
            .0
            .unwrap_or_else(|| physics_spec(*class).angular_control);

        match mode {
            AngularControl::Classic => {
                // Snap to commanded rate; ignore impulses (SC2 default).
                ang_vel.0 = target_omega;
                torque.0 = 0.0;
            }
            AngularControl::Inertial => {
                // Proportional torque toward target. Gain is per-ship —
                // agile ships have a higher gain *relative to* their
                // smaller moment of inertia, so they snap back faster.
                let error = target_omega - ang_vel.0;
                torque.0 = error * derived.inertial_torque_gain;
            }
        }

        thrust.0 = if input.pressed(input::INPUT_THRUST) {
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
fn swap_rotation_frame(mut q: Query<(&Rotation, &ShipFrames, &mut Sprite, &mut Transform)>) {
    for (rot, frames, mut sprite, mut transform) in &mut q {
        if frames.frames.is_empty() {
            continue;
        }
        let angle = rot.sin.atan2(rot.cos);
        let n = frames.frames.len() as f32;
        let mut idx = ((-angle) / std::f32::consts::TAU * n).rem_euclid(n) as usize;
        if idx >= frames.frames.len() {
            idx = 0;
        }
        sprite.image = frames.frames[idx].clone();
        // Cancel Avian's rotation sync on this sprite — the chosen
        // frame already encodes the rotation visually.
        transform.rotation = Quat::IDENTITY;
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
    let dt_frames = time.delta_secs() / 0.050;
    for (ship, mut battery, mut timer) in &mut q {
        if ship.stats.recharge_rate <= 0 || ship.stats.recharge_amount <= 0 {
            continue;
        }
        // RechargeTimer is integer frames, but dt may not align exactly;
        // accumulate by subtracting and topping up when we cross zero.
        timer.remaining_frames -= dt_frames.round() as i32;
        while timer.remaining_frames <= 0 {
            battery.current = (battery.current + ship.stats.recharge_amount).min(battery.max);
            timer.remaining_frames += ship.stats.recharge_rate;
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
}

/// Spawn a beam entity owned by `owner`. The beam follows the owner
/// each tick (its world pose is recomputed from the owner's transform),
/// damages the nearest non-friendly ship along its ray, and despawns
/// when `duration_s` elapses.
pub(crate) fn spawn_beam(
    commands: &mut Commands,
    owner: Entity,
    local_origin: Vec2,
    local_dir: Vec2,
    range: f32,
    damage_per_tick: i32,
    color: Color,
    auto_aim: bool,
    duration_s: f32,
    width: f32,
) {
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
        },
        // Sprite is sized/positioned each tick by `tick_beams`; this
        // initial transform is just so it has a place to start.
        Sprite::from_color(color, Vec2::new(width * 2.0, range)),
        Transform::from_translation(Vec3::ZERO),
    ));
}

/// Marker on a projectile spawned from a VUX-style limpet weapon. When
/// such a projectile hits a non-owner ship, that ship's Avian Mass is
/// incremented by `LIMPET_MASS` and the projectile despawns. No joint,
/// no parent-child reparenting — the slowing is emergent from giving
/// the same thruster force more mass to push.
#[derive(Component, Debug)]
pub struct Limpet;

/// How much each limpet adds to the target's mass (kg). Tunable; canonical
/// SC2 limpets are bulkier than the small fast projectiles I have today,
/// so 3 kg per stick is fairly aggressive — three of them on a Spathi
/// (mass 16 kg) is a 60 % heavier ship.
pub const LIMPET_MASS: f32 = 3.0;

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
const SC2_VEL_SCALE: f32 = 9.6;
const SC2_RANGE_SCALE: f32 = 40.0;
fn sc2_turning(t: f32) -> f32 {
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
            commands.entity(entity).despawn();
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
    ships: Query<(Entity, &Ship, &Position), Without<Projectile>>,
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
                let d2 = (p.0 - proj_pos.0).length_squared();
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

        let to_target = target_pos.0 - proj_pos.0;
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

/// Apply damage from every active `DamageZone` to every ship inside its
/// radius (excluding the zone's `source`, when set), then decrement
/// lifetimes and despawn expired zones. Shield damage_factor still
/// applies — a Pkunk in phase shift takes 0 from a Shofixti Glory.
fn tick_damage_zones(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut zones: Query<(Entity, &Transform, &mut DamageZone)>,
    mut ships: Query<(Entity, &Position, &mut Crew), With<Ship>>,
    shields: Query<&ShieldActive>,
) {
    let dt = time.delta_secs();
    for (zone_entity, zone_tf, mut zone) in &mut zones {
        if zone.damage_per_sec > 0.0 && dt > 0.0 {
            let zone_pos = zone_tf.translation.truncate();
            let r2 = zone.radius * zone.radius;
            for (ship_e, ship_pos, mut crew) in &mut ships {
                if zone.source == Some(ship_e) {
                    continue;
                }
                if (ship_pos.0 - zone_pos).length_squared() > r2 {
                    continue;
                }
                let factor = shields
                    .get(ship_e)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = (zone.damage_per_sec * dt * factor).ceil().max(0.0) as i32;
                if dmg > 0 {
                    crew.current = (crew.current - dmg).max(0);
                }
            }
        }
        zone.lifetime -= dt;
        if zone.lifetime <= 0.0 {
            commands.entity(zone_entity).despawn();
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
    limpets: Query<&Limpet>,
    shields: Query<&ShieldActive>,
    mut crews: Query<&mut Crew>,
    mut masses: Query<&mut Mass>,
) {
    for event in reader.read() {
        let (proj_entity, other_entity) = if projectiles.get(event.collider1).is_ok() {
            (event.collider1, event.collider2)
        } else if projectiles.get(event.collider2).is_ok() {
            (event.collider2, event.collider1)
        } else {
            continue;
        };

        let proj = match projectiles.get(proj_entity) {
            Ok(p) => p,
            Err(_) => continue,
        };

        if proj.owner == other_entity {
            continue;
        }

        // Damage bookkeeping (shield-aware).
        if let Ok(mut crew) = crews.get_mut(other_entity) {
            let factor = shields
                .get(other_entity)
                .map(|s| s.damage_factor)
                .unwrap_or(1.0);
            let damage = ((proj.damage as f32 * factor).round() as i32).max(0);
            crew.current = (crew.current - damage).max(0);
            info!(
                "hit: -{} crew (now {}/{}){}",
                damage,
                crew.current,
                crew.max,
                if factor < 1.0 { " [shielded]" } else { "" }
            );
        }

        // Limpet attach: instead of "just despawn", the limpet's mass
        // is transferred onto the target ship before the projectile
        // dies. Avian recomputes thrust/acceleration response from
        // the new Mass next tick, so the target accelerates more
        // slowly *and* its terminal speed at full throttle drops
        // (terminal_v = thrust / (mass · damping)). Stack hits → more
        // mass → progressively immobilised target. No special-case
        // "slow effect" timer needed; it's just heavier.
        if limpets.get(proj_entity).is_ok() {
            if let Ok(mut mass) = masses.get_mut(other_entity) {
                mass.0 += LIMPET_MASS;
                info!(
                    "limpet stuck: target mass now {:.1} kg",
                    mass.0
                );
            }
        }
        commands.entity(proj_entity).despawn();
    }
}

/// Ship-on-ship contact damage. Avian has already applied the impulse
/// (so the bigger ship pushes the smaller one and they both potentially
/// spin); on top of that we deduct crew proportional to how fast they
/// were closing. Below a threshold relative speed it's just a love tap,
/// no damage.
fn handle_ship_collisions(
    mut reader: MessageReader<CollisionStart>,
    mut q: Query<(&LinearVelocity, &mut Crew), With<Ship>>,
    shields: Query<&ShieldActive>,
) {
    const RAM_DAMAGE_THRESHOLD: f32 = 80.0;
    const RAM_DAMAGE_SCALE: f32 = 60.0;

    for event in reader.read() {
        let Ok([(v1, _), (v2, _)]) = q.get_many([event.collider1, event.collider2]) else {
            continue;
        };
        let rel_speed = (v1.0 - v2.0).length();
        if rel_speed < RAM_DAMAGE_THRESHOLD {
            continue;
        }
        let base = ((rel_speed - RAM_DAMAGE_THRESHOLD) / RAM_DAMAGE_SCALE)
            .ceil()
            .max(1.0);

        for entity in [event.collider1, event.collider2] {
            let factor = shields
                .get(entity)
                .map(|s| s.damage_factor)
                .unwrap_or(1.0);
            let dmg = ((base * factor).round() as i32).max(0);
            if let Ok((_, mut crew)) = q.get_mut(entity) {
                crew.current = (crew.current - dmg).max(0);
                info!(
                    "ram: -{dmg} crew (now {}/{}){}",
                    crew.current,
                    crew.max,
                    if factor < 1.0 { " [shielded]" } else { "" }
                );
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
            commands.entity(entity).remove::<ShieldActive>();
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
    mut firers: Query<(Entity, &Ship, &Position, &mut PointDefenseActive)>,
    projectiles: Query<(Entity, &Position, &Projectile)>,
    mut ships: Query<(Entity, &Ship, &Position, &mut Crew), Without<PointDefenseActive>>,
    shields: Query<&ShieldActive>,
) {
    let dt = time.delta_secs();
    for (firer_entity, firer, firer_pos, mut beam) in &mut firers {
        let r2 = beam.range * beam.range;

        for (proj_entity, proj_pos, proj) in &projectiles {
            if (proj_pos.0 - firer_pos.0).length_squared() > r2 {
                continue;
            }
            // Don't blow up our own outgoing missiles.
            if proj.owner == firer_entity {
                continue;
            }
            commands.entity(proj_entity).despawn();
        }

        if beam.damage_per_tick > 0 {
            for (ship_entity, ship, ship_pos, mut crew) in &mut ships {
                if ship.player_slot == firer.player_slot {
                    continue;
                }
                if (ship_pos.0 - firer_pos.0).length_squared() > r2 {
                    continue;
                }
                let factor = shields
                    .get(ship_entity)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = ((beam.damage_per_tick as f32 * factor).round() as i32).max(0);
                if dmg > 0 {
                    crew.current = (crew.current - dmg).max(0);
                }
            }
        }

        beam.remaining -= dt;
        if beam.remaining <= 0.0 {
            commands
                .entity(firer_entity)
                .remove::<PointDefenseActive>();
            info!("P{} point defense offline", firer.player_slot + 1);
        }
    }
}

/// Reposition each attached zone to follow its owner, then apply
/// damage to any non-friendly ship inside it. Zones whose owner has
/// died despawn (no orphan damage continuing in dead space).
fn tick_attached_damage_zones(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut zones: Query<(Entity, &mut AttachedDamageZone, &mut Transform, &mut Sprite)>,
    owners: Query<(&Ship, &Position, &Rotation)>,
    mut ships: Query<(Entity, &Ship, &Position, &mut Crew)>,
    shields: Query<&ShieldActive>,
) {
    let dt = time.delta_secs();
    for (zone_entity, mut zone, mut zone_xf, mut zone_sprite) in &mut zones {
        let Ok((owner_ship, owner_pos, owner_rot)) = owners.get(zone.owner) else {
            commands.entity(zone_entity).despawn();
            continue;
        };
        // Ship-local offset → world.
        let world_offset = Vec2::new(
            zone.local_offset.x * owner_rot.cos - zone.local_offset.y * owner_rot.sin,
            zone.local_offset.x * owner_rot.sin + zone.local_offset.y * owner_rot.cos,
        );
        let zone_pos = owner_pos.0 + world_offset;
        zone_xf.translation = zone_pos.extend(0.2);
        // Keep sprite size in sync (radius can change frame-to-frame
        // in the future without re-spawning the entity).
        zone_sprite.custom_size = Some(Vec2::splat(zone.radius * 2.0));
        zone_sprite.color = zone.color;

        if zone.damage_per_sec > 0.0 && dt > 0.0 {
            let r2 = zone.radius * zone.radius;
            for (ship_e, ship, ship_pos, mut crew) in &mut ships {
                // Don't damage the owner with its own attached zone.
                if ship.player_slot == owner_ship.player_slot {
                    continue;
                }
                if (ship_pos.0 - zone_pos).length_squared() > r2 {
                    continue;
                }
                let factor = shields
                    .get(ship_e)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = (zone.damage_per_sec * dt * factor).ceil().max(0.0) as i32;
                if dmg > 0 {
                    crew.current = (crew.current - dmg).max(0);
                }
            }
        }

        zone.lifetime -= dt;
        if zone.lifetime <= 0.0 {
            commands.entity(zone_entity).despawn();
        }
    }
}

/// Cast each beam, damage what it hits, position the sprite so it
/// visibly connects owner → hit point (or owner → full range when it
/// misses). Beams whose owner has died despawn cleanly.
fn tick_beams(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut beams: Query<(Entity, &mut Beam, &mut Transform, &mut Sprite)>,
    owners: Query<(&Ship, &Position, &Rotation)>,
    mut ships: Query<(Entity, &Ship, &Position, &mut Crew)>,
    shields: Query<&ShieldActive>,
) {
    let dt = time.delta_secs();
    for (beam_entity, mut beam, mut beam_xf, mut beam_sprite) in &mut beams {
        // Owner gone → beam goes with it.
        let Ok((owner_ship, owner_pos, owner_rot)) = owners.get(beam.owner) else {
            commands.entity(beam_entity).despawn();
            continue;
        };
        // Rotate local origin and direction into world space.
        let world_origin = owner_pos.0
            + Vec2::new(
                beam.local_origin.x * owner_rot.cos - beam.local_origin.y * owner_rot.sin,
                beam.local_origin.x * owner_rot.sin + beam.local_origin.y * owner_rot.cos,
            );
        let mut world_dir = Vec2::new(
            beam.local_dir.x * owner_rot.cos - beam.local_dir.y * owner_rot.sin,
            beam.local_dir.x * owner_rot.sin + beam.local_dir.y * owner_rot.cos,
        );

        // Auto-aim: swing the beam to point at the nearest enemy in
        // range. Canonical Arilou auto-target (shparisk.cpp:78).
        if beam.auto_aim {
            let mut best: Option<(Entity, Vec2, f32)> = None;
            for (e, s, p, _) in &ships {
                if s.player_slot == owner_ship.player_slot {
                    continue;
                }
                let d2 = (p.0 - world_origin).length_squared();
                if d2 > beam.range * beam.range {
                    continue;
                }
                if best.map_or(true, |(_, _, b)| d2 < b) {
                    best = Some((e, p.0, d2));
                }
            }
            if let Some((_, target, _)) = best {
                let delta = target - world_origin;
                if delta.length_squared() > 1e-6 {
                    world_dir = delta.normalize();
                }
            }
        }

        // Cast: find the nearest enemy whose centre is within
        // `beam.width` of the ray, with ray-parameter t ∈ [0, range].
        // Cheap projection: t = (p - o) · dir; perp = |(p - o) - t·dir|.
        let mut hit_t = beam.range;
        let mut hit_target: Option<Entity> = None;
        for (e, s, p, _) in &ships {
            if s.player_slot == owner_ship.player_slot {
                continue;
            }
            let to_ship = p.0 - world_origin;
            let t = to_ship.dot(world_dir);
            if t < 0.0 || t > beam.range {
                continue;
            }
            let perp = to_ship - world_dir * t;
            // Use a generous hit thickness so a beam visibly grazing a
            // ship registers. The ship's collider radius is 12–34;
            // anything within `beam.width + 18` reads as "the beam
            // touched the hull".
            if perp.length_squared() > (beam.width + 18.0).powi(2) {
                continue;
            }
            if t < hit_t {
                hit_t = t;
                hit_target = Some(e);
            }
        }

        // Apply damage to the nearest target (shield-aware).
        if let Some(target) = hit_target {
            if let Ok((_, _, _, mut crew)) = ships.get_mut(target) {
                let factor = shields
                    .get(target)
                    .map(|s| s.damage_factor)
                    .unwrap_or(1.0);
                let dmg = ((beam.damage_per_tick as f32 * factor).round() as i32).max(0);
                if dmg > 0 {
                    crew.current = (crew.current - dmg).max(0);
                }
            }
        }

        // Position the sprite to span owner → hit (or full range when
        // missing). Sprite default y-up axis, custom_size = (w, len)
        // means we rotate by atan2(dy, dx) - π/2 to align long-axis
        // with the world direction.
        let midpoint = world_origin + world_dir * (hit_t * 0.5);
        let angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;
        beam_xf.translation = midpoint.extend(0.3);
        beam_xf.rotation = Quat::from_rotation_z(angle);
        beam_sprite.custom_size = Some(Vec2::new(beam.width * 2.0, hit_t.max(1.0)));
        beam_sprite.color = beam.color;

        beam.remaining -= dt;
        if beam.remaining <= 0.0 {
            commands.entity(beam_entity).despawn();
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
