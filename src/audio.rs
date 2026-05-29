//! Music + global SFX wiring. All clips originate from the upstream
//! TW-Light data tarball (`tw-light-0.5.tar.gz`, SourceForge); see the
//! per-asset comments below for the canon source.
//!
//! Per-ship weapon / special sounds live in `assets/ships/<code>/sounds/`
//! and are played directly from `src/ability.rs` — this module owns
//! everything else: title music, combat music, the per-faction victory
//! ditty, and the canon ship-death "BOOMSHIP" sample.

use bevy::prelude::*;

use crate::hud::{MatchOutcome, MatchPhase};
use crate::ship::{Crew, Ship};
use crate::AppState;

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MusicTrack>()
            // Stop the currently-playing music track whenever we leave
            // a state with one, so the title theme doesn't bleed into
            // a match and vice versa.
            .add_systems(OnExit(AppState::MainMenu), stop_music)
            .add_systems(OnExit(AppState::InMatch), stop_music)
            .add_systems(OnEnter(AppState::MainMenu), start_title_music)
            // Sequence after `ship::spawn_match` so the ship query
            // sees the freshly-spawned opponent and we can key combat
            // music off their race instead of falling back to MELEEMUS.
            .add_systems(
                OnEnter(AppState::InMatch),
                start_combat_music.after(crate::ship::spawn_match),
            )
            .add_systems(
                Update,
                (play_ship_death_boom, play_victory_ditty).run_if(in_state(AppState::InMatch)),
            );
    }
}

/// Handle to whatever music entity is currently playing (title theme,
/// combat loop). Stored so we can despawn it cleanly on state exit
/// without searching the world.
#[derive(Resource, Default)]
struct MusicTrack(Option<Entity>);

fn stop_music(mut commands: Commands, mut track: ResMut<MusicTrack>) {
    if let Some(e) = track.0.take() {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
    }
}

/// Canon `scpgui.dat:TITLEMUSIC_WAV` — 60 s stereo loop the original
/// plays under the title screen (`scp.cpp:490 sound.play_music`).
fn start_title_music(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut track: ResMut<MusicTrack>,
) {
    let e = commands
        .spawn((
            AudioPlayer::<AudioSource>(assets.load("music/title.mp3")),
            PlaybackSettings::LOOP,
        ))
        .id();
    track.0 = Some(e);
}

/// Canon Star Control 2 race-theme music as combat loop. Each race
/// has its own tracker module shipped in `timewarp_2006_06_05.zip`'s
/// `gamedata/dialogs/<race>.mod` — the canon SC2 battle themes. We
/// pick the *opponent's* theme (slot 1 in 1v1) the way the original
/// game played the enemy's faction music. Ships whose race has no
/// theme in the upstream data (Chenjesu, Androsynth, Mmrnmhrm, Alarian
/// — TimeWarp's `gamedata/dialogs/` doesn't ship a clip for them) fall
/// back to the original TW-Light `melee.dat:MELEEMUS_MOD` combat loop.
///
/// `start_combat_music` is sequenced via `add_systems(OnEnter, ...)`
/// AFTER `ship::spawn_match`, so the ship query already has the
/// opponent's class component when this runs.
fn start_combat_music(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut track: ResMut<MusicTrack>,
    ships: Query<&Ship>,
) {
    // Slot 1 is the canonical "opponent" in our 1v1 / vs-AI flow.
    // Falls back to ANY ship if slot 1 hasn't spawned (multi-slot
    // free-for-all). Fallback fallback is MELEEMUS.
    let theme = ships
        .iter()
        .find(|s| s.player_slot == 1)
        .or_else(|| ships.iter().next())
        .and_then(|s| race_theme_stem(s.stats.code.as_str()));

    let path = match theme {
        Some(stem) => format!("music/race/{}.ogg", stem),
        None => "music/melee.mp3".to_string(),
    };
    let e = commands
        .spawn((
            AudioPlayer::<AudioSource>(assets.load(path)),
            PlaybackSettings::LOOP,
        ))
        .id();
    track.0 = Some(e);
}

/// Maps a ship code to its faction's race-theme stem under
/// `assets/music/race/<stem>.ogg`. The original tracker modules are in
/// `timewarp_2006_06_05/gamedata/dialogs/<stem>.mod`. Returns `None`
/// for ships whose race has no upstream theme (defaults the caller to
/// MELEEMUS). Both Ur-Quan factions share "urquan" in canon.
fn race_theme_stem(code: &str) -> Option<&'static str> {
    Some(match code {
        "earcr" => "human",
        "spael" => "spathi",
        "yehte" => "yehat",
        "chmav" => "chmmr",
        "kzedr" => "urquan",
        "mycpo" => "mycon",
        "shosc" => "shofixty",
        "arisk" => "arilou",
        "pkufu" => "pkunk",
        "ilwav" => "ilwrath",
        "thrto" => "thraddash",
        "vuxin" => "vux",
        "supbl" => "supox",
        "kohma" => "korah",
        "syrpe" => "syreen",
        "druma" => "druuge",
        "utwju" => "utwig",
        "zfpst" => "zoqfot",
        "orzne" => "orz",
        "slypr" => "sylandro",
        "umgdr" => "umgah",
        "meltr" => "melnorme",
        // Chenjesu / Androsynth / Mmrnmhrm / Alarian don't have a
        // dedicated theme in TimeWarp's `gamedata/dialogs/`. The
        // caller falls back to MELEEMUS for these.
        _ => return None,
    })
}

/// Canon `melee.dat:BOOMSHIP_WAV` (2.1 s, 22 kHz mono) — played from
/// `mship.cpp:514` when a ship is destroyed. Fires once per ship that
/// drops to zero crew this tick.
fn play_ship_death_boom(
    mut commands: Commands,
    assets: Res<AssetServer>,
    q: Query<&Crew, (With<Ship>, Changed<Crew>)>,
) {
    for crew in &q {
        if crew.current <= 0 {
            commands.spawn((
                AudioPlayer::<AudioSource>(assets.load("sfx/boom_ship.wav")),
                PlaybackSettings::DESPAWN,
            ));
        }
    }
}

/// Per-faction post-match jingle (`victoryditty.dat:<RACE>DITTY_WAV`).
/// Triggered the tick the match transitions to `PostMatch` (one-shot,
/// guarded by `Changed<MatchPhase>`).
fn play_victory_ditty(
    mut commands: Commands,
    assets: Res<AssetServer>,
    phase: Res<MatchPhase>,
    outcome: Res<MatchOutcome>,
    ships: Query<&Ship>,
) {
    if !phase.is_changed() || *phase != MatchPhase::PostMatch {
        return;
    }
    let Some(winner_slot) = outcome.winner else {
        return;
    };
    // Find the surviving winner's ship to key the ditty off its code.
    let Some(code) = ships
        .iter()
        .find(|s| s.player_slot == winner_slot)
        .map(|s| s.stats.code.as_str())
    else {
        return;
    };
    let path = format!("music/victory/{}.mp3", victory_ditty_stem(code));
    commands.spawn((
        AudioPlayer::<AudioSource>(assets.load(path)),
        PlaybackSettings::DESPAWN,
    ));
}

/// Maps a ship code to its faction's victory-ditty stem under
/// `assets/music/victory/<stem>.mp3`. Stems come from the original
/// `victoryditty.dat` entry names (`HUMDITTY_WAV` → "hum", etc.).
/// The Ur-Quan share one ditty ("urq") in canon — both Kzer-Za and
/// Kohr-Ah map to it.
fn victory_ditty_stem(code: &str) -> &'static str {
    match code {
        "earcr" => "hum",
        "spael" => "spa",
        "yehte" => "yeh",
        "chmav" => "chm",
        "kzedr" => "urq",
        "mycpo" => "myc",
        "shosc" => "sho",
        "arisk" => "ari",
        "pkufu" => "pku",
        "ilwav" => "ilw",
        "thrto" => "thr",
        "vuxin" => "vux",
        "supbl" => "sup",
        "kohma" => "urq",
        "syrpe" => "syr",
        "andgu" => "and",
        "chebr" => "che",
        "druma" => "dru",
        "utwju" => "utw",
        "zfpst" => "zoq",
        "mmrxf" => "mmr",
        "orzne" => "orz",
        "slypr" => "sly",
        "umgdr" => "umg",
        "meltr" => "mel",
        "alabc" => "ala",
        // Fallback to the canon empty ditty if a new ship lands
        // without a faction mapping — keeps the audio system silent
        // instead of panicking on a missing file.
        _ => "for",
    }
}
