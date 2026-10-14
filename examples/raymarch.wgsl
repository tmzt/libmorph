// A raymarched scene, all in this one shader: blended spheres and a box, a
// tilting ring and a checkered floor, with soft shadows, ambient occlusion
// and fog. The scene is read from eight storage buffers in the fragment
// stage, twice the downlevel baseline of four, so the app requires
// `max_storage_buffers_per_shader_stage: 8` through `MorphApp::limits`.

struct Frame {
    // surface width and height (physical pixels), seconds, 1.0 when the
    // surface is not sRGB and this shader must gamma-encode
    size_time: vec4<f32>,
    // camera yaw and pitch (radians), distance, unused
    camera: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
// Spheres: centre xyz, radius.
@group(0) @binding(1) var<storage, read> spheres: array<vec4<f32>>;
// Sphere motion: bob height, bob speed, phase, unused.
@group(0) @binding(2) var<storage, read> sphere_motion: array<vec4<f32>>;
// Boxes: centre xyz, corner radius.
@group(0) @binding(3) var<storage, read> box_centres: array<vec4<f32>>;
// Boxes: half extents xyz, spin speed about y.
@group(0) @binding(4) var<storage, read> box_sizes: array<vec4<f32>>;
// Rings: centre xyz, major radius.
@group(0) @binding(5) var<storage, read> rings: array<vec4<f32>>;
// Rings: minor radius, tilt, tilt swing, swing speed.
@group(0) @binding(6) var<storage, read> ring_shapes: array<vec4<f32>>;
// Materials: albedo rgb, specular. 0 is the floor, then the spheres, the
// boxes and the rings, in buffer order.
@group(0) @binding(7) var<storage, read> materials: array<vec4<f32>>;
// Lights: direction towards the light xyz, intensity.
@group(0) @binding(8) var<storage, read> lights: array<vec4<f32>>;

const FAR: f32 = 40.0;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the viewport.
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn rotate_x(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x, c * p.y - s * p.z, s * p.y + c * p.z);
}

fn rotate_y(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(c * p.x + s * p.z, p.y, c * p.z - s * p.x);
}

// Polynomial smooth minimum: blends two distances over `k`.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn sd_round_box(p: vec3<f32>, half_size: vec3<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - radius;
}

fn sd_torus(p: vec3<f32>, major: f32, minor: f32) -> f32 {
    return length(vec2<f32>(length(p.xz) - major, p.y)) - minor;
}

// Distance to the scene and the material index of the nearest surface.
fn map(p: vec3<f32>) -> vec2<f32> {
    let t = frame.size_time.z;
    var hit = vec2<f32>(p.y + 1.0, 0.0);

    // Spheres and boxes melt into one blob; its material is the nearest part.
    var blob = 1e9;
    var nearest = 1e9;
    var blob_material = 0.0;
    let sphere_count = arrayLength(&spheres);
    for (var i = 0u; i < sphere_count; i++) {
        let s = spheres[i];
        let m = sphere_motion[i];
        let centre = s.xyz + vec3<f32>(0.0, m.x * sin(t * m.y + m.z), 0.0);
        let d = length(p - centre) - s.w;
        blob = smin(blob, d, 0.35);
        if d < nearest {
            nearest = d;
            blob_material = f32(1u + i);
        }
    }
    let box_count = arrayLength(&box_centres);
    for (var i = 0u; i < box_count; i++) {
        let c = box_centres[i];
        let size = box_sizes[i];
        let d = sd_round_box(rotate_y(p - c.xyz, t * size.w), size.xyz, c.w);
        blob = smin(blob, d, 0.35);
        if d < nearest {
            nearest = d;
            blob_material = f32(1u + sphere_count + i);
        }
    }
    if blob < hit.x {
        hit = vec2<f32>(blob, blob_material);
    }

    let ring_count = arrayLength(&rings);
    for (var i = 0u; i < ring_count; i++) {
        let r = rings[i];
        let shape = ring_shapes[i];
        let tilt = shape.y + shape.z * sin(t * shape.w);
        let d = sd_torus(rotate_x(p - r.xyz, tilt), r.w, shape.x);
        if d < hit.x {
            hit = vec2<f32>(d, f32(1u + sphere_count + box_count + i));
        }
    }
    return hit;
}

fn normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(1.0, -1.0) * 0.0005;
    return normalize(
        e.xyy * map(p + e.xyy).x + e.yyx * map(p + e.yyx).x
            + e.yxy * map(p + e.yxy).x + e.xxx * map(p + e.xxx).x
    );
}

// Distance along the ray and the material, or material -1 for a miss.
fn march(origin: vec3<f32>, dir: vec3<f32>) -> vec2<f32> {
    var t = 0.0;
    for (var i = 0; i < 160; i++) {
        let h = map(origin + dir * t);
        if h.x < 0.0005 * max(t, 1.0) {
            return vec2<f32>(t, h.y);
        }
        t += h.x;
        if t > FAR {
            break;
        }
    }
    return vec2<f32>(t, -1.0);
}

fn soft_shadow(origin: vec3<f32>, dir: vec3<f32>) -> f32 {
    var lit = 1.0;
    var t = 0.02;
    for (var i = 0; i < 64; i++) {
        let h = map(origin + dir * t).x;
        lit = min(lit, 10.0 * h / t);
        t += clamp(h, 0.01, 0.25);
        if lit < 0.001 || t > 10.0 {
            break;
        }
    }
    return clamp(lit, 0.0, 1.0);
}

fn occlusion(p: vec3<f32>, n: vec3<f32>) -> f32 {
    var occluded = 0.0;
    var weight = 1.0;
    for (var i = 1; i <= 5; i++) {
        let h = 0.01 + 0.03 * f32(i);
        occluded += (h - map(p + n * h).x) * weight;
        weight *= 0.85;
    }
    return clamp(1.0 - 3.0 * occluded, 0.0, 1.0);
}

fn sky(dir: vec3<f32>) -> vec3<f32> {
    return mix(vec3<f32>(0.75, 0.82, 0.95), vec3<f32>(0.25, 0.45, 0.85), clamp(dir.y * 1.5, 0.0, 1.0));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = frame.size_time.xy;
    // The short side spans -1..1; y up.
    var uv = (2.0 * position.xy - size) / min(size.x, size.y);
    uv.y = -uv.y;

    let yaw = frame.camera.x;
    let pitch = frame.camera.y;
    let focus = vec3<f32>(0.0, -0.1, 0.0);
    let eye = focus + frame.camera.z * vec3<f32>(cos(pitch) * sin(yaw), sin(pitch), cos(pitch) * cos(yaw));
    let forward = normalize(focus - eye);
    let right = normalize(cross(forward, vec3<f32>(0.0, 1.0, 0.0)));
    let up = cross(right, forward);
    let dir = normalize(uv.x * right + uv.y * up + 2.0 * forward);

    var colour = sky(dir);
    let hit = march(eye, dir);
    if hit.y >= 0.0 {
        let p = eye + dir * hit.x;
        let n = normal(p);
        let material = materials[u32(hit.y + 0.5)];
        var albedo = material.rgb;
        if hit.y < 0.5 {
            let check = fract((floor(p.x) + floor(p.z)) * 0.5) * 2.0;
            albedo *= 0.6 + 0.4 * check;
        }
        let ao = occlusion(p, n);
        var lit = albedo * (0.3 * (0.5 + 0.5 * n.y) * ao) * vec3<f32>(0.6, 0.75, 1.0);
        let light_count = arrayLength(&lights);
        for (var i = 0u; i < light_count; i++) {
            let light = lights[i];
            let towards = normalize(light.xyz);
            let diffuse = clamp(dot(n, towards), 0.0, 1.0) * soft_shadow(p + n * 0.002, towards);
            let halfway = normalize(towards - dir);
            let specular = pow(clamp(dot(n, halfway), 0.0, 1.0), 48.0) * material.w;
            lit += light.w * diffuse * (albedo + specular) * vec3<f32>(1.0, 0.95, 0.85);
        }
        colour = mix(lit, sky(dir), 1.0 - exp(-0.003 * hit.x * hit.x));
    }

    colour = clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0));
    if frame.size_time.w > 0.5 {
        colour = pow(colour, vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(colour, 1.0);
}
