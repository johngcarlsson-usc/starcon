//! Power-up HUD for boss co-op: a brief pop-up toast when a fighter
//! grabs a pickup, plus a live strip of chips counting down the *timed*
//! buffs a fighter is carrying (shield, point-defense, bazooka,
//! overcharge, nitrous).
//!
//! Pure UI/visual. Toasts are spawned in response to the `PowerUpPicked`
//! message and fade themselves out; the buff chips are a pre-spawned
//! pool (filled as needed, rest hidden) refreshed every frame from the
//! buff components on the player ships. Boss-only.

use bevy::prelude::*;

use crate::ship::{
    BazookaActive, MatchConfig, NitrousActive, OverchargeActive, PointDefenseActive, PowerUpKind,
    PowerUpPicked, ShieldActive, Ship,
};

pub struct PowerUpHudPlugin;

impl Plugin for PowerUpHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_buff_chips)
            .add_systems(Update, (spawn_toasts, fade_toasts, update_buff_chips));
    }
}

/// How many timed-buff chips can show at once (≤ players × buff kinds).
const CHIP_POOL: usize = 16;
/// Seconds a pickup toast lives.
const TOAST_LIFE: f32 = 2.2;

/// Player accent colour — mirrors `hud`/`indicator`/`minimap`.
fn slot_color(slot: usize) -> Color {
    match slot {
        0 => Color::srgb(0.6, 0.9, 1.0),
        1 => Color::srgb(1.0, 0.7, 0.6),
        2 => Color::srgb(0.6, 1.0, 0.7),
        _ => Color::srgb(1.0, 0.6, 1.0),
    }
}

// ---- Pickup toast ---------------------------------------------------

/// A transient "P1 — BAZOOKA" banner that floats up and fades out.
#[derive(Component)]
struct PickupToast {
    remaining: f32,
    base_top: f32,
    color: Color,
}

fn spawn_toasts(mut commands: Commands, mut picked: MessageReader<PowerUpPicked>) {
    for ev in picked.read() {
        // Stagger by slot so two simultaneous grabs don't stack on the
        // same line.
        let base_top = 18.0 + ev.slot as f32 * 30.0;
        let color = ev.kind.color();
        commands.spawn((
            PickupToast { remaining: TOAST_LIFE, base_top, color },
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(base_top),
                // Roughly centred; exact centring isn't worth a width
                // measure for a 2-second banner.
                left: Val::Percent(38.0),
                ..default()
            },
            Text::new(format!("P{} — {}", ev.slot + 1, ev.kind.label())),
            TextFont { font_size: 22.0, ..default() },
            TextColor(color),
            ZIndex(60),
        ));
    }
}

fn fade_toasts(
    mut commands: Commands,
    time: Res<Time>,
    mut toasts: Query<(Entity, &mut PickupToast, &mut Node, &mut TextColor)>,
) {
    let dt = time.delta_secs();
    for (e, mut toast, mut node, mut col) in &mut toasts {
        toast.remaining -= dt;
        if toast.remaining <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let t = toast.remaining / TOAST_LIFE; // 1 → 0
        // Rise ~28px over its life and fade the last ~60%.
        node.top = Val::Px(toast.base_top - (1.0 - t) * 28.0);
        col.0 = toast.color.with_alpha((t / 0.6).min(1.0));
    }
}

// ---- Active-buff chip strip -----------------------------------------

/// One pooled chip in the top-left buff strip.
#[derive(Component)]
struct BuffChip;

fn spawn_buff_chips(mut commands: Commands) {
    for _ in 0..CHIP_POOL {
        commands.spawn((
            BuffChip,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(14.0),
                top: Val::Px(0.0),
                ..default()
            },
            Text::new(""),
            TextFont { font_size: 15.0, ..default() },
            TextColor(Color::WHITE),
            Visibility::Hidden,
            ZIndex(55),
        ));
    }
}

/// One buff line to draw: which fighter, its label/colour, seconds left.
struct ChipData {
    slot: usize,
    label: &'static str,
    color: Color,
    secs: f32,
}

fn update_buff_chips(
    config: Res<MatchConfig>,
    ships: Query<(
        &Ship,
        Option<&ShieldActive>,
        Option<&PointDefenseActive>,
        Option<&BazookaActive>,
        Option<&OverchargeActive>,
        Option<&NitrousActive>,
    )>,
    mut chips: Query<(&mut Node, &mut Text, &mut TextColor, &mut Visibility), With<BuffChip>>,
) {
    // The buff strip is a boss-co-op fixture (in normal matches a ship's
    // own force field would show up here too, which we don't want).
    if !config.boss {
        for (_, _, _, mut vis) in &mut chips {
            *vis = Visibility::Hidden;
        }
        return;
    }

    let mut wanted: Vec<ChipData> = Vec::new();
    for (ship, shield, pd, bz, oc, nitro) in &ships {
        let slot = ship.player_slot;
        let sc = slot_color(slot);
        // Tint each chip toward the buff's pickup colour but keep it
        // legible by blending lightly with the slot accent.
        let mut push = |label: &'static str, secs: f32, kind: PowerUpKind| {
            let k = kind.color().to_srgba();
            let s = sc.to_srgba();
            let blend = Color::srgb(
                (k.red * 0.7 + s.red * 0.3).min(1.0),
                (k.green * 0.7 + s.green * 0.3).min(1.0),
                (k.blue * 0.7 + s.blue * 0.3).min(1.0),
            );
            wanted.push(ChipData { slot, label, color: blend, secs });
        };
        if let Some(s) = shield {
            push("SHIELD", s.remaining, PowerUpKind::Shield);
        }
        if let Some(p) = pd {
            push("POINT-DEF", p.remaining, PowerUpKind::PointDefense);
        }
        if let Some(b) = bz {
            push("BAZOOKA", b.remaining, PowerUpKind::Bazooka);
        }
        if let Some(o) = oc {
            push("OVERCHARGE", o.remaining, PowerUpKind::Overcharge);
        }
        if let Some(n) = nitro {
            push("NITROUS", n.remaining, PowerUpKind::Nitrous);
        }
    }
    // Group reads better sorted by slot then by time left.
    wanted.sort_by(|a, b| {
        a.slot
            .cmp(&b.slot)
            .then(b.secs.partial_cmp(&a.secs).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut it = wanted.into_iter().enumerate();
    for (mut node, mut text, mut col, mut vis) in &mut chips {
        match it.next() {
            Some((i, c)) => {
                node.top = Val::Px(14.0 + i as f32 * 20.0);
                **text = format!("P{} {} {:.0}s", c.slot + 1, c.label, c.secs.max(0.0).ceil());
                col.0 = c.color;
                *vis = Visibility::Inherited;
            }
            None => {
                *vis = Visibility::Hidden;
            }
        }
    }
}
