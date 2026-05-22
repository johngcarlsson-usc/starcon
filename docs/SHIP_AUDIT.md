# Canonical-fidelity audit

Per-ship audit of primary + special weapons against `tw-light` legacy
source (`/tmp/tw-light/tw-light-0.5/src/ships/shp*.cpp`) and the
extracted `.ini` data sheets. Plus a section on which engine systems
currently bypass Avian's collision pipeline and need migration.

## Conventions

- ✅ = matches canon
- 🟡 = approximate (functionally close, minor canonical deviations)
- 🔴 = significantly off / not implemented
- All speeds/ranges below in *world units*: `speed = velocity_ini · 9.6`,
  `range = range_ini · 40`, `turn = (2π/16) / (turn_rate_ini+1) / 0.05`.

## Per-ship status

| Ship | Primary canon | Primary now | Special canon | Special now | Notes |
|------|---------------|-------------|---------------|-------------|-------|
| Earcr (Earthling) | HomingMissile from (0, +s/2), v=80, r=60, dmg=4, turn=3 | ✅ SpawnProjectiles homing | PointDefense range=5, dmg=1, 100 frames | ✅ GrantPointDefense | OK |
| Spael (Spathi) | Missile from (0, +s/2), v=96, r=17 | ✅ | BUTT homing-back missile, v=45, r=12, turn=1 | ✅ SpawnProjectiles back-homing | OK |
| Yehte (Yehat) | 2× Missile (±24, 14), v=80, r=12 | ✅ multi-barrel | Shield 500 frames, normal=0 | ✅ GrantShield | OK |
| Chmav (Chmmr) | ChmmrLaser from (0, 25), range 10, dmg=2 | ✅ SpawnBeams | Tractor (force=30 toward firer, range=100) | ✅ SpawnTractor | OK |
| Kzedr (Kzer-Za) | KzerZaMissile from (0, +s/2), v=80, r=22, dmg=6 | ✅ | Launch 1-2 KzerZaFighters (each costs 1 crew, has laser + return-home) | 🟡 SubEntity HomeAndDetonate (no laser, no return) | Fighters detonate on hit instead of orbiting + lasing |
| Mycpo (Mycon) | MyconPlasma homing, v=35, r=60, dmg=10, turn=1 | ✅ | Repair: heal 4 crew | ✅ ModifyCrew(+4) | OK. Minor: canon damage decays with distance, not implemented |
| Shosc (Shofixti) | Missile from (0, +s/2), v=96, r=14 | ✅ | Glory Device (3-press confirm in canon, instant blast in ours) | 🟡 SpawnDamageZone | Single-press triggers immediately; canon requires 3 presses |
| Arisk (Arilou) | Auto-aim Laser, r=5.5, dmg=1, weaponRange+200 search | ✅ SpawnBeams auto_aim | Random teleport ±1500 | ✅ TeleportRandom | OK. Now also `InertialessDrive` per shparisk.cpp:accelerate filter |
| Pkufu (Pkunk) | 3× AnimatedShot forward + ±π/2 lateral | ✅ multi-barrel | Refill battery on hold | ✅ RefillBattery | OK. Canon also has a respawn-on-death mechanic (Phaser anim) not implemented |
| Ilwav (Ilwrath) | AnimatedShot from (0, +s/2). If cloaked + target, intercept-aim | 🟡 SpawnProjectiles forward, no cloak-aim | Cloak (isInvisible toggle) | ✅ GrantInvisibility | Minor: cloaked auto-aim missing |
| Thrto (Thraddash) | Missile from (0, +s/2), v=120, r=25 | ✅ | Afterburner thrust + drop ThraddashFlame trail | ✅ Sequence(ApplyImpulse, SpawnDamageZone) | OK |
| Vuxin (VUX) | Laser from (size/11, +s/2.07), r=9, dmg=1 | ✅ SpawnBeams | VuxLimpet from (0, -s/2.8), v=25, slowdown=0.5 | ✅ SpawnProjectiles is_limpet | OK |
| Supbl (Supox) | Missile from (0, +s/4), v=120, r=15 | ✅ | While-held strafe: L/R = ⊥ thrust, T = back thrust, no rotation | 🔴 Not implemented (currently no-op `Todo`) | **Needs held-input-remap primitive** |
| Kohma (Kohr-Ah) | KohrAhBlade with MaxBlades=9, Persists=1; passive after release | 🟡 SpawnProjectiles single, no cap, no persist, no passive | F.R.I.E.D. — 16 outward KohrAhFRIED shots | ✅ SpawnProjectiles 16 barrels | **Major: no blade cap, no persistence after release, no passive slow-homing** |
| Syrpe (Syreen) | Missile from (0, +s/2 + 10), v=120, r=17, collide_sameship=ALL | ✅ | Siren song: damage human-crew enemies in range, spawn CrewPods at hit positions | 🟡 BrakeImpulse + 3× drift-pod (radial spawn from firer, not from enemy hit positions) | OK-ish |
| Andgu (Androsynth) | AndrosynthBubble v=24, r=50. Blocked while specialActive | ✅ (in Normal mode) | Blazer comet: SpeedMax=60, Mass=5, drains batt, damage on contact | ✅ ShipModes + collide_damage handler | OK |
| Chebr (Chenjesu) | ChenjesuShot + on-release shatter into 8 shards | ✅ `tick_chebr_crystal` ManagedExternally | DOGI sub-entity (1, max 4, homes + saps batt) | 🟡 SubEntity HomeAndDetonate (1 only, no max stack, no avoidance) | OK. Could cap at 4 simultaneously |
| Druma (Druuge) | DruugeMissile v=120, r=40, dmg=6 + recoil DriftVelocity=375 | ✅ recoil_impulse=375·9.6 | Crew-burn: -1 crew, +SpecialDrain batt | ✅ BurnCrewForBattery | OK |
| Utwju (Utwig) | 6× Missile at (±34,11), (±18,20), (±6,27), v=120, r=14 | ✅ 6-barrel | Fortitude — incoming damage → batt | ✅ GrantDamageToBattery (conversion=1.0) | OK |
| Zfpst (ZFP) | ZoqFotPikShot with random spread ±10·(2π/64) | ✅ | ZoqFotPikTongue: ship-attached, dist=39, 6-frame life | ✅ SpawnAttachedDamageZone | OK |
| Mmrxf (Mmrnmhrm) | T-form: 2 lasers ±asin(28/r). Y-form: 2 HomingMissiles ±25° | ✅ ShipModes T+Y | Toggle T↔Y form | ✅ ToggleMode | OK |
| Orzne (Orz) | OrzMissile from turret (angle + turretAngle), with recoil offset | 🟡 SpawnProjectiles forward only (turret_angle frozen at 0) | Spawn OrzMarine (-1 crew, attaches + drains target) | 🟡 ModifyCrew(-1) + SubEntity AttachAndDrain (one-shot drain, no joint-attach) | **Turret aim missing** (needs held-input-remap so L/R-while-special rotates turretAngle on the OverlaySprite) |
| Slypr (Slylandro) | SlylandroLaserNew: multi-segment lightning that snaps to nearest target | 🔴 `Todo` (no implementation) | Asteroid harvest (refills batt — only refuel method) | 🔴 `Todo` (no asteroids in arena) | **Needs segmented-beam primitive + asteroid bodies** |
| Umgdr (Umgah) | UmgahCone — ship-attached forward damage zone, no movement, damages while held | ✅ SpawnAttachedDamageZone (fires every frame while held) | Anti-grav slingshot: pos -= forward·2·size, vel=0 | ✅ TeleportRelative | OK |
| Meltr (Melnorme) | MelnormeShot with charge phases (×2 dmg per phase, +RangeUp range, max 3) | ✅ `tick_meltr_charge` ManagedExternally | Confusion ray (disables target controls for N frames) | 🔴 `Todo` | **Special needs InputOverride primitive** |

## Remaining canonical work, in priority order

1. **Supox held-strafe** + **Orz turret aim** — both need the same
   held-input-remap primitive (while special is held, intercept the
   ship's input and route it elsewhere). Single primitive unlocks both.

2. **Kohma blade cap + persistence** — blades survive after fire is
   released, behave as slow-homing passive shots. Up to `MaxBlades=9`
   active at once; pressing fire when full kills the oldest. Needs a
   `PersistentBladeCarrier` component similar to `CrystalCarrier`.

3. **Slypr segmented lightning** — multi-segment beam primitive that
   snaps to a target and renders as connected line segments. Variant
   of `Beam` with per-segment endpoints.

4. **Meltr confusion ray** — `InputOverride` primitive: insert a
   component on a target ship that intercepts `input::read_local_*`
   for some duration.

5. **Slypr asteroid harvest** — needs asteroid bodies in the arena
   (RigidBody::Kinematic, marker component) and a collision-based
   refuel handler when Slypr touches one.

6. **Glory Device 3-press confirmation** — small UX fix; track press
   count in a per-Shosc-ship component.

7. **Kzer-Za fighter laser + return-home** — extend `SubEntityAi`
   with a `HomeAndAttackThenReturn` variant that periodically fires
   a beam at the target and returns home after a timeout.

8. **Pkunk reborn-from-the-ashes** — handle_damage hook that, when
   a Pkunk crew hits 0, with 50% chance respawns the ship at a
   random position with full stats instead of destroying it.

9. **Ilwrath cloaked intercept-aim** — when firing while `Invisible`
   and a target is in range, snap the shot direction to lead the
   target's velocity.

10. **Mycon plasmoid damage decay** — damage_factor scales down with
    `d / range` distance traveled.

## Engine systems that do *not* go through Avian's collision pipeline

Currently the following systems use **custom spatial queries**
(distance / ray / circle checks against ship positions) rather than
emitting Avian `CollisionStart` events:

| System | Where | What it does | Avian-native plan |
|--------|-------|--------------|-------------------|
| `tick_beams` | ship.rs | Ray-casts from a beam's owner each tick, picks the nearest enemy along the ray within `range + width` | Spawn a thin rectangle sensor collider as part of the beam entity; resize/reposition per tick; handle `CollisionStart` events to apply damage |
| `tick_damage_zones` | ship.rs | Distance check from a static zone to every ship each tick | Sensor circle collider on the zone entity; collision events apply per-tick damage |
| `tick_attached_damage_zones` | ship.rs | Same as above but the zone follows an owner | Sensor circle + per-tick position update |
| `tick_tractors` | ship.rs | Distance check to find nearest enemy in range, applies velocity nudge | Sensor circle collider, push impulses on the targets it overlaps |
| `tick_point_defense` | ship.rs | Distance check to despawn projectiles + damage ships within radius | Sensor circle, react to projectile + ship overlap events |
| `tick_sub_entities` (target acquisition) | ship.rs | Each homing sub-entity scans all ships each tick to pick the nearest non-friendly target | The hit detection IS already Avian-driven (collisions go through `handle_sub_entity_collisions`); only target *acquisition* uses spatial queries, which is unavoidable without a query API on Avian colliders |
| `steer_homing_projectiles` | ship.rs | Same as above for projectiles with `Homing` | Same — hit detection is Avian, target acquisition is a scan |

### Migration plan

Avian 0.6 supports **`Sensor`** colliders (no impulse imparted on
contact) and **collision events** are emitted for sensor overlaps.
The work to convert each of the above to physics-native:

1. Add `Sensor` + `Collider::circle(r)` or `Collider::rectangle(w, h)`
   to the entity at spawn.
2. Add `CollisionEventsEnabled`.
3. Replace the per-tick spatial scan with a `MessageReader<CollisionStart>`
   handler that filters for sensor entities and applies the effect.
4. For *attached* zones (and the beam, which moves with its owner),
   keep the per-tick `Transform`/`Position` update so the sensor
   follows the owner.
5. For *per-tick damage* (zones, beams), use `CollisionStart` +
   `CollisionEnd` to track "currently inside" set, and tick damage
   only on entities in the set.

The two scanning operations that *aren't* migratable (target
acquisition for homing/sub-entities) are documented as expected.
Avian doesn't expose a generic "what's the nearest collider matching
filter X" query that would replace them efficiently. They could be
moved to a quadtree later, but it's not a physics-engine concern.

## Documented exceptions to "everything through the physics engine"

- **Mode swap stat replacement** (`tick_ship_modes`) — replacing
  `Mass`, `ShipPhysicsDerived`, `ShipFrames`, `ShipAbilities` on a
  mode toggle is a non-physics operation (component replacement).
  Avian then picks up the new `Mass` automatically for collision
  response.

- **Per-tick velocity overwrite for `InertialessDrive`** (Arilou) —
  intentionally bypasses physics-driven momentum integration so the
  ship has zero inertia. Avian still applies the resulting velocity
  during the step; we just keep overwriting it before each step.

- **Edge-detection input state** (`LastTurnInput`, `last_fire_held`
  on carriers) — input handling, not physics.

- **Boundary tracing for collider polygon extraction**
  (`compute_polygon` in collider.rs) — sprite alpha → polygon
  contour. One-time, at asset load. The *resulting* polygon
  IS fed to Avian as the actual collider.

Anything that *imparts force or damage on contact between two
physical bodies* belongs in Avian's collision pipeline. The
`tick_beams` / `tick_damage_zones` / `tick_tractors` / `tick_point_defense`
list above is the gap to close.
