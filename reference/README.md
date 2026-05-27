# Reference: TW-Light (Timewarp) original source

This is the **original C++ source** of TW-Light 0.5 (the Star Control:
Timewarp fan game this project is a Rust/Bevy port of), vendored here so
it's available in every dev session. The container's `/tmp` is
ephemeral and the repo is the only durable store, so the canonical
gameplay reference lives in-repo.

## What's here

- `src/` — the TW-Light game source, **minus** `libraries/` (the bundled
  Allegro 4 game library, which we don't port). The parts that matter
  for replicating ship behaviour:
  - `src/ships/shp*.cpp` — per-ship weapons/specials (e.g.
    `shpchebr.cpp` = Chenjesu, `shpkohma.cpp` = Kohr-Ah). The Rust code
    cites these in comments (e.g. `// shpchebr.cpp:calculate`).
  - `src/melee/` — melee combat framework, ship-data loading
    (`mshpdata.cpp`), physics helpers (`mhelpers.cpp`).
  - `src/other/`, `src/util/`, `src/games/`, `src/ais/` — supporting
    engine code.
- `COPYING`, `AUTHORS`, `THANKS`, `README` — upstream licensing /
  attribution.

## What's NOT here

- `src/libraries/` (bundled Allegro) — not needed; we use Bevy/Avian.
- The `data/` datafiles (~141 MB). The specific `.dat` files we extract
  sprites from already live in `assets/legacy-dat/`.

## License

TW-Light is **GPL** (see `COPYING`) — it descends from the Ur-Quan
Masters / Star Control 2 release. This port is a derivative work; keep
that in mind for distribution. This directory is reference material, not
compiled into the Rust build.

## How to use it when porting a ship

1. Open `src/ships/shp<code>.cpp` for the ship (codes match our
   `ShipClass` / `.ini`, e.g. `thrto`, `kohma`, `chebr`).
2. Read `activate_weapon` / `activate_special` and any custom
   projectile/animation classes for the exact behaviour + which
   `spriteWeapon` / `spriteSpecial` it draws.
3. Mirror it in `src/ship.rs` / `src/ability.rs`, citing the `.cpp` in a
   comment as the existing code does.
