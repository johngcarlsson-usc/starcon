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

/// Marker + per-instance data for an in-match ship entity.
#[derive(Component, Debug)]
pub struct Ship {
    pub stats: ShipStats,
    pub player_slot: usize,
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
        app.add_systems(
            FixedUpdate,
            (
                apply_player_input,
                tick_weapon_cooldown,
                fire_weapons.after(tick_weapon_cooldown),
                tick_projectile_lifetime,
                handle_projectile_hits,
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

/// Spawn the test scene: two Earthling Cruisers facing each other. Player
/// 1 (arrows + Z/X) on the left; player 2 (WASD + G/H) on the right. They
/// share a hull class for now — once we have a fleet picker, this becomes
/// data-driven.
pub fn spawn_match(
    mut commands: Commands,
    catalog: Res<ShipCatalog>,
    assets: Res<AssetServer>,
) {
    let Some(stats) = catalog.ships.get("earcr").cloned() else {
        error!("earcr missing from catalog");
        return;
    };
    let frames = load_rotation_frames(&assets, "earcr");
    if frames.is_empty() {
        error!("no rotation frames found for earcr");
        return;
    }

    spawn_ship(
        &mut commands,
        &stats,
        &frames,
        Vec2::new(-300.0, 0.0),
        std::f32::consts::FRAC_PI_2, // face +x (right)
        0,
    );
    spawn_ship(
        &mut commands,
        &stats,
        &frames,
        Vec2::new(300.0, 0.0),
        -std::f32::consts::FRAC_PI_2, // face -x (left)
        1,
    );
    info!("spawned 2 ships");
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
        Crew {
            current: stats.crew_max,
            max: stats.crew_max,
        },
        WeaponCooldown::default(),
        ShipFrames {
            frames: frames.to_vec(),
        },
    );
    let visual = (
        Sprite::from_image(initial),
        Transform::from_translation(position.extend(0.0)),
    );
    let physics = (
        RigidBody::Dynamic,
        Collider::circle(20.0),
        Mass(stats.mass),
        Rotation::radians(rotation_rad),
        LinearDamping(0.4),
        AngularDamping(4.0),
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
/// weapon is off cooldown. The projectile is a small fast dynamic body,
/// so when it hits a ship the physics solver computes the impulse-at-
/// contact correctly — the spin imparted to the target is free.
fn fire_weapons(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut q: Query<(
        Entity,
        &Ship,
        &Position,
        &Rotation,
        &LinearVelocity,
        &mut WeaponCooldown,
    )>,
) {
    const PROJECTILE_SPEED: f32 = 700.0;
    const PROJECTILE_LIFETIME: f32 = 2.0;
    const MUZZLE_OFFSET: f32 = 28.0;

    for (entity, ship, pos, rot, vel, mut cooldown) in &mut q {
        if cooldown.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_FIRE) {
            continue;
        }

        // Local +Y rotated into world space.
        let forward = Vec2::new(-rot.sin, rot.cos);
        let muzzle = pos.0 + forward * MUZZLE_OFFSET;
        let projectile_vel = vel.0 + forward * PROJECTILE_SPEED;

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
                lifetime: PROJECTILE_LIFETIME,
            },
            Sprite::from_color(Color::srgb(1.0, 0.9, 0.4), Vec2::new(6.0, 6.0)),
            Transform::from_translation(muzzle.extend(0.5)),
            RigidBody::Dynamic,
            Collider::circle(3.0),
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
            // Projectiles can't damage their own ship.
            continue;
        }

        if let Ok(mut crew) = crews.get_mut(other_entity) {
            crew.current = (crew.current - proj.damage).max(0);
            info!(
                "hit: -{} crew (now {}/{})",
                proj.damage, crew.current, crew.max
            );
        }
        commands.entity(proj_entity).despawn();
    }
}
