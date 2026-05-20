# Extensibility recipes

Where each kind of new content / mechanic plugs in. Cross-link from this doc
when you find yourself wondering "if I want to add X, where does it go?"

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
