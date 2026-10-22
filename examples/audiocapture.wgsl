// The audiocapture example's scrolling spectrogram. The texture is a ring of
// columns (x) by frequency bands (y, row 0 lowest); `next` is the column the
// analysis writes next, so `next - 1` is the newest and goes at the right.
// A band across the top shows the capture mode by colour.

struct Ring {
    next: u32,
    columns: u32,
    rows: u32,
    // The capture mode's place in the example's `MODES`.
    mode: u32,
}

@group(0) @binding(0) var spectrum: texture_2d<f32>;
@group(0) @binding(1) var<uniform> ring: Ring;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    // One triangle covering the screen.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOut;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

// Black, blue, magenta, orange, white.
fn ramp(t: f32) -> vec3<f32> {
    let stops = array<vec3<f32>, 5>(
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(0.05, 0.05, 0.45),
        vec3<f32>(0.55, 0.05, 0.55),
        vec3<f32>(1.0, 0.45, 0.05),
        vec3<f32>(1.0, 1.0, 1.0),
    );
    let x = clamp(t, 0.0, 1.0) * 4.0;
    let i = min(u32(x), 3u);
    return mix(stops[i], stops[i + 1u], x - f32(i));
}

// One colour per capture mode, in `MODES` order.
fn mode_colour(mode: u32) -> vec3<f32> {
    let colours = array<vec3<f32>, 7>(
        vec3<f32>(0.5, 0.5, 0.5),   // Default
        vec3<f32>(0.9, 0.9, 0.2),   // Generic
        vec3<f32>(0.2, 0.8, 0.9),   // Camcorder
        vec3<f32>(0.3, 0.9, 0.3),   // VoiceRecognition
        vec3<f32>(0.2, 0.4, 1.0),   // VoiceCommunication
        vec3<f32>(1.0, 0.3, 0.3),   // Unprocessed
        vec3<f32>(0.9, 0.4, 0.9),   // VoicePerformance
    );
    return colours[min(mode, 6u)];
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if in.uv.y < 0.06 {
        return vec4<f32>(mode_colour(ring.mode), 1.0);
    }
    // Columns back from the newest at the right edge.
    let back = min(u32((1.0 - in.uv.x) * f32(ring.columns)), ring.columns - 1u);
    let column = (ring.next + ring.columns - 1u - back) % ring.columns;
    // Low frequencies at the bottom of the screen.
    let row = min(u32((1.0 - in.uv.y) * f32(ring.rows)), ring.rows - 1u);
    let level = textureLoad(spectrum, vec2<u32>(column, row), 0).r;
    return vec4<f32>(ramp(level), 1.0);
}
