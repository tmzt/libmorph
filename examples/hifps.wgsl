// A bar sweeping over a gradient. Its position comes from simulated time,
// not from a frame counter, so it crosses the screen at the same speed
// whatever the frame rate - which is the point of the example: the loop
// runs as fast as the surface allows, and the motion does not speed up
// with it.

struct Frame {
    // x: seconds of simulated time. yz: the surface in physical pixels.
    time_size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the clip volume, so there are no vertex buffers.
    var xy = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(xy[index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = max(frame.time_size.yz, vec2<f32>(1.0, 1.0));
    let uv = pos.xy / size;

    // Once every two seconds, left to right.
    let sweep = fract(frame.time_size.x * 0.5);
    let bar = smoothstep(0.035, 0.0, abs(uv.x - sweep));

    let background = vec3<f32>(0.04, 0.05, 0.09) + vec3<f32>(0.0, 0.07, 0.15) * uv.y;
    return vec4<f32>(background + vec3<f32>(0.95, 0.82, 0.35) * bar, 1.0);
}
