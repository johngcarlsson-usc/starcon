// Radial alpha fade — samples the portrait texture and multiplies
// alpha by a soft falloff so the rectangular sprite edges disappear.
// `params.x` is a top-level alpha multiplier driven from the
// cinematic's fade-in/out timer.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(2) @binding(0) var portrait_tex: texture_2d<f32>;
@group(2) @binding(1) var portrait_sampler: sampler;
@group(2) @binding(2) var<uniform> params: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let c = textureSample(portrait_tex, portrait_sampler, uv);
    // Distance from sprite center, normalised so 1.0 hits the
    // shortest edge. Corners get >1.0 → faded out further.
    let d = length(uv - vec2<f32>(0.5, 0.5)) * 2.0;
    let fade = 1.0 - smoothstep(0.55, 0.98, d);
    return vec4<f32>(c.rgb, c.a * fade * params.x);
}
