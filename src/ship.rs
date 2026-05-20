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

/// Stable order so 1..=6 and F1..=F6 hotkeys map deterministically.
pub const ALL_CLASSES: [ShipClass; 6] = [
    ShipClass::Earcr,
    ShipClass::Spael,
    ShipClass::Yehte,
    ShipClass::Chmav,
    ShipClass::Kzedr,
    ShipClass::Mycpo,
];

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
    /// Yehat Terminator — twin cannons, energy shield (TODO).
    Yehte,
    /// Chmmr Avatar — laser + orbiting defense satellites (TODO).
    Chmav,
    /// Ur-Quan Kzer-Za Dreadnought — fusion bolt + launch fighters (TODO).
    Kzedr,
    /// Mycon Podship — plasmoid (TODO).
    Mycpo,
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
            .add_systems(Update, class_picker_input);
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
/// mutable via the class-picker hotkeys (1..=6 → P1, F1..=F6 → P2).
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

/// 1..=6 → P1 class, F1..=F6 → P2 class. Changes apply on the next
/// rematch — switching mid-round would mean despawning the current
/// hull, which is jarring; better to commit to your pick.
fn class_picker_input(keys: Res<ButtonInput<KeyCode>>, mut config: ResMut<MatchConfig>) {
    let p1_keys = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ];
    let p2_keys = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
    ];
    for (i, key) in p1_keys.iter().enumerate() {
        if keys.just_pressed(*key) {
            config.p1_class = ALL_CLASSES[i];
            info!("P1 → {:?} (takes effect next rematch)", config.p1_class);
        }
    }
    for (i, key) in p2_keys.iter().enumerate() {
        if keys.just_pressed(*key) {
            config.p2_class = ALL_CLASSES[i];
            info!("P2 → {:?} (takes effect next rematch)", config.p2_class);
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
) {
    for e in &ships {
        commands.entity(e).despawn();
    }
    for e in &projectiles {
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
    );
    let visual = (
        Sprite::from_image(initial),
        Transform::from_translation(position.extend(0.0)),
    );
    let phys = physics_spec(class, stats);
    let physics = (
        RigidBody::Dynamic,
        Collider::circle(phys.collider_radius),
        Mass(phys.mass),
        Rotation::radians(rotation_rad),
        LinearDamping(phys.linear_damping),
        AngularDamping(phys.angular_damping),
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

/// Read keyboard for this peer's slot and write the ship's persistent
/// thrust force + steering torque components. The physics solver reads
/// these every step, so we never touch velocities directly — that's
/// what gives us angular momentum "for free": a collision impulse at an
/// off-centre contact point spins the ship up against our torque, and
/// the player has to actively cancel the spin.
///
/// Force/torque magnitudes are scaled from the ship's stats so .ini
/// tuning still drives behaviour. Damping (in `spawn_ship`) is what
/// sets the terminal turn rate / cruise speed.
fn apply_player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut q: Query<(&Ship, &mut ConstantLocalForce, &mut ConstantTorque)>,
) {
    // Tuning knobs — moved into a Resource later so different ships can
    // override per-class. For now uniform constants get us in the ballpark.
    const THRUST_GAIN: f32 = 4000.0;
    const TORQUE_GAIN: f32 = 2.0e6;

    for (ship, mut thrust, mut torque) in &mut q {
        let input = input::read_local_input(&keys, ship.player_slot);

        let turn = if input.pressed(input::INPUT_LEFT) {
            1.0
        } else if input.pressed(input::INPUT_RIGHT) {
            -1.0
        } else {
            0.0
        };
        torque.0 = turn * ship.stats.turn_rate * TORQUE_GAIN;

        thrust.0 = if input.pressed(input::INPUT_THRUST) {
            // Sprite faces +Y at zero rotation, so thrust along local +Y.
            Vec2::new(0.0, ship.stats.accel_rate * ship.stats.mass * THRUST_GAIN)
        } else {
            Vec2::ZERO
        };
    }
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

fn tick_weapon_cooldown(time: Res<Time>, mut q: Query<&mut WeaponCooldown>) {
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
        &LinearVelocity,
        &mut WeaponCooldown,
    )>,
) {
    for (entity, ship, class, pos, rot, vel, mut cooldown) in &mut q {
        if cooldown.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_FIRE) {
            continue;
        }

        let spec = primary_weapon(*class);
        let local_dir = spec.local_direction;
        let world_dir = Vec2::new(
            local_dir.x * rot.cos - local_dir.y * rot.sin,
            local_dir.x * rot.sin + local_dir.y * rot.cos,
        );
        let muzzle = pos.0 + world_dir * spec.muzzle_offset;
        let projectile_vel = vel.0 + world_dir * spec.speed;

        // SC2's [Weapon] Rate is in legacy 36 Hz ticks; convert to seconds.
        let cooldown_secs = if ship.stats.weapon_rate > 0 {
            ship.stats.weapon_rate as f32 / 36.0
        } else {
            0.25
        };
        cooldown.0 = cooldown_secs;

        let damage = ship.stats.weapon_damage.max(1);
        commands.spawn((
            Projectile {
                owner: entity,
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
    }
}

/// Per-class physics knobs. Defaults come from `ShipStats.mass`; this
/// layer adds the *feel* parameters that the .ini doesn't capture —
/// collider radius, how snappy the steering is (angular damping),
/// how much the ship coasts in space (linear damping).
///
/// High angular damping ≈ fighter-like instant-turn (SC2 stock feel).
/// Low angular damping + big mass ≈ boat-like inertia drift.
struct PhysicsSpec {
    collider_radius: f32,
    mass: f32,
    linear_damping: f32,
    angular_damping: f32,
}

fn physics_spec(class: ShipClass, stats: &ShipStats) -> PhysicsSpec {
    match class {
        ShipClass::Earcr => PhysicsSpec {
            collider_radius: 22.0,
            mass: stats.mass,
            linear_damping: 0.4,
            angular_damping: 4.0,
        },
        ShipClass::Spael => PhysicsSpec {
            // Eluder is small and twitchy — snappier turn, lower mass.
            collider_radius: 16.0,
            mass: stats.mass.max(1.0) * 0.8,
            linear_damping: 0.5,
            angular_damping: 6.0,
        },
        ShipClass::Yehte => PhysicsSpec {
            collider_radius: 20.0,
            mass: stats.mass,
            linear_damping: 0.4,
            angular_damping: 5.0,
        },
        ShipClass::Chmav => PhysicsSpec {
            // Chmmr is heavy and slow to rotate — feels weighty.
            collider_radius: 28.0,
            mass: stats.mass * 1.4,
            linear_damping: 0.5,
            angular_damping: 2.5,
        },
        ShipClass::Kzedr => PhysicsSpec {
            // Ur-Quan Dreadnought — biggest, boat-feel. Low angular
            // damping is what makes a hit visibly spin it; the giant
            // moment of inertia (mass × big radius) does the rest.
            collider_radius: 34.0,
            mass: stats.mass * 1.6,
            linear_damping: 0.6,
            angular_damping: 1.5,
        },
        ShipClass::Mycpo => PhysicsSpec {
            collider_radius: 22.0,
            mass: stats.mass,
            linear_damping: 0.4,
            angular_damping: 3.5,
        },
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
        },
        ShipClass::Spael => WeaponSpec {
            // BUTT missile — fires backwards as the Eluder runs away.
            local_direction: backward,
            muzzle_offset: 28.0,
            speed: 500.0,
            lifetime: 3.0,
            color: Color::srgb(1.0, 0.5, 0.7),
            sprite_size: 8.0,
        },
        ShipClass::Yehte => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 30.0,
            speed: 900.0,
            lifetime: 1.2,
            color: Color::srgb(0.8, 1.0, 0.4),
            sprite_size: 5.0,
        },
        ShipClass::Chmav => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 30.0,
            speed: 1200.0,
            lifetime: 0.8,
            color: Color::srgb(0.4, 0.9, 1.0),
            sprite_size: 4.0,
        },
        ShipClass::Kzedr => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 36.0,
            speed: 500.0,
            lifetime: 2.5,
            color: Color::srgb(0.6, 1.0, 0.6),
            sprite_size: 10.0,
        },
        ShipClass::Mycpo => WeaponSpec {
            local_direction: forward,
            muzzle_offset: 26.0,
            speed: 400.0,
            lifetime: 3.5,
            color: Color::srgb(1.0, 0.5, 0.3),
            sprite_size: 9.0,
        },
    }
}

fn tick_projectile_lifetime(
    mut commands: Commands,
    time: Res<Time>,
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
    time: Res<Time>,
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

fn tick_special_cooldown(time: Res<Time>, mut q: Query<&mut SpecialCooldown>) {
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
        match class {
            ShipClass::Earcr => {
                // Forward dash — short impulse, 1.5 s cooldown.
                vel.0 += forward * 350.0;
                cooldown.0 = 1.5;
                info!("P{} dash", ship.player_slot + 1);
            }
            ShipClass::Spael => {
                // Phase-jump backwards 200 units. The classic Spathi flee.
                pos.0 -= forward * 200.0;
                vel.0 *= 0.5; // bleed momentum so the warp feels distinct
                cooldown.0 = 2.5;
                info!("P{} warp", ship.player_slot + 1);
            }
            ShipClass::Yehte => {
                // Yehat energy shield — 2 s of 25% incoming damage, 5 s
                // cooldown. First special that affects *other* ships'
                // attacks rather than this ship's motion.
                commands.entity(entity).insert(ShieldActive {
                    remaining: 2.0,
                    damage_factor: 0.25,
                });
                cooldown.0 = 5.0;
                info!("P{} shield up", ship.player_slot + 1);
            }
            ShipClass::Chmav | ShipClass::Kzedr | ShipClass::Mycpo => {
                // TODO: tractor beam / fighters / plasmoid (M5+). For
                // now: free brake — kill 80% of velocity. Gives the
                // class something to do while the real abilities cook.
                vel.0 *= 0.2;
                cooldown.0 = 2.0;
            }
        }
    }
}
