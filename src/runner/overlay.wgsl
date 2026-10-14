// Masks what the emulated device's glass would hide: the rounded display
// corners and the camera cutouts. Lengths are device pixels; `scale` maps
// them onto the window.

struct Cutout {
    // x, y, width, height
    rect: vec4<f32>,
    // kind (1 circle, 2 capsule, 3 notch, 4 rect), notch corner radius
    shape: vec4<f32>,
}

struct Uniforms {
    // scale x, scale y, device width, device height
    params: vec4<f32>,
    // top-left, top-right, bottom-right, bottom-left radii
    corners: vec4<f32>,
    // cutout count
    info: vec4<f32>,
    cutouts: array<Cutout, 4>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the viewport.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// Signed distance to a box of half-size `half` with corner radius `r`.
fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn cutout_distance(p: vec2<f32>, cutout: Cutout) -> f32 {
    let half = cutout.rect.zw * 0.5;
    let local = p - (cutout.rect.xy + half);
    let kind = cutout.shape.x;
    if kind < 1.5 {
        return length(local) - min(half.x, half.y);
    }
    if kind < 2.5 {
        return rounded_box(local, half, min(half.x, half.y));
    }
    if kind < 3.5 {
        return rounded_box(local, half, min(cutout.shape.y, min(half.x, half.y)));
    }
    return rounded_box(local, half, 0.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = position.xy / u.params.xy;
    let half = u.params.zw * 0.5;
    let centered = p - half;

    var radius = u.corners.x;
    if centered.x >= 0.0 && centered.y < 0.0 {
        radius = u.corners.y;
    } else if centered.x >= 0.0 && centered.y >= 0.0 {
        radius = u.corners.z;
    } else if centered.x < 0.0 && centered.y >= 0.0 {
        radius = u.corners.w;
    }
    var cover = smoothstep(-0.5, 0.5, rounded_box(centered, half, radius));

    for (var i = 0u; i < u32(u.info.x); i++) {
        cover = max(cover, 1.0 - smoothstep(-0.5, 0.5, cutout_distance(p, u.cutouts[i])));
    }
    return vec4<f32>(0.0, 0.0, 0.0, cover);
}
