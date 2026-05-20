//! Offline asset extractor: salvages sprites + sounds from the legacy
//! Allegro 4 `.dat` archives (`assets/legacy-dat/*.dat`) into per-ship
//! directories under `assets/ships/<code>/`.
//!
//! Run with:
//! ```sh
//! cargo run --bin extract_dat --features tools
//! ```
//!
//! Pipeline:
//! 1. Shell out to the Allegro `dat` CLI (`apt install liballegro4-dev`)
//!    to dump every object as `.bmp` / `.wav` into a temp staging dir.
//! 2. Convert each BMP to PNG, replacing the Allegro magenta key
//!    (`#FF00FF`) with full-alpha transparency so sprite masking works
//!    in Bevy without a custom shader.
//! 3. Copy `.wav` samples through unchanged (they're already PCM RIFF).
//! 4. Emit `manifest.json` listing rotation frames, projectile frames,
//!    and sounds so the runtime can load deterministically without a
//!    filesystem scan.

use image::{ImageBuffer, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAGENTA: [u8; 3] = [0xFF, 0x00, 0xFF];

fn main() {
    let legacy_dir = Path::new("assets/legacy-dat");
    let out_root = Path::new("assets/ships");

    let dats: Vec<PathBuf> = fs::read_dir(legacy_dir)
        .expect("read assets/legacy-dat")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension() == Some(OsStr::new("dat")))
        .collect();

    if dats.is_empty() {
        eprintln!("no .dat files in {legacy_dir:?}");
        std::process::exit(1);
    }

    for dat in &dats {
        let code = dat
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("dat stem");
        match extract_ship(dat, &out_root.join(code)) {
            Ok(stats) => println!(
                "{code}: {} sprites, {} sounds",
                stats.sprite_count, stats.sound_count
            ),
            Err(e) => eprintln!("{code}: FAILED — {e}"),
        }
    }
}

struct ExtractStats {
    sprite_count: usize,
    sound_count: usize,
}

fn extract_ship(dat_path: &Path, out_dir: &Path) -> Result<ExtractStats, String> {
    let staging = Path::new("target/extract").join(
        dat_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("bad stem")?,
    );
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("mkdir staging: {e}"))?;

    // Step 1: invoke Allegro `dat` to dump every object.
    // It writes into the current directory, so we run it there.
    let abs = fs::canonicalize(dat_path).map_err(|e| format!("canonicalize: {e}"))?;
    let status = Command::new("dat")
        .arg("-x")
        .arg(&abs)
        .arg("*")
        .current_dir(&staging)
        .status()
        .map_err(|e| format!("spawn dat: {e} — is liballegro4-dev installed?"))?;
    if !status.success() {
        return Err(format!("dat exited with {status}"));
    }

    // Step 2/3: route extracted files into sprites/ and sounds/ under out_dir.
    let sprites_dir = out_dir.join("sprites");
    let sounds_dir = out_dir.join("sounds");
    fs::create_dir_all(&sprites_dir).map_err(|e| format!("mkdir sprites: {e}"))?;
    fs::create_dir_all(&sounds_dir).map_err(|e| format!("mkdir sounds: {e}"))?;

    let mut sprites: BTreeMap<String, Value> = BTreeMap::new();
    let mut sounds: Vec<String> = Vec::new();
    let mut sprite_count = 0;
    let mut sound_count = 0;

    for entry in fs::read_dir(&staging).map_err(|e| format!("read staging: {e}"))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path = entry.path();
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        match path.extension().and_then(|s| s.to_str()) {
            Some("bmp") => {
                let png_name = sanitize_name(&stem) + ".png";
                let dest = sprites_dir.join(&png_name);
                convert_bmp_to_png(&path, &dest)?;
                let (w, h) = png_dimensions(&dest)?;
                sprites.insert(
                    sanitize_name(&stem),
                    json!({ "file": png_name, "w": w, "h": h }),
                );
                sprite_count += 1;
            }
            Some("wav") => {
                let wav_name = sanitize_name(&stem) + ".wav";
                fs::copy(&path, sounds_dir.join(&wav_name))
                    .map_err(|e| format!("copy {stem}: {e}"))?;
                sounds.push(sanitize_name(&stem));
                sound_count += 1;
            }
            _ => {
                // SHIP_DAT and other binary blobs — ignore for now.
            }
        }
    }

    sounds.sort();
    let manifest = json!({
        "sprites": sprites,
        "sounds": sounds,
    });
    fs::write(
        out_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| format!("write manifest: {e}"))?;

    Ok(ExtractStats {
        sprite_count,
        sound_count,
    })
}

/// `SHIP_S01_PCX` → `ship_s01`, dropping the legacy `_PCX` suffix and
/// lowercasing for filesystem-friendly names.
fn sanitize_name(raw: &str) -> String {
    let trimmed = raw.strip_suffix("_PCX").unwrap_or(raw);
    let trimmed = trimmed.strip_suffix("_WAV").unwrap_or(trimmed);
    trimmed.to_ascii_lowercase()
}

fn convert_bmp_to_png(bmp: &Path, png: &Path) -> Result<(), String> {
    let img = image::open(bmp).map_err(|e| format!("decode {bmp:?}: {e}"))?;
    let rgba = img.to_rgba8();
    let masked = mask_magenta(&rgba);
    masked
        .save(png)
        .map_err(|e| format!("write {png:?}: {e}"))?;
    Ok(())
}

/// Allegro 4 truecolor sprites use bright magenta as the transparent
/// color. Replace it with `(0,0,0,0)` so the PNG carries real alpha.
fn mask_magenta(src: &RgbaImage) -> RgbaImage {
    let (w, h) = src.dimensions();
    let mut out = ImageBuffer::new(w, h);
    for (x, y, p) in src.enumerate_pixels() {
        let [r, g, b, _] = p.0;
        let pixel = if [r, g, b] == MAGENTA {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([r, g, b, 0xFF])
        };
        out.put_pixel(x, y, pixel);
    }
    out
}

fn png_dimensions(p: &Path) -> Result<(u32, u32), String> {
    let img = image::image_dimensions(p).map_err(|e| format!("dims {p:?}: {e}"))?;
    Ok(img)
}
