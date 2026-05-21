# Extensibility recipes

Where each kind of new content / mechanic plugs in. Cross-link from this doc
when you find yourself wondering "if I want to add X, where does it go?"

> **Direction-of-travel note.** The two big architectural shifts in
> flight — composable abilities (no more `match ShipClass` arms) and
> composite ships (one ship ≠ one entity) — are written up at the
> *bottom* of this file. The "one match arm at a time" recipe below
> still describes the codebase as it stands today; expect that recipe
> to evaporate once the data-driven ability layer lands.

## Add a new ship class (vanilla SC2 behaviour)

Say you want to add the Slylandro Probe (`slypr`). It picks up a new
`ShipClass` variant; everything else is one match arm at a time.

1. **Drop the assets in place.** Run the extractor once with the legacy
   `.dat` copied next to the other ones:

   ```sh
   cp /path/to/shpslypr.dat assets/legacy-dat/slypr.dat
   cp /path/to/shpslypr.ini assets/ships/slypr.ini
   cp /path/to/shpslypr.txt assets/ships/slypr.txt
   cargo run --bin extract_dat --features tools
   ```

   That produces `assets/ships/slypr/{sprites,sounds,manifest.json}`.

2. **Add the enum variant.** `src/ship.rs`:

   ```rust
   pub enum ShipClass { ..., Slypr }
   impl ShipClass {
       pub fn code(self) -> &'static str {
           match self { ..., ShipClass::Slypr => "slypr" }
       }
   }
   pub const ALL_CLASSES: [ShipClass; 7] = [..., ShipClass::Slypr];
   ```

3. **Add the per-class tuning arms.** Three places, each a single arm:
   - `physics_spec` — collider radius, mass multiplier, damping
   - `primary_weapon` — direction, speed, lifetime, sprite color
   - `trigger_specials` — special ability behaviour

4. **Done.** No new system, no plugin registration. The class is now
   pickable via the F-key plus 7 (you'll want to bump the hotkey table
   if you go past 6).

## Add a wild new mechanic (not in SC2)

Anything that doesn't fit the per-class match arms gets its own system.
The pattern:

1. Define the state as a Component (or Resource, for global mechanics)
2. The triggering ship's `trigger_specials` arm spawns the entity / sets
   the resource
3. A new system in `ShipPlugin::build` ticks the mechanic each frame

### Example: time bubble (per-region slow zone)

```rust
#[derive(Component)]
pub struct TimeBubble {
    pub radius: f32,
    pub scale: f32,
    pub lifetime: f32,
}

// In trigger_specials for the relevant class:
commands.spawn((
    TimeBubble { radius: 200.0, scale: 0.3, lifetime: 4.0 },
    Transform::from_translation(pos.0.extend(0.1)),
    Position(pos.0),
));

// New system in FixedUpdate:
fn apply_time_bubble(
    fields: Query<(&Position, &TimeBubble)>,
    mut affected: Query<(&Position, &mut LinearVelocity), Without<TimeBubble>>,
) {
    for (fp, bubble) in &fields {
        for (bp, mut vel) in &mut affected {
            if (fp.0 - bp.0).length() < bubble.radius {
                vel.0 *= bubble.scale; // crude but adequate
            }
        }
    }
}
```

See `src/timeflow.rs` for the longer prose on doing this *properly* and
the three other extensibility hooks waiting in that module.

### Example: tractor beam (planar linkage)

Avian's joints are first-class. A tractor beam is just a `DistanceJoint`
that you spawn between the firing ship and the target, then despawn
when the special expires or the target is out of range.

```rust
// In trigger_specials:
let joint = commands.spawn(DistanceJoint::new(this_ship, target)
    .with_rest_length(100.0)
    .with_compliance(0.05)
).id();
// Insert a lifetime component, tick it down in another system,
// despawn the joint when it expires.
```

The chained motion ("the Chmmr drags the Spathi closer") is fully
emergent from the physics solver.

### Example: time rewind (the most ambitious one)

Don't try to run the physics solver with negative dt — it diverges.
Instead exploit the rollback netcode (M4) state-snapshot ring buffer:
every frame the snapshot system records world state for the last N
seconds anyway. The rewind ability becomes a UI on top of that:
suspend the live solver, pop snapshots backwards out of the buffer,
apply them. See `src/timeflow.rs` for the longer note.

## Add a new input action

`src/input.rs`. One new `INPUT_FOO` flag bit (next power of two), one
new keymap entry per slot in `read_local_input`. Existing systems that
care about it call `input.pressed(INPUT_FOO)`. Stays as a single byte
on the wire so GGRS doesn't notice the change.

## Add a new HUD element

`src/hud.rs`. Define a marker component, spawn the node in `setup_hud`
with that marker, write a system that updates the text/colour by
querying ships / resources.

## Add a per-ship persistent stat (e.g. battery)

Two pieces:

1. Component on the ship: `#[derive(Component)] pub struct Battery { ... }`
2. Add to the `spawn_ship` `gameplay` bundle. Reset on rematch happens
   automatically — `teardown_match` despawns and `spawn_ship` rebuilds.

## Where things live

| File | Owns |
| ---- | ---- |
| `src/main.rs` | Bevy app, AppState machine, top-level wiring |
| `src/ship.rs` | All ship-related ECS — class enum, dispatch tables, gameplay systems |
| `src/physics.rs` | Avian config: zero gravity, fixed tick, arena wrap |
| `src/input.rs` | Input flag bits + `read_local_input` keymap |
| `src/hud.rs` | UI nodes, crew readouts, match-phase banner, score |
| `src/timeflow.rs` | Time scaling — current global control + extensibility hooks |
| `src/netplay.rs` | GGRS / matchbox scaffolding (M4) |
| `tools/extract_dat.rs` | Offline asset extractor (Allegro 4 → PNG/WAV) |
| `assets/ships/*.ini` | Static stat sheets (legacy TW format) |
| `assets/ships/<code>/` | Extracted sprites + sounds + manifest |

---

# Architectural directions

The two sections below describe shifts that aren't in code yet but
will shape every PR going forward. Read them before adding anything
non-trivial — they exist precisely so we stop entrenching the patterns
they replace.

> Both shifts are about *how the engine expresses behaviour*. Neither
> changes the prime directive: ship gameplay still comes from the
> legacy TW-Light `shp*.cpp` + `.ini` data. We're refactoring the
> port, not redesigning the game.

## 1. Composable abilities — kill the per-class match arms

### Why

Today every new ship lights up arms in `fire_weapons`, `trigger_specials`,
and `physics_spec`. That scales linearly with ships (already 25 arms each)
and — worse — *quadratically* with mechanics: every time we add a new
behaviour like "homing", we have to revisit every ship that should be
able to opt into it. This is the smell behind every "TODO: needs new
primitive" comment in `src/ship.rs`. If sustaining the port means
overhauling the engine for every Orz / Mmrnmhrm / Chmmr, we lose.

The fix is to flip the dependency. **Components ARE the vocabulary.**
A ship doesn't *have a class that triggers code*; a ship has a
*manifest of abilities*, each ability is a small data structure, and a
single generic dispatcher reads that manifest and spawns ECS entities
with the right components. Adding a new ship is a `.ron` file. Adding
a new mechanic is one component + one system, and *every existing
ship can immediately opt into it* without re-touching its code.

### The vocabulary we already have (primitives in `src/ship.rs`)

These are orthogonal — any combination can apply to any spawned
projectile or buff.

| Component             | What it does                                             | Read by                         |
| --------------------- | -------------------------------------------------------- | ------------------------------- |
| `Projectile`          | Owner + damage + lifetime; the base "this is a shot"     | `tick_projectile_lifetime`, `handle_projectile_hits` |
| `Homing`              | Steers toward nearest enemy at `turn_rate` rad/s         | `steer_homing_projectiles`      |
| `Limpet`              | On-hit transfer mass to target (no crew damage)          | `handle_projectile_hits`        |
| `DamageZone`          | Stationary AoE that damages anything inside              | `tick_damage_zones`             |
| `ShieldActive`        | Multiplier on incoming damage for a duration             | `handle_projectile_hits`        |
| `PointDefenseActive`  | Periodic radius scan that nukes hostile projectiles      | `tick_point_defense`            |
| Avian joints (`DistanceJoint`, `RevoluteJoint`, `FixedJoint`, `PrismaticJoint`) | Constraints between rigid bodies | Avian solver |
| Avian `RigidBody`, `LinearVelocity`, `Mass`, `Collider` | The physics state itself      | Avian solver |

That covers ~70% of the SC2 roster. The rest of the TODOs in `ship.rs`
either need one of these wired more flexibly, or one new generic
primitive — listed below in "primitives we still need".

### Proposed shape: `AbilitySpec` + a generic dispatcher

Roughly (sketch, not final):

```rust
// One per ship, loaded from data (RON sibling of the .ini).
struct ShipAbilities {
    primary: AbilitySpec,
    special: AbilitySpec,
}

// Each ability is a verb (what to do when the button is pressed) plus
// zero-or-more effect descriptors (the components to attach to whatever
// it spawns).
enum AbilitySpec {
    SpawnProjectiles { volleys: Vec<Volley>, effects: Vec<EffectSpec> },
    ApplyImpulse { dir: ImpulseDir, magnitude: f32 },
    Teleport { kind: TeleportKind },
    GrantBuff { component: BuffSpec, duration_s: f32 },
    ModifySelf { delta: SelfModSpec }, // heal crew, refill batt, burn crew
    SpawnDamageZone { radius: f32, dps: f32, duration_s: f32, anchor: Anchor },
    SpawnJointedPart { part: PartSpec, joint: JointSpec },
    // Composite — the canonical Druuge "recoil + projectile" is just a Sequence.
    Sequence(Vec<AbilitySpec>),
    // Escape hatch for things we haven't generalised yet. Logs a TODO
    // and degrades to a placeholder so the ship still works.
    Custom { ident: String },
}

struct Volley {
    barrels: Vec<Barrel>,             // already exists
    random_spread_rad: f32,
    speed: f32,                       // both in canonical scaled units
    lifetime: f32,
    sprite_path: Option<String>,
}

struct EffectSpec {
    homing_turn_rate: Option<f32>,
    is_limpet: bool,
    recoil_impulse: f32,
    on_hit_damage_zone: Option<ZoneSpec>,
    // grow as needed — each variant maps to a component attach
}
```

The dispatcher is one `fn apply_ability(cmd, world_pose, &spec)` that
folds `AbilitySpec` into entity spawns. The whole of
`fire_weapons`/`trigger_specials` collapses to "look up `ShipAbilities`
for this class, dispatch the right verb". No per-class arms.

Storage of the manifest is open — likely a sibling `assets/ships/<code>.ron`
parsed alongside the existing `.ini` (the legacy `.ini` format has no
slot for behaviour). The `.ini` stays the source of truth for *numbers*
(crew, batt, mass, drains) so we don't fork the canonical stat sheet.

### Primitives we still need (and which ships unlock them)

Each becomes one new component + one new system. Once it exists,
every ship that wants the behaviour just lists it in their `.ron`.

- **`Beam`** — sustained line damage, owner-attached, ticks while a key is held.
  Unlocks: Chmmr laser, Arilou auto-aim laser, VUX laser.
- **`AttachedDamageZone`** — `DamageZone` that follows its owner each tick.
  Unlocks: Umgah cone, ZFP tongue (today is a stationary placeholder).
- **`SubEntity`** — small autonomous body with its own AI script (`steer_toward_target`,
  `return_to_parent`, `detonate_on_proximity`), parented to a spawner.
  Unlocks: Kzer-Za fighters, Chenjesu DOGI, Orz marines, Syreen crew pods.
- **`ChargedFire`** — primary holds to charge, releases to fire with scaled stats.
  Unlocks: Melnorme plasma, Chenjesu crystal-shatter on release.
- **`ModeToggle`** — swap a bundle of stats (speed/accel/turn/sprite/abilities) on press.
  Unlocks: Mmrnmhrm T↔Y form, Androsynth Blazer mode.
- **`InvisibleTo`** — drops targeting locks; homing missiles re-query targets.
  Unlocks: Ilwrath cloak, Arilou post-teleport telefrag immunity.
- **`InputOverride`** — temporarily replace another ship's input mask (confusion).
  Unlocks: Melnorme confusion ray.
- **`AppliedForce`** — apply an impulse to another entity (not self).
  Unlocks: Chmmr tractor, Druuge missile shove.
- **`StatSwapTimed`** — temporarily replace `Mass` / `LinearDamping` / etc., revert on expiry.
  Unlocks: Androsynth Blazer (mass swap + damage trigger), Utwig fortitude.

Each of those is a 50-100-line addition in isolation. None of them
should ever reference a specific `ShipClass` — that's how we make sure
the next ship after the 25 stock SC2 ones doesn't trigger another
engine rewrite.

### Migration plan (zero big-bang)

1. Add `AbilitySpec` + the generic dispatcher alongside the existing
   match arms. Both run; the dispatcher is a no-op for any ship
   without a `.ron` yet.
2. Convert one ship to data (Earthling first — already canonical).
   Delete its match arms when the `.ron` works.
3. Convert the easy ships next (anything whose abilities already
   decompose into existing primitives).
4. Each TODO primitive lands as a generic component+system in
   isolation. As it lands, its ships convert from match arms to data.
5. When the last ship is converted, delete `primary_weapon` /
   `trigger_specials` match arms entirely. `ShipClass` becomes a thin
   identifier instead of a behaviour selector.

The match-arm code in `src/ship.rs` becomes the *test suite* for the
data-driven layer — if a ship's `.ron`-driven behaviour ever drifts
from its old match-arm behaviour we'll see it immediately, because
both can be diffed.

---

## 2. Composite ships — one ship ≠ one entity

### Why

Real SC2 ships already aren't single bodies (Chmmr satellites orbit
the hub, Ur-Quan fighters detach from the dreadnought, the Yehat
shield is a separate sprite, Andro Blazer is a literal hull change).
Beyond canon, the user explicitly wants linkages, harpoons, levers,
and "accessories that float around and you add them to your arsenal".

Today every `Query<(&Ship, ...)>` in the codebase implicitly assumes
*one ship = one rigid body*. Embed that assumption deeper and we'll
never get composite ships without rewriting half the engine.

### Proposed model

A ship is a **graph of jointed parts**, with one designated root.

```rust
/// Marks the root rigid body of a ship. The Avian joint graph from
/// here defines the rest of the ship.
#[derive(Component)]
pub struct Hull {
    pub player_slot: usize,
    pub class: ShipClass,
    pub parts: Vec<Entity>, // every part of this ship, root included
}

/// Marks a non-root rigid body that belongs to a hull. Carries a
/// back-pointer to the root so destruction / scoring / HUD lookups
/// don't have to walk the joint graph every tick.
#[derive(Component)]
pub struct Part {
    pub hull: Entity,
    pub kind: PartKind,
}

enum PartKind {
    Core,            // the hull's main body — losing this destroys the ship
    Turret,          // independent aim (Orz)
    Satellite,       // orbiter (Chmmr ZapSats)
    Fighter,         // detachable sub-entity (Kzer-Za fighter, Orz marine)
    Accessory,       // bolt-on the player added (the user's arsenal idea)
    Harpoon,         // a runtime-spawned tether part
}
```

Each `Part` is a *real* Avian rigid body with its own collider, mass,
sprite, and joint(s) back to the hull. The "ship's mass" is the sum
of part masses, propagated automatically by the solver. The "ship's
crew" is currently a single number on the root — promoting that to
"each part has hit points, losing the core kills the ship" is a
follow-up; we don't have to do it day one.

### What this unlocks

- **Chmmr satellites**: spawn 3 satellites at construction time,
  each a `Part { kind: Satellite }`, joined by `RevoluteJoint` to a
  fixed anchor at the hull's position with the joint's local angle
  driven to spin the satellite in formation. No special "orbiter"
  primitive — just joints.
- **Harpoons**: on hit, spawn a `Part { kind: Harpoon }` welded
  (`FixedJoint`) to the *target* and tethered (`DistanceJoint` with
  compliance) to the firing hull. Solver pulls the target in. Cut the
  joint to release. This is the Chmmr beam, Syreen abduction *and*
  the user's "harpoon" all unified.
- **Detachable fighters**: a `Part { kind: Fighter }` joined to the
  carrier by a breakable `FixedJoint`. "Launch" = break the joint and
  hand the part a `SubEntity` AI component (see ability primitives).
  When the fighter dies, the hull lost a part but isn't destroyed.
- **User's accessory idea**: each accessory is a small Bevy asset
  bundle (`PartSpec` in the ability layer) listing its sprite,
  collider, mass, and joint type back to the hull. "Adding to your
  arsenal" = the loadout screen writes a list of `PartSpec`s into the
  hull manifest; `spawn_ship` instantiates each one as a `Part` joined
  to the root. Player customisation falls out for free.
- **Linkages and levers**: just joints. A two-segment lever arm is a
  `Part` joined to the hull by `RevoluteJoint`, optionally with a
  motor target angle driven by input. Star Control: Megalomaniac
  Mode.

### Migration plan (also zero big-bang)

1. Introduce `Hull` and `Part` components alongside the existing
   `Ship` component, with `Ship` deprecated to "the marker on the
   root part". Every system that does `Query<&Ship>` keeps working.
2. New systems that care about composite behaviour query
   `Query<(&Hull, ...)>` and walk the `parts` vec when they need to
   touch the whole ship.
3. Convert the first composite ship — Yehat shield as a separate
   sprite child part (small scope, no AI). Then Chmmr satellites,
   then a true harpoon mechanic.
4. Eventually the `Ship` component is renamed to `Hull` and the
   one-entity-per-ship implicit assumption is gone.

### Invariant for anything written before then

Don't bake "this ship is exactly one entity" into any new system. If
a system needs to iterate over "all parts of the firing ship", write
it that way today even if `Hull::parts` is currently always `vec![self]`
— that turns the future migration into a `vec.push()` instead of a
refactor.

---

## Cross-cutting: composition over special cases

If you find yourself writing `match ShipClass::Foo => special_behaviour()`,
or `if entity == ship_root { ... handle specially ... }`, stop and ask:

- Could this be a Component the ship's data manifest opts into?
- Could this be a Part attached at spawn?
- Could this be a joint the physics solver already understands?

The answer is "yes" more often than the existing match-heavy code
suggests. The two refactors above are how we make "yes" the easy
default.
