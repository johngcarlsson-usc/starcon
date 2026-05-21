//! Data-driven ability layer.
//!
//! A ship's primary + special are expressed as `AbilitySpec` data
//! structures; one generic dispatcher (`dispatch_primary` /
//! `dispatch_special`) reads them and produces the right ECS spawns.
//! This is the alternative to the per-class match arms in
//! `fire_weapons` / `trigger_specials`.
//!
//! During the migration the two layers coexist: a ship has match-arm
//! behaviour by default, and opts in to data-driven by being given a
//! `ShipAbilities` component at spawn. The match-arm queries filter
//! `Without<ShipAbilities>` so a converted ship goes through the
//! dispatcher exclusively (no double-firing).
//!
//! See `docs/EXTENSIBILITY.md` — section "1. Composable abilities".
//!
//! Vocabulary today: `SpawnProjectiles` (with N barrels + spread +
//! homing + limpet + recoil), `GrantPointDefense`, `GrantShield`,
//! `Todo` (logs an ident, no-op — for ships whose canonical behaviour
//! needs a primitive that doesn't exist yet). Each new generic
//! primitive that lands gets one variant here + one match arm in
//! `apply_ability`, and is then available to every ship's manifest.

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::input;
use crate::ship::{
    Barrel, Battery, Homing, Limpet, PointDefenseActive, Projectile, ShieldActive, Ship,
    SpecialCooldown, WeaponCooldown,
};

/// Per-ship behaviour manifest. Present on entities that have been
/// converted away from the per-class match arms in `ship.rs`.
#[derive(Component, Debug, Clone)]
pub struct ShipAbilities {
    pub primary: AbilitySpec,
    pub special: AbilitySpec,
}

/// One ability — what happens when the corresponding button is pressed.
/// `cooldown_s` is set onto the shared `WeaponCooldown`/`SpecialCooldown`
/// timer after a successful activation, so the existing cooldown-tick
/// systems still apply.
#[derive(Debug, Clone)]
pub struct AbilitySpec {
    pub kind: AbilityKind,
    pub cooldown_s: f32,
}

#[derive(Debug, Clone)]
pub enum AbilityKind {
    /// Spawn one or more volleys of projectiles. Each volley fires one
    /// shot per `Barrel`, so multi-gun weapons (Yehat twin missiles,
    /// Pkunk triple, Utwig six-shot, Mmrnmhrm Y-Form twin homers) are
    /// expressed as a single `SpawnProjectiles` with N barrels.
    SpawnProjectiles { volleys: Vec<VolleySpec> },

    /// Attach a `PointDefenseActive` component to the firer. Canonical
    /// Earthling Cruiser special (shpearcr.cpp).
    GrantPointDefense {
        range: f32,
        damage_per_tick: i32,
        duration_s: f32,
    },

    /// Attach a `ShieldActive` component to the firer. Canonical Yehat
    /// shield, Ilwrath cloak placeholder, Utwig fortitude placeholder.
    GrantShield {
        duration_s: f32,
        damage_factor: f32,
    },

    /// Behaviour that needs a primitive we haven't built yet. The
    /// dispatcher logs the ident once per activation and applies no
    /// effect (cooldown still ticks so the player doesn't see input
    /// lag). Lets us convert a ship to data-driven without waiting for
    /// every primitive to land — the missing one becomes a focused
    /// follow-up PR. See `docs/EXTENSIBILITY.md` for the list.
    Todo { ident: &'static str },
}

/// Shared projectile parameters for a single volley. One volley
/// produces `barrels.len()` projectiles per activation.
#[derive(Debug, Clone)]
pub struct VolleySpec {
    pub barrels: Vec<Barrel>,
    /// Random angle jitter per shot in radians (ZFP-style spread).
    pub random_spread_rad: f32,
    pub speed: f32,
    pub lifetime: f32,
    pub color: Color,
    pub sprite_size: f32,
    /// Asset path relative to `assets/` for the projectile sprite.
    /// `None` falls back to a tinted square (placeholder).
    pub sprite_path: Option<String>,
    /// Rad/sec turn cap if the projectile should track the nearest
    /// enemy. Zero = straight-line.
    pub homing_turn_rate: f32,
    /// On hit, transfer mass to the target via the `Limpet` pipeline
    /// instead of dealing crew damage.
    pub is_limpet: bool,
    /// Newton's-third-law kickback applied to the firing ship per shot.
    pub recoil_impulse: f32,
}

pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, (dispatch_primary, dispatch_special));
    }
}

fn dispatch_primary(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    mut q: Query<(
        Entity,
        &Ship,
        &ShipAbilities,
        &Position,
        &Rotation,
        &mut LinearVelocity,
        &mut WeaponCooldown,
        &mut Battery,
    )>,
) {
    for (entity, ship, abilities, pos, rot, mut vel, mut cd, mut batt) in &mut q {
        if cd.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_FIRE) {
            continue;
        }
        if ship.stats.weapon_drain > 0 && batt.current < ship.stats.weapon_drain {
            continue;
        }
        batt.current = (batt.current - ship.stats.weapon_drain).max(0);
        let damage = ship.stats.weapon_damage.max(1);
        apply_ability(
            &mut commands,
            &assets,
            entity,
            ship,
            pos,
            rot,
            &mut vel,
            damage,
            &abilities.primary,
        );
        cd.0 = abilities.primary.cooldown_s;
    }
}

fn dispatch_special(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<AssetServer>,
    mut q: Query<(
        Entity,
        &Ship,
        &ShipAbilities,
        &Position,
        &Rotation,
        &mut LinearVelocity,
        &mut SpecialCooldown,
        &mut Battery,
    )>,
) {
    for (entity, ship, abilities, pos, rot, mut vel, mut cd, mut batt) in &mut q {
        if cd.0 > 0.0 {
            continue;
        }
        let input = input::read_local_input(&keys, ship.player_slot);
        if !input.pressed(input::INPUT_SPECIAL) {
            continue;
        }
        if ship.stats.special_drain > 0 && batt.current < ship.stats.special_drain {
            continue;
        }
        batt.current = (batt.current - ship.stats.special_drain).max(0);
        // Specials reuse weapon_damage for projectile damage today;
        // when a per-ability damage override is needed (e.g. Syreen
        // siren song damage scales with range, not weapon_damage),
        // that lands as a `damage_override: Option<i32>` field on
        // `VolleySpec`. Not needed yet.
        let damage = ship.stats.weapon_damage.max(1);
        apply_ability(
            &mut commands,
            &assets,
            entity,
            ship,
            pos,
            rot,
            &mut vel,
            damage,
            &abilities.special,
        );
        cd.0 = abilities.special.cooldown_s;
    }
}

fn apply_ability(
    commands: &mut Commands,
    assets: &AssetServer,
    entity: Entity,
    ship: &Ship,
    pos: &Position,
    rot: &Rotation,
    vel: &mut LinearVelocity,
    damage: i32,
    spec: &AbilitySpec,
) {
    match &spec.kind {
        AbilityKind::SpawnProjectiles { volleys } => {
            for volley in volleys {
                spawn_volley(commands, assets, entity, ship, pos, rot, vel, damage, volley);
            }
        }
        AbilityKind::GrantPointDefense {
            range,
            damage_per_tick,
            duration_s,
        } => {
            commands.entity(entity).insert(PointDefenseActive {
                remaining: *duration_s,
                range: *range,
                damage_per_tick: *damage_per_tick,
            });
            info!("P{} point defense online", ship.player_slot + 1);
        }
        AbilityKind::GrantShield {
            duration_s,
            damage_factor,
        } => {
            commands.entity(entity).insert(ShieldActive {
                remaining: *duration_s,
                damage_factor: *damage_factor,
            });
            info!("P{} shield up", ship.player_slot + 1);
        }
        AbilityKind::Todo { ident } => {
            info!(
                "P{} ability '{}' has no engine primitive yet — no-op",
                ship.player_slot + 1,
                ident
            );
        }
    }
}

fn spawn_volley(
    commands: &mut Commands,
    assets: &AssetServer,
    entity: Entity,
    ship: &Ship,
    pos: &Position,
    rot: &Rotation,
    vel: &mut LinearVelocity,
    damage: i32,
    volley: &VolleySpec,
) {
    let mass = ship.stats.mass.max(0.0001);
    for barrel in &volley.barrels {
        let world_pos_offset = Vec2::new(
            barrel.local_pos.x * rot.cos - barrel.local_pos.y * rot.sin,
            barrel.local_pos.x * rot.sin + barrel.local_pos.y * rot.cos,
        );
        let mut world_dir = Vec2::new(
            barrel.direction.x * rot.cos - barrel.direction.y * rot.sin,
            barrel.direction.x * rot.sin + barrel.direction.y * rot.cos,
        );
        if volley.random_spread_rad > 0.0 {
            let jitter = (fastrand::f32() * 2.0 - 1.0) * volley.random_spread_rad;
            let (s, c) = jitter.sin_cos();
            world_dir = Vec2::new(
                world_dir.x * c - world_dir.y * s,
                world_dir.x * s + world_dir.y * c,
            );
        }
        spawn_one_projectile(
            commands,
            assets,
            entity,
            pos.0 + world_pos_offset,
            world_dir,
            vel.0,
            volley,
            damage,
        );
        if volley.recoil_impulse > 0.0 {
            vel.0 -= world_dir * volley.recoil_impulse / mass;
        }
    }
}

fn spawn_one_projectile(
    commands: &mut Commands,
    assets: &AssetServer,
    owner: Entity,
    muzzle: Vec2,
    world_dir: Vec2,
    ship_vel: Vec2,
    volley: &VolleySpec,
    damage: i32,
) {
    let projectile_vel = ship_vel + world_dir * volley.speed;
    // Sprite frame 1 points +Y (up). Rotate by the velocity vector's
    // angle minus π/2 so the art faces the direction of travel at spawn.
    let initial_angle = world_dir.y.atan2(world_dir.x) - std::f32::consts::FRAC_PI_2;

    let sprite = if let Some(path) = &volley.sprite_path {
        Sprite {
            image: assets.load(path.clone()),
            color: volley.color,
            custom_size: Some(Vec2::splat(volley.sprite_size)),
            ..default()
        }
    } else {
        Sprite::from_color(volley.color, Vec2::splat(volley.sprite_size))
    };

    let mut ent = commands.spawn((
        Projectile {
            owner,
            damage,
            lifetime: volley.lifetime,
        },
        sprite,
        Transform::from_translation(muzzle.extend(0.5)),
        RigidBody::Dynamic,
        Collider::circle(volley.sprite_size * 0.5),
        Mass(0.5),
        Rotation::radians(initial_angle),
        LinearVelocity(projectile_vel),
        AngularVelocity::ZERO,
        LinearDamping(0.0),
        AngularDamping(0.0),
        CollisionEventsEnabled,
    ));
    if volley.homing_turn_rate > 0.0 {
        ent.insert(Homing {
            target: None,
            turn_rate: volley.homing_turn_rate,
        });
    }
    if volley.is_limpet {
        ent.insert(Limpet);
    }
}
