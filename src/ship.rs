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
    /// Seconds until the next `recharge_amount` is added to battery.
    /// Kept in seconds (not SC2 frames) so the FixedUpdate dt
    /// integrates naturally — the previous `remaining_frames: i32`
    /// arithmetic rounded a 60 Hz step's `0.33 frames` to zero each
    /// tick, so the timer never decremented and batteries never
    /// regenerated.
    pub remaining_s: f32,
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
        // Bevy 0.18's `add_systems` macro caps a single tuple at 20
        // entries. We've outgrown it; split into two FixedUpdate
        // groups (the order across groups is unconstrained, but each
        // system inside this plugin is independent so that's fine).
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
                tick_battery_recharge,
                tick_projectile_lifetime,
                steer_homing_projectiles,
                orient_projectiles,
                tick_damage_zones,
                tick_attached_damage_zones,
                tick_beams,
                tick_tractors,
            ),
        );
        app.add_systems(
            FixedUpdate,
            (
                tick_invisible,
                tick_damage_to_battery,
                tick_sub_entities,
                handle_projectile_hits,
                handle_sub_entity_collisions,
            ),
        )
        .add_systems(Update, (swap_rotation_frame, update_overlay_sprites));
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
    info!("  P1 — Arrow keys (turn / thrust),  Z fire,  X special");
    info!("  P2 — W A S D    (thrust / turn),  G fire,  H special");
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
        &ship_colliders,
    );
    spawn_class(
        &mut commands,
        &catalog,
        &assets,
        config.p2_class,
        Vec2::new(300.0, 0.0),
        std::f32::consts::FRAC_PI_2,
        1,
        &ship_colliders,
    );
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
fn class_picker_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<MatchConfig>,
    mut next_state: ResMut<NextState<crate::AppState>>,
    current_state: Res<State<crate::AppState>>,
) {
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

    for (i, key) in P1_DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let Some(class) = ALL_CLASSES.get(idx).copied() {
                if config.p1_class != class {
                    config.p1_class = class;
                    info!("P1 → {:?}", config.p1_class);
                    changed = true;
                }
            }
        }
    }
    for (i, key) in P2_FKEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            let idx = bank_offset + i;
            if let Some(class) = ALL_CLASSES.get(idx).copied() {
                if config.p2_class != class {
                    config.p2_class = class;
                    info!("P2 → {:?}", config.p2_class);
                    changed = true;
                }
            }
        }
    }

    // Cycling hotkeys — quick way to walk the roster mid-match.
    if keys.just_pressed(KeyCode::Tab) {
        let dir: i32 = if shift { -1 } else { 1 };
        config.p1_class = cycle_class(config.p1_class, dir);
        info!("P1 → {:?}", config.p1_class);
        changed = true;
    }
    if keys.just_pressed(KeyCode::Backquote) {
        let dir: i32 = if shift { -1 } else { 1 };
        config.p2_class = cycle_class(config.p2_class, dir);
        info!("P2 → {:?}", config.p2_class);
        changed = true;
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
    for e in &attached_zones {
        commands.entity(e).despawn();
    }
    for e in &beams {
        commands.entity(e).despawn();
    }
    for e in &tractors {
        commands.entity(e).despawn();
    }
    for e in &sub_entities {
        commands.entity(e).despawn();
    }
    for e in &overlays {
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
    ship_colliders: &crate::collider::ShipColliders,
) {
    let code = class.code();
    let Some(stats) = catalog.ships.get(code).cloned() else {
        error!("{code} missing from catalog");
        return;
    };
    let frames = load_rotation_frames(assets, code, class);
    if frames.is_empty() {
        error!("no rotation frames found for {code}");
        return;
    }
    spawn_ship(
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
            // RechargeRate is in SC2 frames; convert to seconds for
            // FixedUpdate integration (50 ms per SC2 frame).
            remaining_s: stats.recharge_rate as f32 * 0.050,
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
                    cooldown_s: 1.0 / 20.0,
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
                // shpsyrpe.cpp activate_special: damage enemy crew in
                // specialRange, spawn that many CrewPod sub-entities
                // around them. Friendly Syreen ships can collect the
                // pods to add to their crew.
                //
                // We don't have target tracking yet, so we approximate
                // by spawning a handful of pods radially around the
                // firer. Friendly contact picks them up; enemy contact
                // does nothing. Range/Damage tuning from .ini Special
                // (Range=11→440, Damage=5, Velocity=4→38.4).
                kind: AbilityKind::Sequence(vec![
                    AbilityKind::SpawnSubEntity {
                        local_offset: Vec2::new(-30.0, 0.0),
                        initial_angle_offset: -std::f32::consts::FRAC_PI_2,
                        initial_speed: 4.0 * SC2_VEL_SCALE,
                        sprite_path: Some("ships/syrpe/sprites/shot_b01.png".into()),
                        sprite_size: 8.0,
                        color: Color::srgb(1.0, 0.6, 0.9),
                        hp: 1,
                        lifetime_s: 12.0,
                        ai: crate::ability::SubEntityAiSpec::DriftAndCollect {
                            crew_value: 1,
                        },
                    },
                    AbilityKind::SpawnSubEntity {
                        local_offset: Vec2::new(30.0, 0.0),
                        initial_angle_offset: std::f32::consts::FRAC_PI_2,
                        initial_speed: 4.0 * SC2_VEL_SCALE,
                        sprite_path: Some("ships/syrpe/sprites/shot_b01.png".into()),
                        sprite_size: 8.0,
                        color: Color::srgb(1.0, 0.6, 0.9),
                        hp: 1,
                        lifetime_s: 12.0,
                        ai: crate::ability::SubEntityAiSpec::DriftAndCollect {
                            crew_value: 1,
                        },
                    },
                    AbilityKind::SpawnSubEntity {
                        local_offset: Vec2::new(0.0, 30.0),
                        initial_angle_offset: 0.0,
                        initial_speed: 4.0 * SC2_VEL_SCALE,
                        sprite_path: Some("ships/syrpe/sprites/shot_b01.png".into()),
                        sprite_size: 8.0,
                        color: Color::srgb(1.0, 0.6, 0.9),
                        hp: 1,
                        lifetime_s: 12.0,
                        ai: crate::ability::SubEntityAiSpec::DriftAndCollect {
                            crew_value: 1,
                        },
                    },
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
                // The real abilities live in `ShipModes` — see
                // `modes_for(Andgu)`. This is just a fallback for the
                // (impossible) case where ShipModes is missing.
                kind: AbilityKind::ToggleMode,
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
                    sprite_size: 14.0,
                    color: Color::srgb(0.7, 0.8, 1.0),
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
                // shporzne.cpp activate_special: spawn one OrzMarine
                // sub-entity per press (cost 1 crew, up to MAX_MARINES).
                // The marine homes on the nearest enemy, attaches on
                // contact, and drains crew over time. We model that
                // as AttachAndDrain — one-shot heavy crew drain
                // approximating the per-tick drain over the canonical
                // attached duration.
                kind: AbilityKind::Sequence(vec![
                    AbilityKind::ModifyCrew { delta: -1 },
                    AbilityKind::SpawnSubEntity {
                        local_offset: Vec2::new(0.0, 28.0),
                        initial_angle_offset: 0.0,
                        // .ini Special SpeedMax=40 → 384 u/s top.
                        initial_speed: 40.0 * SC2_VEL_SCALE,
                        sprite_path: Some("ships/orzne/sprites/shot_b_01_bmp.png".into()),
                        sprite_size: 10.0,
                        color: Color::srgb(0.9, 1.0, 0.5),
                        // .ini Armour=3 (marine itself can absorb a
                        // few projectile hits — though we don't
                        // currently route projectile damage to subs
                        // — for now hp=1 = one-shot on a ship hit).
                        hp: 1,
                        lifetime_s: 15.0,
                        ai: crate::ability::SubEntityAiSpec::AttachAndDrain {
                            turn_rate: sc2_turning(2.0),
                            speed: 40.0 * SC2_VEL_SCALE,
                            crew_drain: 4,
                        },
                    },
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
        Option<&ShipModes>,
    )>,
) {
    for (ship, class, derived, mut thrust, mut torque, mut ang_vel, modes) in &mut q {
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
fn cap_velocity(mut q: Query<(&ShipPhysicsDerived, &mut LinearVelocity)>) {
    for (derived, mut vel) in &mut q {
        let speed = vel.0.length();
        if speed > derived.speed_max && derived.speed_max > 0.0 {
            vel.0 = vel.0 / speed * derived.speed_max;
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
        Sprite::from_color(color, Vec2::new(width * 2.0, range)),
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
    // Compute the initial world pose so the first render frame after
    // spawn shows the beam at the owner's muzzle, not at world origin.
    // tick_beams updates it every FixedUpdate after that. Without this,
    // there's a brief "ghost beam" at (0, 0) flashing every shot.
    let world_origin = owner_pos
        + Vec2::new(
            local_origin.x * owner_rot.cos - local_origin.y * owner_rot.sin,
            local_origin.x * owner_rot.sin + local_origin.y * owner_rot.cos,
        );
    let world_dir = Vec2::new(
        local_dir.x * owner_rot.cos - local_dir.y * owner_rot.sin,
        local_dir.x * owner_rot.sin + local_dir.y * owner_rot.cos,
    );
    let midpoint = world_origin + world_dir * (range * 0.5);
    let angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

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
        Sprite::from_color(color, Vec2::new(width * 2.0, range)),
        Transform::from_translation(midpoint.extend(0.3))
            .with_rotation(Quat::from_rotation_z(angle)),
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
            commands.entity(entity).remove::<ModeToggleRequest>();
            continue;
        }
        modes.current = (modes.current + 1) % modes.modes.len();
        info!("mode toggled → {}", modes.modes[modes.current].name);
        commands.entity(entity).remove::<ModeToggleRequest>();
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
            commands.entity(entity).insert(new_abilities);
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
    for (overlay_entity, overlay, mut sprite, mut transform) in &mut overlays {
        let Ok((parent_pos, parent_rot)) = parents.get(overlay.parent) else {
            commands.entity(overlay_entity).despawn();
            continue;
        };
        let n = overlay.frames.len();
        if n == 0 {
            continue;
        }
        let parent_angle = parent_rot.sin.atan2(parent_rot.cos);
        let total = parent_angle + overlay.extra_angle;
        let n_f = n as f32;
        let mut idx = ((-total) / std::f32::consts::TAU * n_f).rem_euclid(n_f) as usize;
        if idx >= n {
            idx = 0;
        }
        sprite.image = overlay.frames[idx].clone();
        transform.translation = parent_pos.0.extend(overlay.z_offset);
        // Same trick as `swap_rotation_frame` — the chosen frame
        // already encodes the rotation, so cancel any Transform-side
        // rotation. Avian doesn't touch this entity (no RigidBody),
        // so this stays at IDENTITY across frames.
        transform.rotation = Quat::IDENTITY;
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
    damage_to_batt: Query<&DamageToBattery>,
    mut crews: Query<&mut Crew>,
    mut batteries: Query<&mut Battery>,
    mut velocities: Query<&mut LinearVelocity>,
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

        // Limpets are NOT damage projectiles in canon — they slow
        // the target instead. shpvuxin.cpp:VuxLimpet::inflict_damage
        // calls `handle_speed_loss(slowdown_factor)` and bypasses
        // the normal damage path entirely. We do the same: multiply
        // the target's current LinearVelocity by `slowdown_factor`,
        // skip crew/battery damage.
        if let Ok(limpet) = limpets.get(proj_entity) {
            if let Ok(mut vel) = velocities.get_mut(other_entity) {
                vel.0 *= limpet.slowdown_factor;
                info!(
                    "limpet hit: target velocity ×{:.2} (now |v|={:.0})",
                    limpet.slowdown_factor,
                    vel.0.length()
                );
            }
            commands.entity(proj_entity).despawn();
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
        commands.entity(proj_entity).despawn();
    }
}

/// Ship-on-ship contact damage. Avian has already applied the impulse
/// (so the bigger ship pushes the smaller one and they both potentially
/// spin); on top of that we deduct crew proportional to how fast they
/// were closing. Below a threshold relative speed it's just a love tap,
/// no damage.
// Ship-ship ramming damage was an invention — canon SC2 / TW melee
// has ZERO baseline ram damage. Ships just bounce off each other
// (Avian's solver handles the elastic-ish collision response). The
// few classes that *do* damage on contact (Androsynth Blazer mode,
// Arilou post-teleport telefrag, Pkunk reborn-phaser) get it via
// per-special components/abilities, not via a generic system.
//
// If we need ram damage again as an opt-in (e.g. for a custom mode
// or a future ship that's all about ramming), add it as an
// `AbilityKind::GrantRamDamage` or a class-marker component rather
// than as a global rule.

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
    // PD damage skips invisible enemies — they're not auto-targetable.
    mut ships: Query<
        (Entity, &Ship, &Position, &mut Crew),
        (Without<PointDefenseActive>, Without<Invisible>),
    >,
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
    // Beams skip invisible ships during target search (cloaked Ilwrath
    // can't be hit by an auto-aim laser).
    mut ships: Query<(Entity, &Ship, &Position, &mut Crew), Without<Invisible>>,
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

/// Pull (or push) the nearest enemy in range toward (or away from)
/// each tractor's owner. Despawns when the owner dies or the tractor
/// runs out of remaining time. Updates the sprite each tick to span
/// owner-origin → target so the player sees the connection.
fn tick_tractors(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut tractors: Query<(Entity, &mut TractorBeam, &mut Transform, &mut Sprite)>,
    owners: Query<(&Ship, &Position, &Rotation)>,
    mut ships: Query<(Entity, &Ship, &Position, &mut LinearVelocity, &Mass), Without<Invisible>>,
) {
    let dt = time.delta_secs();
    for (tractor_entity, mut tractor, mut tractor_xf, mut sprite) in &mut tractors {
        let Ok((owner_ship, owner_pos, owner_rot)) = owners.get(tractor.owner) else {
            commands.entity(tractor_entity).despawn();
            continue;
        };
        let world_origin = owner_pos.0
            + Vec2::new(
                tractor.local_origin.x * owner_rot.cos - tractor.local_origin.y * owner_rot.sin,
                tractor.local_origin.x * owner_rot.sin + tractor.local_origin.y * owner_rot.cos,
            );
        let r2 = tractor.range * tractor.range;

        // Find nearest non-friendly, non-invisible ship in range.
        let mut best: Option<(Entity, Vec2, f32)> = None;
        for (e, s, p, _, _) in &ships {
            if s.player_slot == owner_ship.player_slot {
                continue;
            }
            let d2 = (p.0 - world_origin).length_squared();
            if d2 > r2 {
                continue;
            }
            if best.map_or(true, |(_, _, bd)| d2 < bd) {
                best = Some((e, p.0, d2));
            }
        }

        let hit_endpoint = if let Some((target_e, target_pos, _)) = best {
            // Apply the force as a velocity nudge toward the owner.
            // Δv = force_per_tick / target_mass — heavy ships drift
            // less per tick (correct Newtonian behaviour).
            if let Ok((_, _, _, mut vel, mass)) = ships.get_mut(target_e) {
                let to_owner = world_origin - target_pos;
                let len = to_owner.length();
                if len > 1e-3 {
                    let dir = to_owner / len;
                    let m = mass.0.max(0.0001);
                    vel.0 += dir * (tractor.force_per_tick / m);
                }
            }
            target_pos
        } else {
            // No target → sprite shows the full range as a faint hint.
            world_origin + Vec2::Y * tractor.range
        };

        let mid = (world_origin + hit_endpoint) * 0.5;
        let delta = hit_endpoint - world_origin;
        let len = delta.length().max(1.0);
        let angle = delta.y.atan2(delta.x) - std::f32::consts::FRAC_PI_2;
        tractor_xf.translation = mid.extend(0.3);
        tractor_xf.rotation = Quat::from_rotation_z(angle);
        sprite.custom_size = Some(Vec2::new(tractor.width * 2.0, len));
        sprite.color = tractor.color;

        tractor.remaining -= dt;
        if tractor.remaining <= 0.0 {
            commands.entity(tractor_entity).despawn();
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
            commands.entity(e).remove::<Invisible>();
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
            commands.entity(e).remove::<DamageToBattery>();
        }
    }
}

/// Drive each `SubEntity` per tick: lifetime/hp bookkeeping, target
/// (re)acquisition, steering toward the target capped by `turn_rate`.
/// Pure-drift behaviours (Syreen pods) skip the steering.
fn tick_sub_entities(
    mut commands: Commands,
    time: Res<Time<Physics>>,
    mut subs: Query<(Entity, &mut SubEntity, &Position, &mut LinearVelocity, &mut SubEntityAi)>,
    ships: Query<(Entity, &Ship, &Position), Without<SubEntity>>,
) {
    let dt = time.delta_secs();
    for (sub_entity, mut sub, sub_pos, mut sub_vel, mut ai) in &mut subs {
        sub.remaining_s -= dt;
        if sub.remaining_s <= 0.0 || sub.hp <= 0 {
            commands.entity(sub_entity).despawn();
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
            SubEntityAi::AttachAndDrain { crew_drain, .. } => {
                if let Ok(mut crew) = crews.get_mut(other_entity) {
                    crew.current = (crew.current - *crew_drain).max(0);
                }
                info!(
                    "marine boarded P{}: -{} crew",
                    other_ship.player_slot + 1,
                    crew_drain
                );
                commands.entity(sub_entity).despawn();
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
                    commands.entity(sub_entity).despawn();
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
