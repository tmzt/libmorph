// The camera viewfinder: one preview frame's I420 planes, uploaded as three
// R8Unorm textures, converted to RGB here (BT.601 full range), turned
// upright by whole quarter turns, cropped or letterboxed to the viewport.
// `camera/convert.rs` holds the same math on the CPU, with its tests.

struct Params {
    // uv' = uv * crop.xy + crop.zw, in the upright image.
    crop: vec4<f32>,
    // x: clockwise quarter turns still needed (0..3); y: mirror (1.0) or
    // not; z: the target is sRGB (1.0), so write linear values.
    flags: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var planes: sampler;
@group(0) @binding(2) var plane_y: texture_2d<f32>;
@group(0) @binding(3) var plane_u: texture_2d<f32>;
@group(0) @binding(4) var plane_v: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// A triangle strip of four vertices covering the viewport: (0,0) top left,
// (1,1) bottom right.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    var out: VertexOut;
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

// Where a point of the upright image comes from in the stored frame.
fn source_uv(turns: u32, uv: vec2<f32>) -> vec2<f32> {
    switch turns {
        case 1u: { return vec2<f32>(uv.y, 1.0 - uv.x); }
        case 2u: { return vec2<f32>(1.0 - uv.x, 1.0 - uv.y); }
        case 3u: { return vec2<f32>(1.0 - uv.y, uv.x); }
        default: { return uv; }
    }
}

fn yuv_to_rgb(y: f32, u: f32, v: f32) -> vec3<f32> {
    let cu = u - 128.0 / 255.0;
    let cv = v - 128.0 / 255.0;
    let rgb = vec3<f32>(
        y + 1.402 * cv,
        y - 0.344136 * cu - 0.714136 * cv,
        y + 1.772 * cu,
    );
    return clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var uv = in.uv * params.crop.xy + params.crop.zw;
    if params.flags.y > 0.5 {
        uv.x = 1.0 - uv.x;
    }
    let s = source_uv(u32(params.flags.x + 0.5), uv);
    let y = textureSample(plane_y, planes, s).r;
    let u = textureSample(plane_u, planes, s).r;
    let v = textureSample(plane_v, planes, s).r;
    var rgb = yuv_to_rgb(y, u, v);
    if params.flags.z > 0.5 {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
