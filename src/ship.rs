use avian2d::dynamics::rigid_body::forces::{ConstantLocalForce, ConstantTorque};
use avian2d::prelude::*;
use bevy::prelude::*;
use ini::Ini;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
    pub fn from_files(code: &str, ini_path: &Path, txt_path: &Path) -> Result<Self, String> {
        let ini = Ini::load_from_file(ini_path).map_err(|e| format!("ini {code}: {e}"))?;
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

        let description = fs::read_to_string(txt_path).unwrap_or_default();

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
            description,
        })
    }
}

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
fn spawn_damage_zone(
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
                fire_weapons.after(tick_weapon_cooldown),
                trigger_specials.after(tick_special_cooldown),
                tick_projectile_lifetime,
                steer_homing_projectiles,
                tick_damage_zones,
                handle_projectile_hits,
                handle_ship_collisions,
            ),
        )
        .add_systems(Update, swap_rotation_frame);
    }
}

pub fn load_ship_catalog(mut commands: Commands) {
    let dir = Path::new("assets/ships");
    let mut ships = HashMap::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            error!("failed to read {dir:?}: {e}");
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ini") {
            continue;
        }
        let code = match path.file_stem().and_then(|s| s.to_str()) {
            Some(c) => c.to_string(),
            None => continue,
        };
        let txt = path.with_extension("txt");
        match ShipStats::from_files(&code, &path, &txt) {
            Ok(stats) => {
                info!("loaded ship {} ({})", stats.code, stats.name);
                ships.insert(code, stats);
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
    spawn_class(
        &mut commands,
        &catalog,
        &assets,
        config.p1_class,
        Vec2::new(-300.0, 0.0),
        std::f32::consts::FRAC_PI_2,
        0,
    );
    spawn_class(
        &mut commands,
        &catalog,
        &assets,
        config.p2_class,
        Vec2::new(300.0, 0.0),
        -std::f32::consts::FRAC_PI_2,
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
    commands.spawn((gameplay, visual, physics));
}

fn load_rotation_frames(assets: &AssetServer, code: &str) -> Vec<Handle<Image>> {
    // Most ships have 40 or 64 frames named ship_s01..ship_sNN. We probe up
    // to 64 and keep whatever exists on disk; assets/<code>/manifest.json
    // has the authoritative list once we wire that loader up properly.
    let dir = Path::new("assets/ships").join(code).join("sprites");
    let mut frames = Vec::with_capacity(64);
    for i in 1..=64 {
        let name = format!("ship_s{i:02}.png");
        if dir.join(&name).exists() {
            frames.push(assets.load(format!("ships/{code}/sprites/{name}")));
        }
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
fn swap_rotation_frame(mut q: Query<(&Rotation, &ShipFrames, &mut Sprite)>) {
    for (rot, frames, mut sprite) in &mut q {
        if frames.frames.is_empty() {
            continue;
        }
        // Avian's Rotation stores cos/sin; recover the angle.
        let angle = rot.sin.atan2(rot.cos);
        // Map [-π, π] CCW into [0, N) CW frame index.
        let n = frames.frames.len() as f32;
        let mut idx = ((-angle) / std::f32::consts::TAU * n).rem_euclid(n) as usize;
        if idx >= frames.frames.len() {
            idx = 0;
        }
        sprite.image = frames.frames[idx].clone();
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
    mut q: Query<(
        Entity,
        &Ship,
        &ShipClass,
        &Position,
        &Rotation,
        &mut LinearVelocity,
        &mut WeaponCooldown,
    )>,
) {
    for (entity, ship, class, pos, rot, mut vel, mut cooldown) in &mut q {
        if cooldown.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_FIRE) {
            continue;
        }

        let spec = primary_weapon(*class);

        // SC2's [Weapon] Rate is in legacy 36 Hz ticks; convert to seconds.
        let cooldown_secs = if ship.stats.weapon_rate > 0 {
            ship.stats.weapon_rate as f32 / 36.0
        } else {
            0.25
        };
        cooldown.0 = cooldown_secs;

        let damage = ship.stats.weapon_damage.max(1);
        let world_dir =
            spawn_projectile(&mut commands, entity, pos.0, rot, vel.0, &spec, damage);

        // Recoil from firing: applied here (not in spawn_projectile)
        // because it's specifically the *firer's* reaction to the
        // launch impulse. Δv = recoil_impulse / mass keeps it
        // Newton-third-law correct across ship masses.
        if spec.recoil_impulse > 0.0 {
            vel.0 -= world_dir * spec.recoil_impulse / ship.stats.mass;
        }
    }
}

/// Spawn a projectile from a WeaponSpec, given the firing ship's pose
/// and velocity. Returns the world-space firing direction so callers
/// can apply recoil to the firer if the spec requests it.
fn spawn_projectile(
    commands: &mut Commands,
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
    let projectile_vel = ship_vel + world_dir * spec.speed;
    let mut ent = commands.spawn((
        Projectile {
            owner,
            damage,
            lifetime: spec.lifetime,
        },
        Sprite::from_color(spec.color, Vec2::splat(spec.sprite_size)),
        Transform::from_translation(muzzle.extend(0.5)),
        RigidBody::Dynamic,
        Collider::circle(spec.sprite_size * 0.5),
        Mass(0.5),
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
    world_dir
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

/// Per-class primary-weapon shape. The fields stay generic on purpose so
/// adding a class doesn't reshape callers — just add a match arm.
struct WeaponSpec {
    local_direction: Vec2,
    muzzle_offset: f32,
    speed: f32,
    lifetime: f32,
    color: Color,
    sprite_size: f32,
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
}

fn primary_weapon(class: ShipClass) -> WeaponSpec {
    let forward = Vec2::new(0.0, 1.0);
    let backward = Vec2::new(0.0, -1.0);
    match class {
        ShipClass::Earcr => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 28.0,
            speed: 700.0,
            lifetime: 2.0,
            color: Color::srgb(1.0, 0.9, 0.4),
            sprite_size: 6.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Spael => WeaponSpec {
            // Spathi primary is canonically a short-range fast-firing
            // forward cannon — *not* the backward missile. The famous
            // BUTT (Backward Utilizing Tracking Torpedo) is the
            // *special*, see trigger_specials. .ini stat: Velocity=96
            // (≈ 921 world units/s).
            local_direction: forward,
            muzzle_offset: 22.0,
            speed: 920.0,
            lifetime: 0.7,
            color: Color::srgb(1.0, 0.5, 0.7),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Yehte => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 30.0,
            speed: 900.0,
            lifetime: 1.2,
            color: Color::srgb(0.8, 1.0, 0.4),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Chmav => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 30.0,
            speed: 1200.0,
            lifetime: 0.8,
            color: Color::srgb(0.4, 0.9, 1.0),
            sprite_size: 4.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Kzedr => WeaponSpec {
            // Fusion bolt is a slow heavy shot with mild recoil
            // (≈ 25 m/s on a mass-32 Dreadnought).
            local_direction: forward,
            muzzle_offset: 36.0,
            speed: 500.0,
            lifetime: 2.5,
            color: Color::srgb(0.6, 1.0, 0.6),
            sprite_size: 10.0,
            recoil_impulse: 800.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Mycpo => WeaponSpec {
            // Mycon plasmoid — the iconic slow homing shot. Turn
            // rate is the headline ability; without it the plasmoid
            // is just a fat slow ball that misses everything.
            local_direction: forward,
            muzzle_offset: 26.0,
            speed: 400.0,
            lifetime: 3.5,
            color: Color::srgb(1.0, 0.5, 0.3),
            sprite_size: 9.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 1.5,
        },
        ShipClass::Shosc => WeaponSpec {
            // Shofixti gun — fast, low damage. Compensates for the
            // glass hull with shot rate, not punch.
            local_direction: forward,
            muzzle_offset: 22.0,
            speed: 1000.0,
            lifetime: 1.5,
            color: Color::srgb(0.6, 1.0, 1.0),
            sprite_size: 4.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Arisk => WeaponSpec {
            // Arilou's auto-aiming halo. Approximated as a fast straight
            // shot for now; real homing lives in M3 alongside Mycon
            // plasmoid logic.
            local_direction: forward,
            muzzle_offset: 22.0,
            speed: 800.0,
            lifetime: 1.5,
            color: Color::srgb(0.5, 1.0, 0.5),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Pkufu => WeaponSpec {
            // Pkunk fires a fast forward cone in the original. For now
            // a single bolt; the cone is one match-arm change away once
            // we add multi-projectile fire support.
            local_direction: forward,
            muzzle_offset: 22.0,
            speed: 900.0,
            lifetime: 1.5,
            color: Color::srgb(1.0, 0.6, 1.0),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Ilwav => WeaponSpec {
            // Ilwrath's flamethrower — short range, big damage.
            local_direction: forward,
            muzzle_offset: 30.0,
            speed: 600.0,
            lifetime: 1.0,
            color: Color::srgb(1.0, 0.4, 0.2),
            sprite_size: 8.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Thrto => WeaponSpec {
            // Thraddash bullet — small, fast, modest damage.
            local_direction: forward,
            muzzle_offset: 22.0,
            speed: 850.0,
            lifetime: 1.4,
            color: Color::srgb(1.0, 0.7, 0.2),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Vuxin => WeaponSpec {
            // VUX limpet — slow, sticky in canon; we treat it as a
            // chunky slow projectile for now. Real "attach + drag
            // velocity" lands with the joint-based mechanics in M3.
            local_direction: forward,
            muzzle_offset: 26.0,
            speed: 350.0,
            lifetime: 4.0,
            color: Color::srgb(0.4, 0.9, 0.3),
            sprite_size: 9.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Supbl => WeaponSpec {
            // Supox plasma grenade — slow lob in canon; here a fast
            // straight shot until we add ballistic arcs.
            local_direction: forward,
            muzzle_offset: 24.0,
            speed: 700.0,
            lifetime: 1.8,
            color: Color::srgb(0.5, 0.8, 1.0),
            sprite_size: 7.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Kohma => WeaponSpec {
            // Kohr-Ah cleansing flames — wide spread in canon; for
            // now single forward shot. Big damage to make ramming
            // viable like the original.
            local_direction: forward,
            muzzle_offset: 34.0,
            speed: 550.0,
            lifetime: 1.5,
            color: Color::srgb(1.0, 0.55, 0.15),
            sprite_size: 9.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Syrpe => WeaponSpec {
            // Syreen razor — fast straight shot. The siren song
            // ability is on the special button, not the primary.
            local_direction: forward,
            muzzle_offset: 26.0,
            speed: 800.0,
            lifetime: 1.6,
            color: Color::srgb(1.0, 0.85, 0.95),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Andgu => WeaponSpec {
            // Androsynth bubble shot — slow, big, persistent.
            local_direction: forward,
            muzzle_offset: 24.0,
            speed: 450.0,
            lifetime: 2.5,
            color: Color::srgb(0.7, 0.7, 1.0),
            sprite_size: 8.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Chebr => WeaponSpec {
            // Chenjesu crystal shard cluster — single shot for now.
            local_direction: forward,
            muzzle_offset: 32.0,
            speed: 700.0,
            lifetime: 1.5,
            color: Color::srgb(0.9, 0.8, 1.0),
            sprite_size: 6.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Druma => WeaponSpec {
            // Druuge cannon — slow heavy shell. The classic Druuge
            // identity is the recoil: firing physically shoves the
            // ship backward. recoil_impulse value is in N·s, so a
            // mass-18 Druuge gets ≈ 110 m/s of kick per shot, but a
            // mass-2 Shofixti (if it had this cannon) would get 1000
            // m/s — Newton's third law in action.
            local_direction: forward,
            muzzle_offset: 30.0,
            speed: 600.0,
            lifetime: 2.5,
            color: Color::srgb(1.0, 0.3, 0.1),
            sprite_size: 10.0,
            recoil_impulse: 2000.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Utwju => WeaponSpec {
            // Utwig dual prong — fast forward shot.
            local_direction: forward,
            muzzle_offset: 28.0,
            speed: 900.0,
            lifetime: 1.5,
            color: Color::srgb(0.8, 0.7, 1.0),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Zfpst => WeaponSpec {
            // Zoq-Fot-Pik tongue lash — short, fast. Real tongue is a
            // melee swipe; here treated as a very short-range fast
            // shot until we add melee-style hitboxes.
            local_direction: forward,
            muzzle_offset: 20.0,
            speed: 1100.0,
            lifetime: 0.6,
            color: Color::srgb(1.0, 0.7, 0.5),
            sprite_size: 4.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Mmrxf => WeaponSpec {
            // X-Form lasers — fast forward beam shot.
            local_direction: forward,
            muzzle_offset: 24.0,
            speed: 1100.0,
            lifetime: 1.0,
            color: Color::srgb(0.6, 0.8, 1.0),
            sprite_size: 4.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Orzne => WeaponSpec {
            // Orz "flexible arm" — extendable cannon. For now a
            // single forward shot until we have multi-stage projectiles.
            local_direction: forward,
            muzzle_offset: 28.0,
            speed: 700.0,
            lifetime: 1.5,
            color: Color::srgb(0.8, 0.9, 0.6),
            sprite_size: 6.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Slypr => WeaponSpec {
            // Slylandro lightning — homes in canon; straight-line
            // placeholder until homing projectile lands with Mycon
            // plasmoid in M3.
            local_direction: forward,
            muzzle_offset: 18.0,
            speed: 600.0,
            lifetime: 2.0,
            color: Color::srgb(1.0, 1.0, 0.5),
            sprite_size: 5.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Umgdr => WeaponSpec {
            // Umgah anti-grav cone — fan of short-range projectiles
            // in canon; single forward shot placeholder.
            local_direction: forward,
            muzzle_offset: 18.0,
            speed: 500.0,
            lifetime: 0.8,
            color: Color::srgb(0.5, 1.0, 0.7),
            sprite_size: 7.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
        },
        ShipClass::Meltr => WeaponSpec {
            // Melnorme chargeable plasma — held-fire charges in canon.
            // For now a fixed-power forward shot until we wire up
            // press-to-charge / release-to-fire input semantics.
            local_direction: forward,
            muzzle_offset: 26.0,
            speed: 800.0,
            lifetime: 1.5,
            color: Color::srgb(0.9, 0.5, 1.0),
            sprite_size: 6.0,
            recoil_impulse: 0.0,
            homing_turn_rate: 0.0,
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
    shields: Query<&ShieldActive>,
    mut crews: Query<&mut Crew>,
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
    mut q: Query<(
        Entity,
        &Ship,
        &ShipClass,
        &mut Position,
        &Rotation,
        &mut LinearVelocity,
        &mut SpecialCooldown,
    )>,
) {
    for (entity, ship, class, mut pos, rot, mut vel, mut cooldown) in &mut q {
        if cooldown.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_SPECIAL) {
            continue;
        }

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
                // Forward thruster burst — 5000 N·s puts Δv ≈ 357 m/s
                // on a 14 kg Cruiser, ≈ 156 m/s on a 32 kg Dreadnought
                // if it ever borrows the ability.
                dash(&mut vel, forward * 5000.0);
                cooldown.0 = 1.5;
                info!("P{} dash", ship.player_slot + 1);
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
                    speed: 432.0, // Velocity=45 × distance_ratio / time_ratio
                    lifetime: 1.1, // ≈ Range=12 ÷ Velocity at scale
                    color: Color::srgb(1.0, 0.5, 0.7),
                    sprite_size: 7.0,
                    recoil_impulse: 0.0,
                    // .ini Special TurnRate=1 → scale_turning gives
                    // (2π/16) / (1+1) / 0.05 ≈ 3.93 rad/s.
                    homing_turn_rate: 3.93,
                };
                spawn_projectile(&mut commands, entity, pos.0, rot, vel.0, &butt, 2);
                cooldown.0 = 1.2;
                info!("P{} BUTT", ship.player_slot + 1);
            }
            ShipClass::Yehte => {
                commands.entity(entity).insert(ShieldActive {
                    remaining: 2.0,
                    damage_factor: 0.25,
                });
                cooldown.0 = 5.0;
                info!("P{} shield up", ship.player_slot + 1);
            }
            ShipClass::Chmav | ShipClass::Kzedr | ShipClass::Mycpo => {
                // Placeholder brake — 4000 N·s of drag impulse so a
                // heavy ship needs longer to halt than a light one,
                // which is the whole point of asking the physics to
                // do this work. Real abilities (tractor / fighters /
                // plasmoid) land with M3 primitives.
                drag(&mut vel, 4000.0);
                cooldown.0 = 2.0;
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
                // Pure teleport — non-physical, position write is
                // the only correct primitive. Velocity zeroed because
                // hyperspace cancels prior momentum in lore.
                pos.0 += perp * 350.0;
                vel.0 = Vec2::ZERO;
                cooldown.0 = 3.0;
                info!("P{} hyperspace", ship.player_slot + 1);
            }
            ShipClass::Pkufu => {
                commands.entity(entity).insert(ShieldActive {
                    remaining: 1.0,
                    damage_factor: 0.0,
                });
                cooldown.0 = 4.0;
                info!("P{} phase shift", ship.player_slot + 1);
            }
            ShipClass::Ilwav => {
                commands.entity(entity).insert(ShieldActive {
                    remaining: 2.5,
                    damage_factor: 0.0,
                });
                cooldown.0 = 6.0;
                info!("P{} cloak", ship.player_slot + 1);
            }
            ShipClass::Thrto => {
                // Afterburner burst — 3500 N·s gives a 7 kg Torch
                // ≈ 500 m/s sprint.
                dash(&mut vel, forward * 3500.0);
                cooldown.0 = 2.0;
                info!("P{} afterburner", ship.player_slot + 1);
            }
            ShipClass::Vuxin => {
                // VUX hit-and-stop — heavy drag (5000 N·s) brakes a
                // 10 kg Intruder by ≈ 500 m/s. Limpet attachment is
                // M3 work.
                drag(&mut vel, 5000.0);
                cooldown.0 = 2.0;
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
                // F.R.I.E.D. sawblades — a damage ring around the
                // Marauder. Stationary at spawn position for now;
                // making it follow the ship needs a parent-child
                // transform relationship which lands with the
                // satellite/orbiter primitive for Chmmr. Source set
                // to the firer so the Marauder can fly through its
                // own blades. Brake first so the ship is anchored
                // while the ring is up — matches the canon "Kohr-Ah
                // stops to spin sawblades" stance.
                drag(&mut vel, 20_000.0);
                spawn_damage_zone(
                    &mut commands,
                    Some(entity),
                    pos.0,
                    95.0,
                    20.0,
                    1.5,
                    Color::srgba(1.0, 0.5, 0.1, 0.4),
                );
                cooldown.0 = 4.0;
                info!("P{} F.R.I.E.D.", ship.player_slot + 1);
            }
            ShipClass::Syrpe => {
                drag(&mut vel, 20_000.0);
                cooldown.0 = 4.0;
                info!("P{} siren song (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Andgu => {
                // Blazer-comet — 5850 N·s on a 9 kg Guardian for
                // a 650 m/s sprint.
                dash(&mut vel, forward * 5850.0);
                cooldown.0 = 3.0;
                info!("P{} comet", ship.player_slot + 1);
            }
            ShipClass::Chebr => {
                // DOGI mines — five stationary damage zones in a
                // ring around the Broodhome. The canonical DOGI is
                // a small mobile entity that homes on enemies; here
                // we get the area-denial half of the mechanic
                // (stand in the wrong place, take damage) without
                // the mobile-mine sub-entity AI, which lands when
                // the sub-entity primitive does. Source = self so
                // the Chenjesu can fly through its own minefield.
                let count = 5;
                let mine_dist = 70.0;
                for i in 0..count {
                    let angle = std::f32::consts::TAU * (i as f32) / (count as f32);
                    let offset = Vec2::new(angle.cos(), angle.sin()) * mine_dist;
                    spawn_damage_zone(
                        &mut commands,
                        Some(entity),
                        pos.0 + offset,
                        24.0,
                        6.0,
                        7.0,
                        Color::srgba(0.7, 0.8, 1.0, 0.5),
                    );
                }
                cooldown.0 = 5.0;
                info!("P{} DOGI minefield", ship.player_slot + 1);
            }
            ShipClass::Druma => {
                // Ship-jump thruster — 4500 N·s on an 18 kg Mauler
                // ≈ 250 m/s. The iconic cannon recoil lives in
                // fire_weapons via spec.recoil_impulse.
                dash(&mut vel, forward * 4500.0);
                cooldown.0 = 1.0;
                info!("P{} ship jump", ship.player_slot + 1);
            }
            ShipClass::Utwju => {
                commands.entity(entity).insert(ShieldActive {
                    remaining: 2.0,
                    damage_factor: 0.0,
                });
                cooldown.0 = 5.0;
                info!("P{} ricochet shield", ship.player_slot + 1);
            }
            ShipClass::Zfpst => {
                // Taunt-dash on a 5 kg Stinger — 1500 N·s ≈ 300 m/s.
                dash(&mut vel, forward * 1500.0);
                cooldown.0 = 2.0;
                info!("P{} taunt", ship.player_slot + 1);
            }
            ShipClass::Mmrxf => {
                dash(&mut vel, forward * 3850.0);
                cooldown.0 = 2.0;
                info!("P{} transform (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Orzne => {
                dash(&mut vel, forward * 4500.0);
                cooldown.0 = 2.0;
                info!("P{} marines (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Slypr => {
                // Canonical Slylandro Probe special: harvest a nearby
                // asteroid to instantly refill the battery — the ship's
                // *only* way to refuel. There are no asteroids in the
                // arena yet (and no battery resource either; we just
                // track Crew), so this is a no-op with a log line.
                // Lights up properly once the battery system + asteroid
                // bodies land in M5.
                cooldown.0 = 1.0;
                info!("P{} harvest (placeholder)", ship.player_slot + 1);
            }
            ShipClass::Umgdr => {
                // Anti-grav slingshot — reverse impulse. 4000 N·s
                // on an 8 kg Drone ≈ 500 m/s backwards. First
                // negative-direction dash; cleanly the same shape
                // as positive ones thanks to the impulse closure.
                dash(&mut vel, -forward * 4000.0);
                cooldown.0 = 2.0;
                info!("P{} anti-grav", ship.player_slot + 1);
            }
            ShipClass::Meltr => {
                dash(&mut vel, forward * 3600.0);
                cooldown.0 = 2.0;
                info!("P{} confusion (placeholder)", ship.player_slot + 1);
            }
        }
    }
}
