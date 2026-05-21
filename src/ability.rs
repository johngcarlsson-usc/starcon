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
    spawn_damage_zone, Barrel, Battery, Crew, Homing, Limpet, PointDefenseActive, Projectile,
    ShieldActive, Ship, SpecialCooldown, WeaponCooldown,
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

    /// Δ crew on the firer, clamped to [0, crew_max]. Canonical Mycon
    /// repair (positive); also used for Druuge crew-burn at the
    /// negative end via `BurnCrewForBattery` which composes this with
    /// a battery refill.
    ModifyCrew { delta: i32 },

    /// Top off the firer's battery to `Battery::max`. Canonical Pkunk
    /// taunt (refunds the activation drain). The dispatcher's
    /// battery-gate already deducted `special_drain` before this fires;
    /// since the canonical effect is "net no cost, full batt", we just
    /// set current=max.
    RefillBattery,

    /// Burn `crew_cost` crew to add `batt_gain` to battery. Skips with
    /// no effect if the firer has ≤ 1 crew or the battery is already
    /// full. Canonical Druuge special (shpdruma.cpp:calculate_fire_special).
    BurnCrewForBattery { crew_cost: i32, batt_gain: i32 },

    /// Random teleport within ±range on each axis (Arilou hyperspace).
    /// No velocity change in canon — matches the legacy `translate(d)`.
    TeleportRandom { range: f32 },

    /// Translate by a *ship-local* offset (`offset` rotated by the
    /// ship's current rotation), optionally zeroing velocity. Canonical
    /// Umgah anti-grav slingshot: pos -= forward·2·size; vel=0.
    TeleportRelative { offset: Vec2, zero_velocity: bool },

    /// Apply an instantaneous impulse (N·s) along a ship-local
    /// direction. Δv = impulse / mass — light hulls leap, heavy hulls
    /// nudge. Generic dash / strafe primitive.
    ApplyImpulse { local_dir: Vec2, impulse: f32 },

    /// Drag-style velocity cap — bleed up to `max_dv` m/s off the
    /// firer's current velocity, opposite to its direction of travel.
    /// Canonical "stop and plant" for Syreen siren song placeholder
    /// and similar.
    BrakeImpulse { max_dv: f32 },

    /// Spawn a stationary damage zone at the firer's pose. `offset` is
    /// ship-local. `source_self` makes the zone immune to the firer
    /// (DOGI / Kohr-Ah blades use this); `false` is a self-damaging
    /// suicide blast (Shofixti Glory Device).
    SpawnDamageZone {
        offset: Vec2,
        radius: f32,
        damage_per_sec: f32,
        duration_s: f32,
        source_self: bool,
        color: Color,
    },

    /// Run a list of `AbilityKind`s in order. Lets a single ability
    /// compose primitives — e.g. Thraddash special is
    /// `Sequence([ApplyImpulse, SpawnDamageZone])`. Nested Sequences
    /// flatten naturally because each element re-enters apply_ability.
    Sequence(Vec<AbilityKind>),

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
        &mut Position,
        &Rotation,
        &mut LinearVelocity,
        &mut WeaponCooldown,
        &mut Battery,
        &mut Crew,
    )>,
) {
    for (entity, ship, abilities, mut pos, rot, mut vel, mut cd, mut batt, mut crew) in &mut q {
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
        let mut ctx = AbilityCtx {
            commands: &mut commands,
            assets: &assets,
            entity,
            ship,
            pos: &mut pos,
            rot,
            vel: &mut vel,
            batt: &mut batt,
            crew: &mut crew,
            damage,
        };
        apply_kind(&mut ctx, &abilities.primary.kind);
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
        &mut Position,
        &Rotation,
        &mut LinearVelocity,
        &mut SpecialCooldown,
        &mut Battery,
        &mut Crew,
    )>,
) {
    for (entity, ship, abilities, mut pos, rot, mut vel, mut cd, mut batt, mut crew) in &mut q {
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
        let mut ctx = AbilityCtx {
            commands: &mut commands,
            assets: &assets,
            entity,
            ship,
            pos: &mut pos,
            rot,
            vel: &mut vel,
            batt: &mut batt,
            crew: &mut crew,
            damage,
        };
        apply_kind(&mut ctx, &abilities.special.kind);
        cd.0 = abilities.special.cooldown_s;
    }
}

/// Plumbing for a single ability activation — passed to `apply_kind`
/// so each variant gets at the firer's pose, components, and the
/// command buffer without us re-listing 10 fn args per variant.
struct AbilityCtx<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    assets: &'a AssetServer,
    entity: Entity,
    ship: &'a Ship,
    pos: &'a mut Position,
    rot: &'a Rotation,
    vel: &'a mut LinearVelocity,
    batt: &'a mut Battery,
    crew: &'a mut Crew,
    damage: i32,
}

fn apply_kind(ctx: &mut AbilityCtx, kind: &AbilityKind) {
    let slot = ctx.ship.player_slot + 1;
    let forward = Vec2::new(-ctx.rot.sin, ctx.rot.cos);
    let mass = ctx.ship.stats.mass.max(0.0001);
    match kind {
        AbilityKind::SpawnProjectiles { volleys } => {
            for volley in volleys {
                spawn_volley(ctx, volley);
            }
        }
        AbilityKind::GrantPointDefense {
            range,
            damage_per_tick,
            duration_s,
        } => {
            ctx.commands.entity(ctx.entity).insert(PointDefenseActive {
                remaining: *duration_s,
                range: *range,
                damage_per_tick: *damage_per_tick,
            });
            info!("P{} point defense online", slot);
        }
        AbilityKind::GrantShield {
            duration_s,
            damage_factor,
        } => {
            ctx.commands.entity(ctx.entity).insert(ShieldActive {
                remaining: *duration_s,
                damage_factor: *damage_factor,
            });
            info!("P{} shield up", slot);
        }
        AbilityKind::ModifyCrew { delta } => {
            ctx.crew.current = (ctx.crew.current + delta).clamp(0, ctx.crew.max);
            info!("P{} crew {:+} → {}", slot, delta, ctx.crew.current);
        }
        AbilityKind::RefillBattery => {
            ctx.batt.current = ctx.batt.max;
            info!("P{} battery refilled", slot);
        }
        AbilityKind::BurnCrewForBattery {
            crew_cost,
            batt_gain,
        } => {
            if ctx.crew.current > *crew_cost && ctx.batt.current < ctx.batt.max {
                ctx.crew.current -= crew_cost;
                ctx.batt.current = (ctx.batt.current + batt_gain).min(ctx.batt.max);
                info!("P{} burned {} crew → +{} batt", slot, crew_cost, batt_gain);
            }
        }
        AbilityKind::TeleportRandom { range } => {
            let dx = (fastrand::f32() * 2.0 - 1.0) * range;
            let dy = (fastrand::f32() * 2.0 - 1.0) * range;
            ctx.pos.0 += Vec2::new(dx, dy);
            info!("P{} hyperspace", slot);
        }
        AbilityKind::TeleportRelative {
            offset,
            zero_velocity,
        } => {
            // Rotate the ship-local offset into world space.
            let world = Vec2::new(
                offset.x * ctx.rot.cos - offset.y * ctx.rot.sin,
                offset.x * ctx.rot.sin + offset.y * ctx.rot.cos,
            );
            ctx.pos.0 += world;
            if *zero_velocity {
                ctx.vel.0 = Vec2::ZERO;
            }
            info!("P{} translate", slot);
        }
        AbilityKind::ApplyImpulse { local_dir, impulse } => {
            let world_dir = Vec2::new(
                local_dir.x * ctx.rot.cos - local_dir.y * ctx.rot.sin,
                local_dir.x * ctx.rot.sin + local_dir.y * ctx.rot.cos,
            );
            ctx.vel.0 += world_dir * (*impulse / mass);
        }
        AbilityKind::BrakeImpulse { max_dv } => {
            let speed = ctx.vel.0.length();
            if speed > 0.0 {
                let dv = max_dv.min(speed);
                ctx.vel.0 -= ctx.vel.0 / speed * dv;
            }
        }
        AbilityKind::SpawnDamageZone {
            offset,
            radius,
            damage_per_sec,
            duration_s,
            source_self,
            color,
        } => {
            let world_offset = Vec2::new(
                offset.x * ctx.rot.cos - offset.y * ctx.rot.sin,
                offset.x * ctx.rot.sin + offset.y * ctx.rot.cos,
            );
            spawn_damage_zone(
                ctx.commands,
                source_self.then_some(ctx.entity),
                ctx.pos.0 + world_offset,
                *radius,
                *damage_per_sec,
                *duration_s,
                *color,
            );
        }
        AbilityKind::Sequence(steps) => {
            for step in steps {
                apply_kind(ctx, step);
            }
        }
        AbilityKind::Todo { ident } => {
            info!(
                "P{} ability '{}' has no engine primitive yet — no-op",
                slot, ident
            );
        }
    }
    // `forward` is intentionally unused in most arms; keep it computed
    // once at the top so individual arms (future Beam, AppliedForce,
    // attached-zones) can reach for it without recomputing.
    let _ = forward;
}

fn spawn_volley(ctx: &mut AbilityCtx, volley: &VolleySpec) {
    let mass = ctx.ship.stats.mass.max(0.0001);
    for barrel in &volley.barrels {
        let world_pos_offset = Vec2::new(
            barrel.local_pos.x * ctx.rot.cos - barrel.local_pos.y * ctx.rot.sin,
            barrel.local_pos.x * ctx.rot.sin + barrel.local_pos.y * ctx.rot.cos,
        );
        let mut world_dir = Vec2::new(
            barrel.direction.x * ctx.rot.cos - barrel.direction.y * ctx.rot.sin,
            barrel.direction.x * ctx.rot.sin + barrel.direction.y * ctx.rot.cos,
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
            ctx.commands,
            ctx.assets,
            ctx.entity,
            ctx.pos.0 + world_pos_offset,
            world_dir,
            ctx.vel.0,
            volley,
            ctx.damage,
        );
        if volley.recoil_impulse > 0.0 {
            ctx.vel.0 -= world_dir * volley.recoil_impulse / mass;
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
