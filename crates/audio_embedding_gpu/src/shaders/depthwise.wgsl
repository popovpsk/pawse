struct Params {
    channels: u32,
    batch: u32,
    h: u32,
    w: u32,
    ho: u32,
    wo: u32,
    lanes: u32,
    pad0: u32,
}

override KERNEL: u32 = 3u;
override STRIDE: u32 = 1u;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> weights: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read> src: array<f32>;
@group(0) @binding(4) var<storage, read_write> dst: array<f32>;
@group(0) @binding(5) var<storage, read_write> pool: array<f32>;

var<workgroup> tile: array<f32, TILE>;
var<workgroup> taps: array<f32, 400>;
var<workgroup> partial: array<f32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let t = lid.x;
    let lanes = params.lanes;
    let per = 256u / lanes;
    let planes = params.channels * params.batch;
    let first = wid.x * per;
    let plane_in = params.h * params.w;
    let plane = params.ho * params.wo;
    let kk = KERNEL * KERNEL;
    let held = min(per, planes - first);
    let pad = KERNEL / 2u;
    let wp = params.w + 2u * pad;
    let padded = (params.h + 2u * pad) * wp;
    for (var i = t; i < held * padded; i += 256u) {
        let p = i / padded;
        let r = i % padded;
        let y = i32(r / wp) - i32(pad);
        let x = i32(r % wp) - i32(pad);
        var v = 0.0;
        if (y >= 0 && y < i32(params.h) && x >= 0 && x < i32(params.w)) {
            v = src[(first + p) * plane_in + u32(y) * params.w + u32(x)];
        }
        tile[i] = v;
    }
    for (var i = t; i < held * kk; i += 256u) {
        let c = (first + i / kk) / params.batch;
        taps[i] = weights[c * kk + i % kk];
    }
    workgroupBarrier();
    let slot = t / lanes;
    let lane = t % lanes;
    let plane_id = first + slot;
    var sum = 0.0;
    if (slot < held) {
        let local = slot * padded;
        let w0 = slot * kk;
        let b = bias[plane_id / params.batch];
        let plane_out = plane_id * plane;
        for (var o = lane; o < plane; o += lanes) {
            let oy = o / params.wo;
            let ox = o % params.wo;
            let corner = local + oy * STRIDE * wp + ox * STRIDE;
            var acc = 0.0;
            for (var ky = 0u; ky < KERNEL; ky++) {
                for (var kx = 0u; kx < KERNEL; kx++) {
                    acc = acc + taps[w0 + ky * KERNEL + kx] * tile[corner + ky * wp + kx];
                }
            }
            let v = acc + b;
            let act = v * (1.0 / (1.0 + exp(-v)));
            dst[plane_out + o] = act;
            sum = sum + act;
        }
    }
    partial[t] = sum;
    for (var span = lanes / 2u; span > 0u; span = span / 2u) {
        workgroupBarrier();
        if (lane < span) {
            partial[t] = partial[t] + partial[t + span];
        }
    }
    if (lane == 0u && slot < held) {
        pool[plane_id] = partial[t] / f32(plane);
    }
}
