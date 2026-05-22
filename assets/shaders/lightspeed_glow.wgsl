// Pulsating bluish-white radiant glow used by the Earthling
// "jump-to-light-speed" ultimate. Renders a soft circular halo:
// brightest at the center, transparent at the edges. `params.x`
// is a global alpha multiplier driven by the cinematic timer
// (and pulsed externally for the breathing effect).

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(2) @binding(0) var<uniform> params: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let d = length(uv - vec2<f32>(0.5, 0.5)) * 2.0;
    // Sharper hot core, soft falloff outward — gives the radiant
    // "energy buildup" feel rather than a flat circle.
    let core = 1.0 - smoothstep(0.0, 0.55, d);
    let halo = 1.0 - smoothstep(0.30, 1.0, d);
    let alpha = (core * 0.85 + halo * 0.35) * params.x;
    // Bluish-white tint: hot core skews white, halo skews blue.
    let r = 0.75 + 0.25 * core;
    let g = 0.85 + 0.15 * core;
    let b = 1.0;
    return vec4<f32>(r, g, b, alpha);
}
