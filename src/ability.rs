//! Data-driven ability layer.
//!
//! A ship's primary + special are expressed as `AbilitySpec` data
//! structures; one generic dispatcher (`dispatch_primary` /
//! `dispatch_special`) reads them and produces the right ECS spawns.
//! Every ship in the roster goes through this layer — there is no
//! per-class match arm anywhere in the gameplay loop.
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
    spawn_attached_damage_zone, spawn_beam, spawn_damage_zone, spawn_sub_entity, spawn_tractor,
    Barrel, Battery, Crew, DamageToBattery, Homing, Invisible, Limpet, ModeToggleRequest,
    PointDefenseActive, Projectile, ShieldActive, Ship, SpecialCooldown, SubEntityAi,
    WeaponCooldown,
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

    /// Canonical Syreen siren song: drain crew from every valid
    /// enemy within `range` world-units of the firer. Damage is
    /// proximity-weighted — close enemies lose more — capped at
    /// `max_drain` per target per cast, plus a small random bonus.
    /// (shpsyrpe.cpp activate_special.)
    DrainNearbyCrew { range: f32, max_drain: i32 },

    /// Fire one or more beams (sustained line damage). Each beam is
    /// owned by the firer and follows its pose; lives for `duration_s`,
    /// damaging the nearest enemy along its ray per tick. Canonical
    /// Chmmr laser, VUX laser, Arilou auto-aim halo.
    SpawnBeams { beams: Vec<BeamSpec> },

    /// Spawn a TractorBeam — pulls (or pushes) the nearest enemy in
    /// range. Canonical Chmmr Avatar tractor (shpchmav.cpp:87).
    SpawnTractor {
        local_origin: Vec2,
        range: f32,
        force_per_tick: f32,
        color: Color,
        width: f32,
        duration_s: f32,
    },

    /// Mark the firer `Invisible` for a duration — drops homing locks
    /// and auto-aim beams. Canonical Ilwrath cloak (shpilwav.cpp).
    GrantInvisibility { duration_s: f32 },

    /// Mark the firer as converting incoming projectile damage to
    /// battery for a duration. Canonical Utwig fortitude
    /// (shputwju.cpp:96, `batt += normal` while special_recharge > 0).
    GrantDamageToBattery { duration_s: f32, conversion: f32 },

    /// Cycle the firer's `ShipModes.current` to the next mode (wraps
    /// around at the end). `tick_ship_modes` notices the change and
    /// swaps the live ShipPhysicsDerived / Mass / ShipFrames /
    /// ShipAbilities components. Canonical Mmrnmhrm T↔Y transform
    /// and Androsynth normal↔Blazer.
    ///
    /// No-op if the firer has no `ShipModes` component.
    ToggleMode,

    /// Spawn a sub-entity (Chenjesu DOGI, Orz marine, Syreen crew
    /// pod, Kzer-Za fighter). Has its own physics body + sprite + HP
    /// and runs an AI variant for steering / target acquisition /
    /// on-contact behaviour. `initial_angle_offset` is radians CCW
    /// from ship-forward: `0` = out the nose, `π` = out the rear.
    SpawnSubEntity {
        local_offset: Vec2,
        initial_angle_offset: f32,
        initial_speed: f32,
        sprite_path: Option<String>,
        sprite_size: f32,
        color: Color,
        hp: i32,
        lifetime_s: f32,
        ai: SubEntityAiSpec,
    },

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

    /// Spawn an owner-attached damage zone — like `SpawnDamageZone`
    /// but the zone follows the firer each tick rather than staying
    /// where it was spawned. Canonical Umgah cone (`shpumgdr.cpp`)
    /// and ZFP tongue (`shpzfpst.cpp`). Friendly-fire immune to the
    /// owner unconditionally.
    SpawnAttachedDamageZone {
        local_offset: Vec2,
        radius: f32,
        damage_per_sec: f32,
        duration_s: f32,
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

    /// This ability is handled by a dedicated per-ship system (not the
    /// generic dispatcher). The dispatcher silently skips ships
    /// carrying this variant — useful for primaries that need
    /// per-tick state and on-release behaviour (Chenjesu crystal
    /// shatter, Melnorme charge-and-release).
    ManagedExternally { ident: &'static str },
}

/// Per-variant AI tuning for `SpawnSubEntity`. Each variant maps
/// directly to a `ship::SubEntityAi` constructor — kept separate so
/// the manifest carries plain data (no `Option<Entity>`-style runtime
/// state) while the runtime component carries the live AI.
#[derive(Debug, Clone)]
pub enum SubEntityAiSpec {
    HomeAndDetonate {
        turn_rate: f32,
        speed: f32,
        damage_on_hit: i32,
        batt_sap: i32,
    },
    AttachAndDrain {
        turn_rate: f32,
        speed: f32,
        crew_drain: i32,
    },
    // DriftAndCollect: owner_slot is filled in at spawn time from
    // ctx.ship, so the manifest doesn't have to know which slot the
    // firer is in.
    DriftAndCollect {
        crew_value: i32,
    },
}

/// One beam emitted by a `SpawnBeams` ability. World pose is derived
/// from the owner's transform each tick; this just specifies what to
/// emit.
#[derive(Debug, Clone)]
pub struct BeamSpec {
    pub local_origin: Vec2,
    pub local_dir: Vec2,
    pub range: f32,
    pub damage_per_tick: i32,
    pub color: Color,
    pub auto_aim: bool,
    pub duration_s: f32,
    pub width: f32,
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
    virt: Res<input::VirtualInput>,
    assets: Res<AssetServer>,
    mut rng: ResMut<crate::rng::GameRng>,
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
        Option<&crate::ultimate::MmrxfActive>,
        Option<&crate::ultimate::PkunkClone>,
        Option<&crate::ai::AiControlled>,
    )>,
) {
    for (entity, ship, abilities, mut pos, rot, mut vel, mut cd, mut batt, mut crew, mmrxf_active, pkunk_clone, ai) in &mut q {
        // While Mmrnmhrm's ultimate is active the tangled laser
        // owns the primary. Skip the normal Mmrxf beams so the
        // two don't stack.
        if mmrxf_active.is_some() {
            continue;
        }
        // Ships whose primary lives in a dedicated system get skipped
        // here so they fully own input handling + cooldown + drain
        // (Chenjesu crystal launcher, Melnorme charge-and-release).
        if matches!(abilities.primary.kind, AbilityKind::ManagedExternally { .. }) {
            continue;
        }
        if cd.0 > 0.0 {
            continue;
        }
        // Pkunk clones auto-fire continuously, and AI ships fire
        // every cooldown. Otherwise read the local input for
        // this player_slot.
        let force_fire = pkunk_clone.is_some() || ai.is_some();
        if !force_fire {
            let input = input::read_local_input_with_virtual(&keys, Some(&virt), ship.player_slot);
            if !input.pressed(input::INPUT_FIRE) {
                continue;
            }
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
            rng: &mut rng,
        };
        apply_kind(&mut ctx, &abilities.primary.kind);
        cd.0 = abilities.primary.cooldown_s;
    }
}

fn dispatch_special(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    virt: Res<input::VirtualInput>,
    assets: Res<AssetServer>,
    mut rng: ResMut<crate::rng::GameRng>,
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
        Option<&crate::ultimate::MmrxfActive>,
        Option<&crate::ultimate::PkunkClone>,
    )>,
) {
    for (entity, ship, abilities, mut pos, rot, mut vel, mut cd, mut batt, mut crew, mmrxf_active, pkunk_clone) in &mut q {
        // Mmrnmhrm ultimate replaces the special with the split
        // missile launcher — skip the form-toggle here.
        if mmrxf_active.is_some() {
            continue;
        }
        if cd.0 > 0.0 {
            continue;
        }
        // Some abilities (ToggleMode) want edge-trigger semantics —
        // a single key press should fire exactly once, even if the
        // player holds the key longer than the cooldown. Otherwise
        // a 100ms hold flips Mmrnmhrm form 2–3 times in the cooldown
        // window and lands back where it started.
        let edge_only = matches!(abilities.special.kind, AbilityKind::ToggleMode);
        // Pkunk clones force-press their special while in their
        // retreating sub-state (charging batteries away from the
        // enemy). Edge-only abilities still respect the edge —
        // RefillBattery is level-triggered, so this just means the
        // clone hammers special on cooldown until it's full.
        let force = pkunk_clone.map_or(false, |c| c.retreating) && !edge_only;
        let triggered = if force {
            true
        } else if edge_only {
            input::read_local_just_pressed_with_virtual(&keys, Some(&virt), ship.player_slot)
                .pressed(input::INPUT_SPECIAL)
        } else {
            input::read_local_input_with_virtual(&keys, Some(&virt), ship.player_slot)
                .pressed(input::INPUT_SPECIAL)
        };
        if !triggered {
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
            rng: &mut rng,
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
    /// Seeded per-match RNG. Use for any draw whose outcome
    /// affects game state (shot spread, teleport offset).
    /// Visual-only jitter can keep using the global fastrand.
    rng: &'a mut crate::rng::GameRng,
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
        AbilityKind::SpawnBeams { beams } => {
            for b in beams {
                spawn_beam(
                    ctx.commands,
                    ctx.entity,
                    ctx.pos.0,
                    ctx.rot,
                    b.local_origin,
                    b.local_dir,
                    b.range,
                    b.damage_per_tick,
                    b.color,
                    b.auto_aim,
                    b.duration_s,
                    b.width,
                );
            }
        }
        AbilityKind::SpawnTractor {
            local_origin,
            range,
            force_per_tick,
            color,
            width,
            duration_s,
        } => {
            spawn_tractor(
                ctx.commands,
                ctx.entity,
                ctx.pos.0,
                ctx.rot,
                *local_origin,
                *range,
                *force_per_tick,
                *color,
                *width,
                *duration_s,
            );
        }
        AbilityKind::GrantInvisibility { duration_s } => {
            ctx.commands.entity(ctx.entity).insert(Invisible {
                remaining: *duration_s,
            });
            info!("P{} cloaked", slot);
        }
        AbilityKind::GrantDamageToBattery {
            duration_s,
            conversion,
        } => {
            ctx.commands.entity(ctx.entity).insert(DamageToBattery {
                remaining: *duration_s,
                conversion: *conversion,
            });
            info!("P{} fortitude up", slot);
        }
        AbilityKind::ToggleMode => {
            ctx.commands.entity(ctx.entity).insert(ModeToggleRequest);
            info!("P{} mode toggle requested", slot);
        }
        AbilityKind::SpawnSubEntity {
            local_offset,
            initial_angle_offset,
            initial_speed,
            sprite_path,
            sprite_size,
            color,
            hp,
            lifetime_s,
            ai,
        } => {
            let runtime_ai = match ai {
                SubEntityAiSpec::HomeAndDetonate {
                    turn_rate,
                    speed,
                    damage_on_hit,
                    batt_sap,
                } => SubEntityAi::HomeAndDetonate {
                    target: None,
                    turn_rate: *turn_rate,
                    speed: *speed,
                    damage_on_hit: *damage_on_hit,
                    batt_sap: *batt_sap,
                },
                SubEntityAiSpec::AttachAndDrain {
                    turn_rate,
                    speed,
                    crew_drain,
                } => SubEntityAi::AttachAndDrain {
                    target: None,
                    turn_rate: *turn_rate,
                    speed: *speed,
                    crew_drain: *crew_drain,
                },
                SubEntityAiSpec::DriftAndCollect { crew_value } => {
                    SubEntityAi::DriftAndCollect {
                        owner_slot: ctx.ship.player_slot,
                        crew_value: *crew_value,
                    }
                }
            };
            spawn_sub_entity(
                ctx.commands,
                ctx.assets,
                ctx.entity,
                ctx.pos.0,
                ctx.rot,
                *local_offset,
                *initial_angle_offset,
                *initial_speed,
                sprite_path.as_deref(),
                *sprite_size,
                *color,
                *hp,
                *lifetime_s,
                runtime_ai,
            );
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
            // Determinism-critical: peers must agree on where
            // the teleporting ship reappears. Use the seeded
            // per-match RNG, not the global fastrand.
            let dx = ctx.rng.signed_unit() * range;
            let dy = ctx.rng.signed_unit() * range;
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
        AbilityKind::DrainNearbyCrew { range, max_drain } => {
            // Stamp a pending request on the firer; `apply_syreen_drain`
            // (in ship.rs) consumes it next FixedUpdate tick where it
            // has access to the full ship query.
            ctx.commands
                .entity(ctx.entity)
                .insert(crate::ship::SyreenDrainRequest {
                    range: *range,
                    max_drain: *max_drain,
                });
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
        AbilityKind::SpawnAttachedDamageZone {
            local_offset,
            radius,
            damage_per_sec,
            duration_s,
            color,
        } => {
            spawn_attached_damage_zone(
                ctx.commands,
                ctx.entity,
                ctx.pos.0,
                ctx.rot,
                *local_offset,
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
        AbilityKind::ManagedExternally { .. } => {
            // No-op here — a dedicated per-ship system reads input
            // for this ship and fires its primary/special directly.
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
            // Determinism-critical: shot spread directly
            // affects projectile trajectories.
            let jitter = ctx.rng.signed_unit() * volley.random_spread_rad;
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
        // Projectile mass scales with damage. The collision impulse
        // Avian imparts on the target is mass·velocity, so this gives
        // heavy weapons (Mycon plasmoid dmg=10, Kzer-Za fusion dmg=6,
        // Druuge cannon dmg=6) a real physical kick on top of their
        // crew damage — heavy hits visibly spin / shove the target
        // in Inertial mode. Spathi-pellet (dmg=1) stays at 0.5 kg
        // so machine-gun fire doesn't make ships endlessly tumble.
        Mass(0.5 + damage.max(0) as f32 * 0.4),
        // `Position` is required because `PhysicsTransformConfig::
        // transform_to_position` is disabled — without this Avian
        // would default the projectile to (0, 0) on its first sync
        // and the visual would briefly flash at the muzzle, then
        // teleport to world origin before flying off in the right
        // direction. (The "two streams of bullets" bug.)
        avian2d::prelude::Position(muzzle),
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
        // VUX-style: 0.5 = halve target speed on each hit. Canonical
        // .ini Vuxin Special.Slowdown=0.5. If we ever add a non-VUX
        // limpet with a different factor, lift this onto VolleySpec.
        ent.insert(Limpet {
            slowdown_factor: 0.5,
        });
    }
}
