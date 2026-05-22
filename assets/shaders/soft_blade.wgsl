// Soft-edge triangle blade material. The mesh stores barycentric
// coordinates in UV_0 (vertex A → (1, 0), vertex B → (0, 0),
// vertex C → (0, 1)); inside the triangle the interpolated UV
// gives barycentric weights (uv.x, uv.y, 1 - uv.x - uv.y). The
// minimum of those three is the (normalised) distance to the
// nearest edge — we smoothstep it into an alpha falloff so the
// triangle's edges feather instead of reading as sharp polygons.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(2) @binding(0) var<uniform> color: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let u = in.uv.x;
    let v = in.uv.y;
    let w = 1.0 - u - v;
    let edge_dist = min(min(u, v), w);
    // FEATHER controls how wide the soft band is, in barycentric
    // units. 0.18 ≈ 18% of the half-width — generous enough that
    // even thin blade slivers still feel soft.
    let alpha = smoothstep(0.0, 0.18, edge_dist);
    return vec4<f32>(color.rgb, color.a * alpha);
}
