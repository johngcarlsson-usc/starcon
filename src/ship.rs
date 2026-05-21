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
/// `ShipStats` (loaded from `.ini`); per-class *behaviour* lives in match
/// expressions in this file. Adding a new class is one variant here plus
/// arms in `fire_weapons`/etc — no plugin registration ceremony.
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
    /// Max rad/sec the projectile can re-aim. Comes from a WeaponSpec
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
                fire_weapons.after(tick_weapon_cooldown),
                trigger_specials.after(tick_special_cooldown),
                tick_projectile_lifetime,
                steer_homing_projectiles,
                orient_projectiles,
                tick_damage_zones,
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
    // Opt this ship into the data-driven ability dispatcher when its
    // class has a manifest. Otherwise, fall through to the per-class
    // match arms in `fire_weapons` / `trigger_specials`. As classes
    // get converted, this map grows and the match arms shrink.
    if let Some(abilities) = abilities_for(class) {
        entity.insert(abilities);
    }
}

/// Per-class data manifest builder. Returning `None` keeps the ship on
/// the legacy match-arm path. The numbers come straight from the
/// canonical `shp*.cpp` / `.ini`, scaled with the same helpers as
/// `primary_weapon` (SC2_VEL_SCALE, SC2_RANGE_SCALE, sc2_turning).
///
/// The intent is for every variant of `ShipClass` to return `Some(...)`
/// here eventually; at that point `primary_weapon` / `trigger_specials`
/// and the `ShipClass` match arms can be deleted.
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

        // --- The rest of the roster: mechanically faithful conversions
        // from `primary_weapon` / `trigger_specials`. Specials whose
        // canonical behaviour needs a primitive that doesn't exist
        // yet stay as `Todo { ident: "..." }` so the dispatcher logs
        // them but doesn't pretend to do something it can't.

        // Chmmr Avatar — continuous laser (TODO Beam) + tractor beam
        // (TODO AppliedForce). Primary fires a fast straight bolt as
        // a placeholder for the canonical Laser.
        ShipClass::Chmav => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 30.0),
                    random_spread_rad: 0.0,
                    speed: 1500.0,
                    lifetime: (10.0 * SC2_RANGE_SCALE) / 1500.0,
                    color: Color::srgb(1.0, 0.3, 0.3),
                    sprite_size: 4.0,
                    sprite_path: Some("ships/chmav/sprites/shot_a1_00_bmp.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
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

        // Arilou Skiff — auto-aim laser (TODO Beam) + random teleport.
        ShipClass::Arisk => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 22.0),
                    random_spread_rad: 0.0,
                    speed: 2400.0,
                    lifetime: (5.5 * SC2_RANGE_SCALE) / 2400.0,
                    color: Color::srgb(0.6, 1.0, 0.8),
                    sprite_size: 5.0,
                    sprite_path: Some("ships/arisk/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
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

        // VUX Intruder — laser placeholder + backward limpet special.
        // Real laser is TODO (Beam primitive); limpet uses existing
        // pipeline via VolleySpec.is_limpet=true.
        ShipClass::Vuxin => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::SpawnProjectiles { volleys: vec![VolleySpec {
                    barrels: single_barrel(forward, 22.0),
                    random_spread_rad: 0.0,
                    speed: 2400.0,
                    lifetime: (9.0 * SC2_RANGE_SCALE) / 2400.0,
                    color: Color::srgb(0.6, 1.0, 0.4),
                    sprite_size: 6.0,
                    sprite_path: Some("ships/vuxin/sprites/shot_a01.png".into()),
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                    recoil_impulse: 0.0,
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
                // ahead. Attached-zone primitive doesn't exist —
                // approximate with a brief stationary zone.
                kind: AbilityKind::SpawnDamageZone {
                    offset: Vec2::new(0.0, 39.0),
                    radius: 20.0,
                    damage_per_sec: 240.0, // 12 dmg/0.05s
                    duration_s: 0.3,
                    source_self: true,
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

        // Umgah Drone — attached cone (TODO) + anti-grav slingshot.
        ShipClass::Umgdr => Some(ShipAbilities {
            primary: AbilitySpec {
                kind: AbilityKind::Todo { ident: "Umgah attached cone (shpumgdr.cpp:UmgahCone)" },
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

/// Spawn a primary-weapon projectile when FIRE is pressed and the
/// weapon is off cooldown. Dispatches on `ShipClass` so each class can
/// shape its weapon differently — direction, speed, sprite tint, etc.
///
/// The projectile is a small fast dynamic body, so when it hits a ship
/// the physics solver computes the impulse-at-contact correctly —
/// the spin imparted to the target is free.
fn fire_weapons(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    // `Without<ShipAbilities>` so ships that have been migrated to the
    // data-driven dispatcher in `src/ability.rs` are skipped here —
    // otherwise they'd fire twice (once per layer).
    mut q: Query<
        (
            Entity,
            &Ship,
            &ShipClass,
            &Position,
            &Rotation,
            &mut LinearVelocity,
            &mut WeaponCooldown,
            &mut Battery,
        ),
        Without<crate::ability::ShipAbilities>,
    >,
) {
    for (entity, ship, class, pos, rot, mut vel, mut cooldown, mut battery) in &mut q {
        if cooldown.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_FIRE) {
            continue;
        }

        // Gate on battery — `WeaponDrain` is the per-shot energy cost
        // (.ini field). Insufficient battery silently skips the shot;
        // the player has to wait for the recharge ticker to top them
        // back up. Matches the canonical SC2 behaviour where you can
        // hear a "click" but no shot leaves the ship.
        if ship.stats.weapon_drain > 0 && battery.current < ship.stats.weapon_drain {
            continue;
        }
        battery.current = (battery.current - ship.stats.weapon_drain).max(0);

        let spec = primary_weapon(*class);

        // SC2 frame rate is 20 Hz. [Weapon] Rate is the number of
        // frames *between* shots — so 10 → 10/20 = 0.5 s per shot,
        // 2 shots/sec. WeaponRate=0 means "every frame" → machine-
        // gun fire (Spathi cannon, Pkunk Fury, etc); clamp the
        // minimum at one frame so the fixed-update can keep up.
        const SC2_FRAME_RATE: f32 = 20.0;
        let cooldown_secs = (ship.stats.weapon_rate as f32 / SC2_FRAME_RATE).max(1.0 / SC2_FRAME_RATE);
        cooldown.0 = cooldown_secs;

        let damage = ship.stats.weapon_damage.max(1);

        // Default single-barrel fallback when no explicit barrels are
        // declared. Most ships fall through this path.
        let single = [Barrel {
            local_pos: spec.local_direction * spec.muzzle_offset,
            direction: spec.local_direction,
        }];
        let barrels: &[Barrel] = if spec.barrels.is_empty() {
            &single
        } else {
            spec.barrels
        };

        for barrel in barrels {
            // Rotate the barrel's local position + direction into world
            // space by the ship's current rotation.
            let world_pos_offset = Vec2::new(
                barrel.local_pos.x * rot.cos - barrel.local_pos.y * rot.sin,
                barrel.local_pos.x * rot.sin + barrel.local_pos.y * rot.cos,
            );
            let mut world_dir = Vec2::new(
                barrel.direction.x * rot.cos - barrel.direction.y * rot.sin,
                barrel.direction.x * rot.sin + barrel.direction.y * rot.cos,
            );
            // Random per-shot spread (Zoq-Fot-Pik wobble): a small
            // angle jitter centred on the firing direction.
            if spec.random_spread_rad > 0.0 {
                let jitter = (fastrand::f32() * 2.0 - 1.0) * spec.random_spread_rad;
                let (s, c) = jitter.sin_cos();
                world_dir = Vec2::new(
                    world_dir.x * c - world_dir.y * s,
                    world_dir.x * s + world_dir.y * c,
                );
            }

            spawn_projectile_world(
                &mut commands,
                &assets,
                entity,
                pos.0 + world_pos_offset,
                world_dir,
                vel.0,
                &spec,
                damage,
            );

            // Recoil applies once per barrel — heavy guns with many
            // barrels (no canonical examples yet) recoil correspondingly
            // more, which is correct under Newton's third law.
            if spec.recoil_impulse > 0.0 {
                vel.0 -= world_dir * spec.recoil_impulse / ship.stats.mass;
            }
        }
    }
}

/// Spawn a projectile from a WeaponSpec, given the firing ship's pose
/// and velocity. Returns the world-space firing direction so callers
/// can apply recoil to the firer if the spec requests it.
/// High-level: spawn from a ship pose + spec. Single forward shot,
/// pre-`barrels` callers still use this for convenience (e.g. the
/// Spathi BUTT special). For multi-barrel weapons fire_weapons calls
/// `spawn_projectile_world` directly per barrel.
fn spawn_projectile(
    commands: &mut Commands,
    assets: &AssetServer,
    owner: Entity,
    pos: Vec2,
    rot: &Rotation,
    ship_vel: Vec2,
    spec: &WeaponSpec,
    damage: i32,
) -> Vec2 {
    let local_dir = spec.local_direction;
    let world_dir = Vec2::new(
        local_dir.x * rot.cos - local_dir.y * rot.sin,
        local_dir.x * rot.sin + local_dir.y * rot.cos,
    );
    let muzzle = pos + world_dir * spec.muzzle_offset;
    spawn_projectile_world(commands, assets, owner, muzzle, world_dir, ship_vel, spec, damage);
    world_dir
}

/// Low-level: spawn at an explicit world position + direction. Used by
/// `fire_weapons` once it has rotated each `Barrel` into world space.
fn spawn_projectile_world(
    commands: &mut Commands,
    assets: &AssetServer,
    owner: Entity,
    muzzle: Vec2,
    world_dir: Vec2,
    ship_vel: Vec2,
    spec: &WeaponSpec,
    damage: i32,
) {
    let projectile_vel = ship_vel + world_dir * spec.speed;
    // Initial rotation matches velocity direction so the canonical
    // up-facing sprite frame visually points the right way at spawn;
    // `orient_projectiles` keeps it aligned as homing projectiles curve.
    let initial_angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

    let sprite = if let Some(path) = spec.projectile_sprite {
        Sprite {
            image: assets.load(path),
            color: spec.color,
            custom_size: Some(Vec2::splat(spec.sprite_size)),
            ..default()
        }
    } else {
        // Fallback: tinted square. Works on native but renders nothing
        // in browsers via the empty-handle path in Bevy 0.18 — ships
        // without canonical sprite paths show invisible projectiles
        // on web. Wire `projectile_sprite: Some(...)` for the real art.
        Sprite::from_color(spec.color, Vec2::splat(spec.sprite_size))
    };

    let mut ent = commands.spawn((
        Projectile {
            owner,
            damage,
            lifetime: spec.lifetime,
        },
        sprite,
        Transform::from_translation(muzzle.extend(0.5)),
        RigidBody::Dynamic,
        Collider::circle(spec.sprite_size * 0.5),
        Mass(0.5),
        Rotation::radians(initial_angle),
        LinearVelocity(projectile_vel),
        AngularVelocity::ZERO,
        // No damping — projectile flies straight until it dies or hits.
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
    if spec.homing_turn_rate > 0.0 {
        ent.insert(Homing {
            target: None,
            turn_rate: spec.homing_turn_rate,
        });
    }
    if spec.is_limpet {
        ent.insert(Limpet);
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

/// One projectile-spawn descriptor inside a `WeaponSpec`. A WeaponSpec
/// with multiple barrels fires one projectile per barrel per shot,
/// independently positioned and aimed in ship-local space.
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

/// Per-class primary-weapon shape. The fields stay generic on purpose so
/// adding a class doesn't reshape callers — just add a match arm.
struct WeaponSpec {
    local_direction: Vec2,
    muzzle_offset: f32,
    /// Multi-barrel weapons (Yehat twin missiles, Pkunk triple, etc.)
    /// list each barrel here. When non-empty this overrides the
    /// `local_direction` + `muzzle_offset` single-shot defaults — one
    /// projectile spawns per `Barrel`. Empty means "single forward
    /// shot at `local_direction × muzzle_offset`", the common case.
    barrels: &'static [Barrel],
    /// Random angle jitter per shot, in radians. Used by Zoq-Fot-Pik
    /// (`angle + ANGLE_RATIO * random(-10.0, 10.0)` in the legacy).
    /// Zero for everything else.
    random_spread_rad: f32,
    speed: f32,
    lifetime: f32,
    color: Color,
    sprite_size: f32,
    /// Optional asset path (relative to `assets/`) of the projectile's
    /// canonical sprite from the legacy `.dat`. When `Some`, the
    /// projectile renders with that texture and is rotated to face
    /// its velocity vector each tick (see `orient_projectiles`).
    /// When `None`, falls back to the WhitePixel + `color` tint —
    /// useful for placeholder projectiles or AoE blobs.
    ///
    /// Naming convention from `[Objects]` in each ship's `SHIP_DAT`:
    /// `shot_a##` is `WeaponSprites` (primary), `shot_b##` is whatever
    /// follows it (`WeaponExplosion` for ships with no special sprite,
    /// or `SpecialSprites` for ships like Spathi with a separate
    /// special projectile). Picking frame `_01` gives the up-facing
    /// version, which the orient system then rotates per-frame.
    projectile_sprite: Option<&'static str>,
    /// Recoil impulse imparted to the firing ship, in N·s. Real units
    /// — applied as `Δv = -world_dir * recoil_impulse / ship_mass`, so
    /// the same cannon kicks a light hull harder than a heavy one
    /// (Newton's third law). Zero for low-recoil weapons (point defense,
    /// lasers, projectile launchers with internal compensators).
    recoil_impulse: f32,
    /// If > 0, the spawned projectile gets a `Homing` component with
    /// this rad/sec turn cap, so it steers toward the nearest enemy.
    /// Derived from the legacy `.ini` `TurnRate` of the relevant
    /// [Weapon] or [Special] section, scaled the same way the ship's
    /// hull turn rate is (`scale_turning` in mhelpers.cpp).
    homing_turn_rate: f32,
    /// VUX-style limpet: on hit, instead of "deduct crew and despawn",
    /// the projectile transfers its mass onto the target via a `Limpet`
    /// marker — see handle_projectile_hits. Cumulative — every limpet
    /// makes the target heavier, which through the existing physics
    /// (F = m·a, terminal v = thrust / (m·damping)) slows acceleration
    /// AND top speed without any special-case "slow effect" code.
    is_limpet: bool,
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

// Multi-barrel layouts kept as `const` arrays so the `&'static [Barrel]`
// references in `WeaponSpec` are actually static. Coordinates come
// straight from the legacy `shp*.cpp activate_weapon` / `calculate_fire_weapon`
// add(new Missile(this, Vector2(x, y), angle ± offset, ...)) calls.

/// Yehat Terminator — twin forward missiles from (±24, 14).
const YEHAT_BARRELS: [Barrel; 2] = [
    Barrel { local_pos: Vec2::new(-24.0, 14.0), direction: Vec2::new(0.0, 1.0) },
    Barrel { local_pos: Vec2::new( 24.0, 14.0), direction: Vec2::new(0.0, 1.0) },
];

/// Pkunk Fury — forward shot + lateral ±90° shots from the wingtips.
const PKUNK_BARRELS: [Barrel; 3] = [
    Barrel { local_pos: Vec2::new(  0.0, 16.0), direction: Vec2::new( 0.0,  1.0) },
    Barrel { local_pos: Vec2::new(-16.0,  0.0), direction: Vec2::new(-1.0,  0.0) },
    Barrel { local_pos: Vec2::new( 16.0,  0.0), direction: Vec2::new( 1.0,  0.0) },
];

/// Utwig Jugger — six forward missiles in a sweeping arc.
const UTWIG_BARRELS: [Barrel; 6] = [
    Barrel { local_pos: Vec2::new(-34.0, 11.0), direction: Vec2::new(0.0, 1.0) },
    Barrel { local_pos: Vec2::new( 34.0, 11.0), direction: Vec2::new(0.0, 1.0) },
    Barrel { local_pos: Vec2::new(-18.0, 20.0), direction: Vec2::new(0.0, 1.0) },
    Barrel { local_pos: Vec2::new( 18.0, 20.0), direction: Vec2::new(0.0, 1.0) },
    Barrel { local_pos: Vec2::new( -6.0, 27.0), direction: Vec2::new(0.0, 1.0) },
    Barrel { local_pos: Vec2::new(  6.0, 27.0), direction: Vec2::new(0.0, 1.0) },
];

/// Mmrnmhrm Y-Form — twin homing missiles toed out ±25° from forward.
///
/// 25° = 25·(2π/64) rad in canon (ANGLE_RATIO = 2π/64). The barrel
/// direction = rotate (0,1) by ±25°. We hard-code the resulting components
/// because `f32::sin`/`cos` are not const.
const MMRX_Y_BARRELS: [Barrel; 2] = [
    Barrel {
        local_pos: Vec2::new(-13.0, 2.0),
        direction: Vec2::new(-0.42261826, 0.9063078),
    },
    Barrel {
        local_pos: Vec2::new(13.0, 2.0),
        direction: Vec2::new(0.42261826, 0.9063078),
    },
];

fn primary_weapon(class: ShipClass) -> WeaponSpec {
    let forward = Vec2::new(0.0, 1.0);
    match class {
        ShipClass::Earcr => WeaponSpec {
            // shpearcr.cpp activate_weapon: spawns EarthlingMissile from
            // Vector2(0, size.y/2) forward. EarthlingMissile is a
            // HomingMissile with TurnRate from [Weapon] TurnRate=3.
            // .ini: Velocity=80, Range=60, Damage=4, TurnRate=3.
            local_direction: forward,
            muzzle_offset: 28.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 80.0 * SC2_VEL_SCALE,
            lifetime: (60.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 16.0,
            // Canonical EarthlingMissile sprite from shot_a (= WeaponSprites).
            projectile_sprite: Some("ships/earcr/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: sc2_turning(3.0),
            is_limpet: false,
        },
        ShipClass::Spael => WeaponSpec {
            // shpspael.cpp activate_weapon: forward Missile from
            // Vector2(0, size.y/2). .ini: Velocity=96, Range=17, Damage=1.
            // WeaponRate=0 → fires every frame (machine gun).
            local_direction: forward,
            muzzle_offset: 22.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 96.0 * SC2_VEL_SCALE,
            lifetime: (17.0 * SC2_RANGE_SCALE) / (96.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 8.0,
            projectile_sprite: Some("ships/spael/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Yehte => WeaponSpec {
            // shpyehte.cpp activate_weapon: TWO forward Missiles from
            // Vector2(±24, 14). .ini Weapon: Velocity=80, Range=12,
            // Damage=1, Armour=1.
            local_direction: forward,
            muzzle_offset: 0.0,
            barrels: &YEHAT_BARRELS,
            random_spread_rad: 0.0,
            speed: 80.0 * SC2_VEL_SCALE,
            lifetime: (12.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 8.0,
            projectile_sprite: Some("ships/yehte/sprites/shot_a01_bmp.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Chmav => WeaponSpec {
            // shpchmav.cpp activate_weapon: spawns a ChmmrLaser (Laser
            // type — continuous beam) from Vector2(0, 25). Beam
            // primitive doesn't exist yet → fire a fast short-lived
            // projectile placeholder. .ini Weapon: Range=10, Damage=2.
            // TODO: implement Laser primitive (shpchmav.cpp:64).
            local_direction: forward,
            muzzle_offset: 30.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 1500.0,
            lifetime: (10.0 * SC2_RANGE_SCALE) / 1500.0,
            color: Color::srgb(1.0, 0.3, 0.3),
            sprite_size: 4.0,
            projectile_sprite: Some("ships/chmav/sprites/shot_a1_00_bmp.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Kzedr => WeaponSpec {
            // shpkzedr.cpp activate_weapon: forward KzerZaMissile from
            // Vector2(0, size.y/2). .ini Weapon: Velocity=80, Range=22,
            // Damage=6, Armour=6.
            local_direction: forward,
            muzzle_offset: 36.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 80.0 * SC2_VEL_SCALE,
            lifetime: (22.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 12.0,
            projectile_sprite: Some("ships/kzedr/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Mycpo => WeaponSpec {
            // shpmycpo.cpp activate_weapon: spawns MyconPlasma (extends
            // HomingMissile) from Vector2(0, size.y). .ini Weapon:
            // Velocity=35, Range=60, Damage=10, Homing=1 →
            // scale_turning(1) ≈ 3.927 rad/s.
            local_direction: forward,
            muzzle_offset: 26.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 35.0 * SC2_VEL_SCALE,
            lifetime: (60.0 * SC2_RANGE_SCALE) / (35.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 16.0,
            projectile_sprite: Some("ships/mycpo/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: sc2_turning(1.0),
            is_limpet: false,
        },
        ShipClass::Shosc => WeaponSpec {
            // shpshosc.cpp activate_weapon: forward Missile from
            // Vector2(0, size.y/2). .ini Weapon: Velocity=96, Range=14,
            // Damage=1.
            local_direction: forward,
            muzzle_offset: 22.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 96.0 * SC2_VEL_SCALE,
            lifetime: (14.0 * SC2_RANGE_SCALE) / (96.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/shosc/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Arisk => WeaponSpec {
            // shparisk.cpp activate_weapon: spawns a Laser auto-aimed at
            // the nearest non-invisible ship within weaponRange+200.
            // .ini Weapon: Range=5.5, Frames=100, Damage=1. Laser
            // primitive doesn't exist yet — use a very-fast forward
            // projectile placeholder so the "instant hit" feel is
            // preserved. The auto-aim half lands when we add the Laser
            // primitive (TODO: shparisk.cpp:78).
            local_direction: forward,
            muzzle_offset: 22.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 2400.0,
            lifetime: (5.5 * SC2_RANGE_SCALE) / 2400.0,
            color: Color::srgb(0.6, 1.0, 0.8),
            sprite_size: 5.0,
            projectile_sprite: Some("ships/arisk/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Pkufu => WeaponSpec {
            // shppkufu.cpp activate_weapon: THREE AnimatedShots —
            //   forward from Vector2(0, size.y/2),
            //   left    from Vector2(-size.x/2, 0) at angle - π/2,
            //   right   from Vector2( size.x/2, 0) at angle + π/2.
            // .ini Weapon: Velocity=96, Range=5.5, Damage=1.
            local_direction: forward,
            muzzle_offset: 0.0,
            barrels: &PKUNK_BARRELS,
            random_spread_rad: 0.0,
            speed: 96.0 * SC2_VEL_SCALE,
            lifetime: (5.5 * SC2_RANGE_SCALE) / (96.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/pkufu/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Ilwav => WeaponSpec {
            // shpilwav.cpp activate_weapon: AnimatedShot forward from
            // Vector2(0, size.y/2). If cloaked + target in range, the
            // shot direction is intercept-aimed at the target; cloak
            // drops on fire. We don't model the cloak-aim path yet.
            // .ini Weapon: Velocity=28, Range=2.8, Damage=1.
            local_direction: forward,
            muzzle_offset: 30.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 28.0 * SC2_VEL_SCALE,
            lifetime: (2.8 * SC2_RANGE_SCALE) / (28.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 0.6, 0.3),
            sprite_size: 8.0,
            projectile_sprite: Some("ships/ilwav/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Thrto => WeaponSpec {
            // shpthrto.cpp activate_weapon: forward Missile from
            // Vector2(0, 0.5*size.y). .ini Weapon: Velocity=120, Range=25,
            // Damage=1, Armour=2.
            local_direction: forward,
            muzzle_offset: 22.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (25.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/thrto/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Vuxin => WeaponSpec {
            // VUX primary is canonically a *Laser*, not a missile —
            // shpvuxin.cpp activate_weapon spawns a Laser from
            // Vector2(size.x/11, size.y/2.07). Laser primitive isn't
            // wired yet; using a very fast short-range projectile so it
            // still feels like an instant-hit beam. The limpet is the
            // VUX *special* (see trigger_specials), not the primary —
            // I leave is_limpet=false here. .ini Weapon: Range=9, Damage=1.
            // TODO: replace with Laser primitive (shpvuxin.cpp:217).
            local_direction: forward,
            muzzle_offset: 22.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 2400.0,
            lifetime: (9.0 * SC2_RANGE_SCALE) / 2400.0,
            color: Color::srgb(0.6, 1.0, 0.4),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/vuxin/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Supbl => WeaponSpec {
            // shpsupbl.cpp activate_weapon: forward Missile from
            // Vector2(0, 0.25*size.y). .ini Weapon: Velocity=120,
            // Range=15, Damage=1.
            local_direction: forward,
            muzzle_offset: 24.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (15.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/supbl/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Kohma => WeaponSpec {
            // shpkohma.cpp activate_weapon: forward KohrAhBlade from
            // Vector2(0, size.y/2). Persistent + max-N (MaxBlades=9 in
            // .ini). We fire it as a normal projectile with a long
            // life; the persistence/passive-target-tracking from the
            // legacy KohrAhBlade isn't modelled (TODO: shpkohma.cpp:484).
            // .ini Weapon: Velocity=64, Range=12, Damage=4, Armour=6.
            local_direction: forward,
            muzzle_offset: 34.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 64.0 * SC2_VEL_SCALE,
            lifetime: (12.0 * SC2_RANGE_SCALE) / (64.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 12.0,
            projectile_sprite: Some("ships/kohma/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Syrpe => WeaponSpec {
            // shpsyrpe.cpp activate_weapon: forward Missile from
            // Vector2(0, size.y/2 + 10), with collide_flag_sameship =
            // ALL_LAYERS (i.e. the razor can hit objects spawned by
            // its own ship — currently we don't have those). .ini
            // Weapon: Velocity=120, Range=17, Damage=2.
            local_direction: forward,
            muzzle_offset: 32.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (17.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/syrpe/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Andgu => WeaponSpec {
            // shpandgu.cpp activate_weapon: forward AndrosynthBubble
            // from Vector2(0, size.y/2). Blocked while in Blazer-comet
            // mode (specialActive). .ini Weapon: Velocity=24, Range=50,
            // Damage=2.
            local_direction: forward,
            muzzle_offset: 24.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 24.0 * SC2_VEL_SCALE,
            lifetime: (50.0 * SC2_RANGE_SCALE) / (24.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 14.0,
            projectile_sprite: Some("ships/andgu/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Chebr => WeaponSpec {
            // shpchebr.cpp activate_weapon: spawns a single ChenjesuShot
            // forward from Vector2(0, size.y/2). On release, the
            // crystal explodes into 8 shards radiating at PI/4 steps.
            // We fire the crystal but the on-release-shatter isn't
            // modelled (TODO: shpchebr.cpp:1004). .ini Weapon:
            // Velocity=64, Damage=6, ShardRange=9, ShardDamage=2.
            // No [Weapon] Range — the crystal flies until released.
            local_direction: forward,
            muzzle_offset: 32.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 64.0 * SC2_VEL_SCALE,
            lifetime: 4.0,
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 14.0,
            projectile_sprite: Some("ships/chebr/sprites/shot_a_01_tga.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Druma => WeaponSpec {
            // shpdruma.cpp activate_weapon: forward DruugeMissile from
            // Vector2(0, size.y/2). .ini Weapon: Velocity=120, Range=40,
            // Damage=6, DriftVelocity=375 — this is the Newtonian recoil
            // applied to the firer as `accelerate(this, angle+π,
            // DriftVelocity/mass, MAX_SPEED)`. So the SC2 firer Δv is
            // `DriftVelocity / mass` scaled with the same world units.
            // We mirror this via recoil_impulse such that
            //   Δv = recoil_impulse / mass = (375 · 9.6) / mass
            // i.e. recoil_impulse = 375 · 9.6 ≈ 3600 N·s.
            local_direction: forward,
            muzzle_offset: 30.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (40.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 12.0,
            projectile_sprite: Some("ships/druma/sprites/shot_a01.png"),
            recoil_impulse: 375.0 * SC2_VEL_SCALE,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Utwju => WeaponSpec {
            // shputwju.cpp calculate_fire_weapon: SIX forward Missiles
            // from Vector2(±34, 11), (±18, 20), (±6, 27). .ini Weapon:
            // Velocity=120, Range=14, Damage=1.
            local_direction: forward,
            muzzle_offset: 0.0,
            barrels: &UTWIG_BARRELS,
            random_spread_rad: 0.0,
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (14.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/utwju/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Zfpst => WeaponSpec {
            // shpzfpst.cpp activate_weapon: forward ZoqFotPikShot at
            //   angle + ANGLE_RATIO * random(-10, 10)
            // i.e. random spread of ±10·(2π/64) = ±0.982 rad. .ini
            // Weapon: Velocity=120, Range=11, Damage=1.
            local_direction: forward,
            muzzle_offset: 20.0,
            barrels: &[],
            random_spread_rad: 10.0 * (std::f32::consts::TAU / 64.0),
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (11.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 6.0,
            projectile_sprite: Some("ships/zfpst/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Mmrxf => WeaponSpec {
            // shpmmrxf.cpp activate_weapon: in Y-FORM (our default at
            // spawn), two HomingMissiles from Vector2(±13, 2) at
            // angle ± 25° = ± 25·ANGLE_RATIO rad. .ini Weapon2:
            // Velocity=80, Range=50, Damage=1, TurnRate=9.
            // T-FORM (twin laser) is the *other* form — needs the
            // transform-mode primitive to switch into (TODO:
            // shpmmrxf.cpp:411 case T_FORM).
            local_direction: forward,
            muzzle_offset: 0.0,
            barrels: &MMRX_Y_BARRELS,
            random_spread_rad: 0.0,
            speed: 80.0 * SC2_VEL_SCALE,
            lifetime: (50.0 * SC2_RANGE_SCALE) / (80.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 10.0,
            projectile_sprite: Some("ships/mmrxf/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: sc2_turning(9.0),
            is_limpet: false,
        },
        ShipClass::Orzne => WeaponSpec {
            // shporzne.cpp activate_weapon: forward OrzMissile spawned
            // at the turret angle (turret aim is controlled by L/R
            // while special is held). We don't model the independent
            // turret yet — fire straight forward. .ini Weapon:
            // Velocity=120, Range=20, Damage=3.
            // TODO: independent turret aim (shporzne.cpp:549).
            local_direction: forward,
            muzzle_offset: 28.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 120.0 * SC2_VEL_SCALE,
            lifetime: (20.0 * SC2_RANGE_SCALE) / (120.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 8.0,
            projectile_sprite: Some("ships/orzne/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Slypr => WeaponSpec {
            // shpslypr.cpp: weapon is a SlylandroLaserNew — a
            // multi-segment "lightning" presence that snaps to the
            // nearest target. No projectile, no canonical sprite under
            // shot_a##. Lightning primitive doesn't exist; render a
            // very-fast short-lived projectile placeholder until it's
            // wired (TODO: shpslypr.cpp:SlylandroLaserNew). .ini
            // Weapon: Segments=4, SegmentLength=125, RandomAngle=60.
            local_direction: forward,
            muzzle_offset: 18.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 2400.0,
            lifetime: 0.2,
            color: Color::srgb(1.0, 1.0, 0.5),
            sprite_size: 5.0,
            projectile_sprite: None,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Umgdr => WeaponSpec {
            // shpumgdr.cpp: weapon is a ship-attached UmgahCone — a
            // forward-facing damage region that *moves with the ship*
            // and only deals damage while fire_weapon is held. The
            // canonical cone has no Velocity / Range — it lives at
            // a fixed offset (dist=81) ahead of the ship. We don't
            // have an "attached forward damage zone" primitive yet, so
            // fire a fast short-range placeholder projectile.
            // TODO: implement attached UmgahCone (shpumgdr.cpp:UmgahCone).
            // .ini Weapon: Damage=20, DamageType=2.
            local_direction: forward,
            muzzle_offset: 18.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 600.0,
            lifetime: 0.4,
            color: Color::srgb(0.5, 1.0, 0.7),
            sprite_size: 12.0,
            projectile_sprite: Some("ships/umgdr/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
        ShipClass::Meltr => WeaponSpec {
            // shpmeltr.cpp activate_weapon: spawns a single MelnormeShot
            // from Vector2(0, size.y/2). While held, the shot charges
            // (every charge-cycle: damage×=2, armour×=2, range+=RangeUp)
            // for up to 3 phases. We fire the base shot only; the
            // press-to-charge / release-to-fire input model isn't
            // wired yet (TODO: shpmeltr.cpp:MelnormeShot::calculate).
            // .ini Weapon: Velocity=112, Range=21, RangeUp=3, Damage=2.
            local_direction: forward,
            muzzle_offset: 28.0,
            barrels: &[],
            random_spread_rad: 0.0,
            speed: 112.0 * SC2_VEL_SCALE,
            lifetime: (21.0 * SC2_RANGE_SCALE) / (112.0 * SC2_VEL_SCALE),
            color: Color::srgb(1.0, 1.0, 1.0),
            sprite_size: 10.0,
            projectile_sprite: Some("ships/meltr/sprites/shot_a01.png"),
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
            is_limpet: false,
        },
    }
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

fn tick_special_cooldown(time: Res<Time<Physics>>, mut q: Query<&mut SpecialCooldown>) {
    let dt = time.delta_secs();
    for mut cd in &mut q {
        if cd.0 > 0.0 {
            cd.0 = (cd.0 - dt).max(0.0);
        }
    }
}

/// Per-class SPECIAL ability dispatch. Same shape as fire_weapons:
/// new class → one match arm. Each arm decides whether to mutate the
/// ship's own position/velocity (teleport, dash), apply force to other
/// bodies (push field, tractor), or spawn helper entities (mines,
/// drones). The dispatch is sync — for richer behaviour just spawn
/// an entity with its own per-frame lifetime system.
fn trigger_specials(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    // Same skip-when-ability-driven gate as `fire_weapons`.
    mut q: Query<
        (
            Entity,
            &Ship,
            &ShipClass,
            &mut Position,
            &Rotation,
            &mut LinearVelocity,
            &mut SpecialCooldown,
            &mut Battery,
            &mut Crew,
        ),
        Without<crate::ability::ShipAbilities>,
    >,
) {
    for (entity, ship, class, mut pos, rot, mut vel, mut cooldown, mut battery, mut crew) in
        &mut q
    {
        if cooldown.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_SPECIAL) {
            continue;
        }
        // Battery gate — `SpecialDrain` is per-activation cost.
        if ship.stats.special_drain > 0 && battery.current < ship.stats.special_drain {
            continue;
        }
        battery.current = (battery.current - ship.stats.special_drain).max(0);

        let forward = Vec2::new(-rot.sin, rot.cos);
        let mass = ship.stats.mass.max(0.0001);
        let perp = Vec2::new(-forward.y, forward.x);
        // Helpers: every dash specifies the impulse in N·s. The Δv
        // each ship gets is `impulse / mass`, so heavy hulls feel
        // ponderous and light hulls leap. Brakes specify a maximum
        // drag impulse so a Dreadnought *cannot* hit-and-stop the
        // way a Stinger can.
        let dash = |vel: &mut LinearVelocity, impulse: Vec2| {
            vel.0 += impulse / mass;
        };
        let drag = |vel: &mut LinearVelocity, drag_impulse: f32| {
            let speed = vel.0.length();
            if speed > 0.0 {
                let dv = (drag_impulse / mass).min(speed);
                vel.0 -= vel.0 / speed * dv;
            }
        };

        match class {
            ShipClass::Earcr => {
                // Canonical Earthling Cruiser special: point-defense
                // laser. While active, the ship instantly destroys
                // any non-friendly projectile inside `range` and
                // burns crew off any enemy ship in the same area.
                // .ini stats: Range=5 → 200 world units, Damage=1
                // per frame for Frames=100 (at 20 Hz ≈ 5 s lifetime).
                // We use 1.5 s here to match the cooldown budget;
                // the ability is "press button, defense up briefly".
                commands.entity(entity).insert(PointDefenseActive {
                    remaining: 1.5,
                    range: 200.0,
                    damage_per_tick: 1,
                });
                cooldown.0 = 3.0;
                info!("P{} point defense online", ship.player_slot + 1);
            }
            ShipClass::Spael => {
                // Canonical Spathi special: BUTT (Backward Utilizing
                // Tracking Torpedo). A backward-firing tracking
                // missile, the signature SC2 Eluder gag — Spathi
                // runs while shooting over its shoulder. .ini stats:
                // Special Velocity=45, Range=12, Damage=2, TurnRate=1
                // (homes on enemy). Homing isn't wired up yet so the
                // missile flies straight until M3 adds the primitive.
                let butt = WeaponSpec {
                    local_direction: Vec2::new(0.0, -1.0),
                    muzzle_offset: 22.0,
                    barrels: &[],
                    random_spread_rad: 0.0,
                    // .ini Special Velocity=45 → 45 · 9.6 ≈ 432 u/s.
                    speed: 45.0 * SC2_VEL_SCALE,
                    // .ini Special Range=12 → lifetime = 480/432 ≈ 1.11 s.
                    lifetime: (12.0 * SC2_RANGE_SCALE) / (45.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 12.0,
                    // Canonical BUTT sprite from shot_b (= SpecialSprites
                    // for the Spathi — Spael has SpecialSprites=64 in
                    // its SHIP_DAT [Objects]).
                    projectile_sprite: Some("ships/spael/sprites/shot_b01.png"),
                    recoil_impulse: 0.0,
                    // .ini Special TurnRate=1 → scale_turning(1) ≈ 3.93 rad/s.
                    homing_turn_rate: sc2_turning(1.0),
                    is_limpet: false,
                };
                spawn_projectile(&mut commands, &assets, entity, pos.0, rot, vel.0, &butt, 2);
                cooldown.0 = 1.2;
                info!("P{} BUTT", ship.player_slot + 1);
            }
            ShipClass::Yehte => {
                // shpyehte.cpp activate_special: shieldFrames =
                // (shieldFrames % frame_time) + specialFrames. .ini
                // Special: Frames=500 → 25 s at 20 Hz. While
                // shieldFrames > 0, handle_damage sets normal=0 — i.e.
                // total immunity to projectile damage (collisions
                // still hurt). SpecialDrain=3 already deducted above.
                commands.entity(entity).insert(ShieldActive {
                    remaining: 500.0 / 20.0,
                    damage_factor: 0.0,
                });
                cooldown.0 = 0.15; // SpecialRate=2 → 2/20=0.1; small floor for safety
                info!("P{} shield up", ship.player_slot + 1);
            }
            ShipClass::Chmav => {
                // shpchmav.cpp activate_special: ChmmrBeam tractor — if
                // a target is within specialRange and has mass, the
                // beam *accelerates the target toward the firer* at
                // specialForce / target.mass. Needs cross-entity force
                // application + visible beam. TODO (shpchmav.cpp:87).
                // Placeholder: heavy brake so the Avatar can plant.
                drag(&mut vel, 4000.0);
                cooldown.0 = 2.0;
            }
            ShipClass::Kzedr => {
                // shpkzedr.cpp activate_special: spawn TWO KzerZaFighter
                // sub-entities (1 crew each, costing 1-2 crew of the
                // mother ship). Fighters fly out, fire lasers, return
                // home. Sub-entity AI primitive doesn't exist yet —
                // TODO (shpkzedr.cpp:55). Placeholder: brake.
                drag(&mut vel, 4000.0);
                cooldown.0 = 2.0;
            }
            ShipClass::Mycpo => {
                // shpmycpo.cpp activate_special: damage(this, 0, -4)
                // i.e. heal 4 crew (negative damage). Battery drain
                // (SpecialDrain=40) already deducted above. Skip if
                // already full.
                if crew.current < crew.max {
                    crew.current = (crew.current + 4).min(crew.max);
                    cooldown.0 = 0.1;
                    info!("P{} repair (+4 crew)", ship.player_slot + 1);
                } else {
                    // Refund the drain — legacy returns FALSE early
                    // before the drain hits, so we mirror that here.
                    battery.current = (battery.current + ship.stats.special_drain).min(battery.max);
                }
            }
            ShipClass::Shosc => {
                // Glory Device — the canonical suicide explosion.
                // Spawns an instant-burst DamageZone at the ship's
                // position with source=None (no friendly-fire
                // exemption), so the Shofixti dies in its own blast
                // along with anyone nearby. damage_per_sec × short
                // lifetime ≫ any ship's crew so the radius is a
                // hard kill zone. Lifetime is brief (0.1 s) so it
                // doesn't keep damaging long after the bang.
                spawn_damage_zone(
                    &mut commands,
                    None,
                    pos.0,
                    250.0,
                    100_000.0,
                    0.1,
                    Color::srgba(1.0, 0.6, 0.2, 0.55),
                );
                cooldown.0 = 999.0; // can't fire twice
                info!("P{} GLORY DEVICE", ship.player_slot + 1);
            }
            ShipClass::Arisk => {
                // shparisk.cpp activate_special: translate by
                // random(-1500..1500, -1500..1500) — pure teleport, no
                // velocity change. The legacy code also marks
                // just_teleported = 1 so the next collision auto-
                // kills the Arilou (telefrag risk). We don't model
                // that yet — TODO (shparisk.cpp:100).
                let dx = (fastrand::f32() * 2.0 - 1.0) * 1500.0;
                let dy = (fastrand::f32() * 2.0 - 1.0) * 1500.0;
                pos.0 += Vec2::new(dx, dy);
                cooldown.0 = 0.15;
                info!("P{} hyperspace", ship.player_slot + 1);
            }
            ShipClass::Pkufu => {
                // shppkufu.cpp calculate_fire_special: refills the
                // ship's own battery — `batt += special_drain` (clamped
                // to batt_max), guarded by `batt < batt_max`. This is
                // a battery-taunt: trade special_drain (already
                // deducted above) for special_drain back, i.e. it's a
                // no-cost rapid recharge. We refund the drain so the
                // net effect is "top off battery".
                battery.current = battery.max;
                cooldown.0 = (16.0f32 / 20.0).max(0.1); // SpecialRate=16 → 0.8 s
                info!("P{} taunt (battery full)", ship.player_slot + 1);
            }
            ShipClass::Ilwav => {
                // shpilwav.cpp calculate_fire_special: toggles `cloak`
                // — while cloaked, isInvisible()=true (enemies' AI and
                // homing missiles drop lock). No projectile-damage
                // immunity in canon, just stealth. We don't have an
                // invisibility / target-occlusion primitive yet —
                // approximate with a brief shield. TODO: real cloak
                // (shpilwav.cpp:89).
                commands.entity(entity).insert(ShieldActive {
                    remaining: 2.5,
                    damage_factor: 0.0,
                });
                cooldown.0 = 7.0 / 20.0; // SpecialRate=7 → 0.35 s
                info!("P{} cloak", ship.player_slot + 1);
            }
            ShipClass::Thrto => {
                // shpthrto.cpp activate_special: accelerate(this,
                // angle, specialThrust, MAX_SPEED) — adds canonical
                // velocity (.ini Special Thrust=8 → 8·9.6 = 76.8 u/s)
                // and drops a stationary ThraddashFlame damage zone
                // *behind* the ship at pos - unit_vector(angle)*size.x/2.5.
                // Special Damage=2, Armour=2; Frames=39, frame_size=100
                // → ~3.9 s lifetime in the legacy.
                vel.0 += forward * (8.0 * SC2_VEL_SCALE);
                let trail_pos = pos.0 - forward * 18.0;
                spawn_damage_zone(
                    &mut commands,
                    Some(entity),
                    trail_pos,
                    18.0,
                    8.0,
                    3.9,
                    Color::srgba(1.0, 0.5, 0.1, 0.5),
                );
                cooldown.0 = 0.1; // SpecialRate=0 → every frame; floor for safety
                info!("P{} afterburner + flame", ship.player_slot + 1);
            }
            ShipClass::Vuxin => {
                // shpvuxin.cpp activate_special: spawn VuxLimpet from
                // Vector2(0, -size.y/2.8) — back of ship — aimed at
                // the ship's current target (or directly backward when
                // there is none). .ini Special: Velocity=25,
                // Range=35, Slowdown=0.5. We don't track targets yet,
                // so spawn backward like Spathi BUTT but with the
                // is_limpet flag set so on-hit it sticks (extra mass)
                // via the existing limpet pipeline.
                let limpet = WeaponSpec {
                    local_direction: Vec2::new(0.0, -1.0),
                    muzzle_offset: 16.0,
                    barrels: &[],
                    random_spread_rad: 0.0,
                    speed: 25.0 * SC2_VEL_SCALE,
                    lifetime: (35.0 * SC2_RANGE_SCALE) / (25.0 * SC2_VEL_SCALE),
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 10.0,
                    // VUX SpecialSprites — Vuxin has no shot_b in its
                    // extracted pack; fall back to the primary art.
                    projectile_sprite: Some("ships/vuxin/sprites/shot_a01.png"),
                    recoil_impulse: 0.0,
                    homing_turn_rate: 0.0,
                    is_limpet: true,
                };
                spawn_projectile(&mut commands, &assets, entity, pos.0, rot, vel.0, &limpet, 0);
                cooldown.0 = 7.0 / 20.0; // SpecialRate=7 → 0.35 s
                info!("P{} limpet", ship.player_slot + 1);
            }
            ShipClass::Supbl => {
                // Strafe — hold L or R during the special to pick
                // sideways direction. 5600 N·s impulse perpendicular.
                let dir = if input.pressed(input::INPUT_LEFT) {
                    1.0
                } else if input.pressed(input::INPUT_RIGHT) {
                    -1.0
                } else {
                    0.0
                };
                if dir != 0.0 {
                    dash(&mut vel, perp * dir * 5600.0);
                    info!("P{} strafe", ship.player_slot + 1);
                }
                cooldown.0 = 1.0;
            }
            ShipClass::Kohma => {
                // shpkohma.cpp activate_special: F.R.I.E.D. — spawn
                // 16 KohrAhFRIED projectiles radiating outward at
                // i·(2π/16) - π for i in 0..16. .ini Special:
                // Velocity=20, Range=5, Damage=3, Armour=99.
                let speed = 20.0 * SC2_VEL_SCALE;
                let life = (5.0 * SC2_RANGE_SCALE) / speed;
                let fried = WeaponSpec {
                    local_direction: forward, // unused — barrels override
                    muzzle_offset: 0.0,
                    barrels: &[],
                    random_spread_rad: 0.0,
                    speed,
                    lifetime: life,
                    color: Color::srgb(1.0, 1.0, 1.0),
                    sprite_size: 8.0,
                    projectile_sprite: Some("ships/kohma/sprites/shot_b01.png"),
                    recoil_impulse: 0.0,
                    homing_turn_rate: 0.0,
                    is_limpet: false,
                };
                for i in 0..16 {
                    let theta = (i as f32) * std::f32::consts::TAU / 16.0 - std::f32::consts::PI;
                    let dir = Vec2::new(theta.cos(), theta.sin());
                    spawn_projectile_world(
                        &mut commands,
                        &assets,
                        entity,
                        pos.0 + dir * 16.0,
                        dir,
                        vel.0,
                        &fried,
                        3,
                    );
                }
                cooldown.0 = 9.0 / 20.0; // SpecialRate=9 → 0.45 s
                info!("P{} F.R.I.E.D.", ship.player_slot + 1);
            }
            ShipClass::Syrpe => {
                // shpsyrpe.cpp activate_special: siren song — within
                // specialRange (.ini=11 → 440 u), Syreen probabilistic
                // ally damages enemy *human* crew and spawns floating
                // CrewPod pickups that, if collected by the Syreen,
                // add crew. Needs target filtering by panel color and
                // pickup pods — TODO (shpsyrpe.cpp:661). Placeholder:
                // brake so the ship plants while the (silent) song
                // plays.
                drag(&mut vel, 8000.0);
                cooldown.0 = 20.0 / 20.0; // SpecialRate=20 → 1 s
                info!("P{} siren song (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Andgu => {
                // shpandgu.cpp activate_special: enter Blazer-comet
                // mode — set damage_factor=specialDamage, replace
                // sprite, swap mass to specialMass, swap turn_rate to
                // specialTurnRate. While active, thrust is forced full,
                // recharge_amount=-1 (drains battery every tick), and
                // collisions damage the target by specialDamage. Exits
                // when battery hits -1. Needs a runtime stat-swap +
                // collision-damage primitive — TODO (shpandgu.cpp:906).
                // Placeholder: a forward sprint.
                vel.0 += forward * (60.0 * SC2_VEL_SCALE) / ship.stats.mass.max(0.0001);
                cooldown.0 = 0.1;
                info!("P{} blazer comet (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Chebr => {
                // shpchebr.cpp activate_special: spawn ONE ChenjesuDOGI
                // sub-entity at Vector2(0, -size.y/1.5) (back of ship)
                // at angle+π. The DOGI is a small mobile mine with
                // homing-with-avoidance behaviour and fuel-sap-on-hit.
                // Sub-entity AI primitive doesn't exist; for now spawn
                // a single backward damage zone at the DOGI's launch
                // point. TODO: mobile DOGI (shpchebr.cpp:ChenjesuDOGI).
                let back = -forward * 38.0;
                spawn_damage_zone(
                    &mut commands,
                    Some(entity),
                    pos.0 + back,
                    24.0,
                    8.0,
                    6.0,
                    Color::srgba(0.7, 0.8, 1.0, 0.5),
                );
                cooldown.0 = 0.1;
                info!("P{} DOGI (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Druma => {
                // shpdruma.cpp calculate_fire_special: if crew > 1 and
                // batt < batt_max and recharge done, burn 1 crew and
                // add `special_drain` to battery (clamped to batt_max).
                // SpecialDrain=16 means each crewman becomes 16 batt.
                if crew.current > 1 && battery.current < battery.max {
                    crew.current -= 1;
                    battery.current = (battery.current + ship.stats.special_drain).min(battery.max);
                }
                // Refund the activation drain — the special's "cost"
                // is the burned crewman, not the battery (legacy
                // gates on `batt < batt_max` and adds *into* batt).
                battery.current = (battery.current + ship.stats.special_drain).min(battery.max);
                cooldown.0 = 30.0 / 20.0; // SpecialRate=30 → 1.5 s
                info!("P{} crew-burn", ship.player_slot + 1);
            }
            ShipClass::Utwju => {
                // shputwju.cpp handle_damage: while special_recharge > 0
                // incoming `normal` damage is *added to batt* instead of
                // deducted from crew (i.e. fortitude — absorbs hits and
                // converts them to energy). We approximate with a brief
                // total-immunity ShieldActive. TODO: damage-to-battery
                // conversion primitive (shputwju.cpp:96).
                commands.entity(entity).insert(ShieldActive {
                    remaining: 2.0,
                    damage_factor: 0.0,
                });
                cooldown.0 = 7.0 / 20.0; // SpecialRate=7 → 0.35 s
                info!("P{} fortitude shield", ship.player_slot + 1);
            }
            ShipClass::Zfpst => {
                // shpzfpst.cpp activate_special: spawn ZoqFotPikTongue
                // at dist=39 ahead of ship; it's a ship-attached
                // SpaceObject that ticks for 6 frames (~0.3 s) doing
                // specialDamage per tick. Licking=0 (set in .ini) →
                // damage stays positive (flat damage). We don't have
                // ship-attached damage objects yet — approximate with
                // a short-lived damage zone in front of the ship.
                let tip = pos.0 + forward * 39.0;
                spawn_damage_zone(
                    &mut commands,
                    Some(entity),
                    tip,
                    20.0,
                    12.0 * 20.0,
                    0.3,
                    Color::srgba(1.0, 0.5, 0.3, 0.55),
                );
                cooldown.0 = 6.0 / 20.0; // SpecialRate=6 → 0.3 s
                info!("P{} tongue", ship.player_slot + 1);
            }
            ShipClass::Mmrxf => {
                // shpmmrxf.cpp activate_special: form-toggle between
                // T_FORM (twin laser, slower) and Y_FORM (twin homing
                // missiles, faster). The toggle copies form_data[form]
                // → ship stats (speed_max/accel/turn/sprite/etc.).
                // Mode-toggle primitive doesn't exist yet — TODO
                // (shpmmrxf.cpp:435).
                cooldown.0 = 0.1;
                info!("P{} transform (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Orzne => {
                // shporzne.cpp activate_special: spawn an OrzMarine
                // sub-entity (costs 1 crew) that flies out, attaches
                // to the nearest enemy, and drains crew on contact.
                // Sub-entity AI primitive doesn't exist — TODO
                // (shporzne.cpp:564). The same special also drives the
                // turret-aim controls (left/right while held) but our
                // turret isn't independent. Placeholder: burn 1 crew
                // to mirror the canonical cost.
                if crew.current > 1 {
                    crew.current -= 1;
                }
                cooldown.0 = 12.0 / 20.0; // SpecialRate=12 → 0.6 s
                info!("P{} marines (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Slypr => {
                // shpslypr.cpp: the Slylandro Probe's only refill is
                // harvesting an asteroid (collision-based). There are
                // no asteroids in the arena yet — TODO when
                // mcbodies-style world bodies arrive. No-op.
                cooldown.0 = 20.0 / 20.0; // SpecialRate=20 → 1 s
                info!("P{} harvest (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Umgdr => {
                // shpumgdr.cpp activate_special: a Newtonian slingshot
                // — `vel = 0; pos -= unit_vector(angle) * size.x * 2`.
                // It's not a thrust impulse, it's a *teleport* of 2·size
                // backwards plus a hard stop. We model size.x as the
                // collider diameter ≈ 24, so 2·size.x ≈ 48 units back.
                pos.0 -= forward * 48.0;
                vel.0 = Vec2::ZERO;
                cooldown.0 = 2.0 / 20.0; // SpecialRate=2 → 0.1 s
                info!("P{} anti-grav slingshot", ship.player_slot + 1);
            }
            ShipClass::Meltr => {
                // shpmeltr.cpp activate_special: forward MelnormeSpecial
                // — a confusion shot that disables the target's
                // controls for specialFrames frames. Disable-input
                // primitive doesn't exist — TODO (shpmeltr.cpp:1215).
                // Placeholder: forward dash so the button does
                // *something* until the real ability lands.
                vel.0 += forward * (120.0 * SC2_VEL_SCALE) / ship.stats.mass.max(0.0001) * 0.2;
                cooldown.0 = 20.0 / 20.0; // SpecialRate=20 → 1 s
                info!("P{} confusion (placeholder)", ship.player_slot + 1);
            }
        }
    }
}
