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
            .add_systems(OnEnter(AppState::InMatch), start_combat_music)
            .add_systems(
                Update,
                (play_ship_death_boom, play_victory_ditty, tick_music_ducking)
                    .run_if(in_state(AppState::InMatch)),
            );
    }
}

/// Volume the combat loop ducks to while a captain's ultimate voice
/// line is playing. Low enough to clear the speech, not so low the
/// loop dies entirely (sudden silence would feel like a bug). User
/// asked for ~10 %.
const DUCK_VOLUME: f32 = 0.10;
/// Linear lerp rate in volume-units per second. ~3.0 = ~0.3 s for a
/// full 1.0 → 0.0 fade — slow enough to feel intentional, fast enough
/// not to step on a one-word voice line.
const DUCK_FADE_RATE: f32 = 3.0;

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

/// Canon `melee.dat:MELEEMUS_MOD` — 107 s combat loop the original
/// plays for the duration of a battle.
fn start_combat_music(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut track: ResMut<MusicTrack>,
) {
    let e = commands
        .spawn((
            AudioPlayer::<AudioSource>(assets.load("music/melee.mp3")),
            PlaybackSettings::LOOP,
        ))
        .id();
    track.0 = Some(e);
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
/// guarded by `Changed<MatchPhase>`). Also kills the combat loop so
/// the ditty plays clean — the state stays in `InMatch` during the
/// post-match summary, so the OnExit(InMatch) `stop_music` hook
/// doesn't fire here.
fn play_victory_ditty(
    mut commands: Commands,
    assets: Res<AssetServer>,
    phase: Res<MatchPhase>,
    outcome: Res<MatchOutcome>,
    ships: Query<&Ship>,
    mut track: ResMut<MusicTrack>,
) {
    if !phase.is_changed() || *phase != MatchPhase::PostMatch {
        return;
    }
    // Stop the combat loop regardless of whether we have a winner
    // ditty to play (draws should still go silent for the summary).
    if let Some(e) = track.0.take() {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
        }
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

/// While any `UltimateVoicePlayer` entity is alive, lerp the combat
/// music's sink volume down to `DUCK_VOLUME`; lerp it back to 1.0 the
/// moment the voice entity self-despawns. `AudioSink` is added by
/// bevy_audio once playback starts, so we tolerate it being absent
/// for the first few frames after `start_combat_music`.
fn tick_music_ducking(
    time: Res<Time<Real>>,
    track: Res<MusicTrack>,
    voices: Query<(), With<crate::ultimate::UltimateVoicePlayer>>,
    mut sinks: Query<&mut AudioSink>,
) {
    let Some(music_entity) = track.0 else { return };
    let Ok(mut sink) = sinks.get_mut(music_entity) else {
        return;
    };
    let target = if voices.is_empty() { 1.0 } else { DUCK_VOLUME };
    let current = sink.volume().to_linear();
    let dt = time.delta_secs();
    let next = if current < target {
        (current + DUCK_FADE_RATE * dt).min(target)
    } else {
        (current - DUCK_FADE_RATE * dt).max(target)
    };
    // Skip the per-frame write when we're already at the target —
    // avoids touching the sink (and its mutation tracking) every
    // tick during the steady state.
    if (next - current).abs() > 1e-4 {
        sink.set_volume(bevy::audio::Volume::Linear(next));
    }
}
