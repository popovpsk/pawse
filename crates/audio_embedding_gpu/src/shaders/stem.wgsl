struct Params {
    batch: u32,
    h: u32,
    w: u32,
    ho: u32,
    wo: u32,
    channels: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> weights: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read> src: array<f32>;
@group(0) @binding(4) var<storage, read_write> dst: array<f32>;

var<workgroup> taps: array<f32, 288>;
var<workgroup> offsets: array<f32, 32>;

@compute @workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    for (var i = lid.x; i < params.channels * 9u; i += 256u) {
        taps[i] = weights[i];
    }
    for (var i = lid.x; i < params.channels; i += 256u) {
        offsets[i] = bias[i];
    }
    workgroupBarrier();
    let o = gid.x;
    let plane = params.ho * params.wo;
    if (o >= params.batch * plane) {
        return;
    }
    let b = o / plane;
    let pos = o % plane;
    let oy = pos / params.wo;
    let ox = pos % params.wo;
    var x: array<f32, 9>;
    for (var ky = 0u; ky < 3u; ky++) {
        for (var kx = 0u; kx < 3u; kx++) {
            let iy = i32(oy * 2u + ky) - 1;
            let ix = i32(ox * 2u + kx) - 1;
            var v = 0.0;
            if (iy >= 0 && iy < i32(params.h) && ix >= 0 && ix < i32(params.w)) {
                v = src[(b * params.h + u32(iy)) * params.w + u32(ix)];
            }
            x[ky * 3u + kx] = v;
        }
    }
    for (var c = 0u; c < params.channels; c++) {
        var acc = 0.0;
        for (var i = 0u; i < 9u; i++) {
            acc = acc + taps[c * 9u + i] * x[i];
        }
        let v = acc + offsets[c];
        dst[(c * params.batch + b) * plane + pos] = v * (1.0 / (1.0 + exp(-v)));
    }
}
