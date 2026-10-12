struct Params {
    channels: u32,
    squeeze: u32,
    batch: u32,
    lanes: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> w1: array<f32>;
@group(0) @binding(2) var<storage, read> b1: array<f32>;
@group(0) @binding(3) var<storage, read> w2: array<f32>;
@group(0) @binding(4) var<storage, read> b2: array<f32>;
@group(0) @binding(5) var<storage, read> pool: array<f32>;
@group(0) @binding(6) var<storage, read_write> gate: array<f32>;

var<workgroup> pooled: array<f32, 1152>;
var<workgroup> partial: array<f32, 256>;
var<workgroup> squeezed: array<f32, 64>;

@compute @workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let b = wid.x;
    let t = lid.x;
    let channels = params.channels;
    let squeeze = params.squeeze;
    let lanes = params.lanes;
    for (var c = t; c < channels; c += 256u) {
        pooled[c] = pool[c * params.batch + b];
    }
    workgroupBarrier();
    let j = t / lanes;
    let lane = t % lanes;
    var acc = 0.0;
    if (j < squeeze) {
        for (var c = lane; c < channels; c += lanes) {
            acc = acc + w1[c * squeeze + j] * pooled[c];
        }
    }
    partial[t] = acc;
    for (var span = lanes / 2u; span > 0u; span = span / 2u) {
        workgroupBarrier();
        if (lane < span) {
            partial[t] = partial[t] + partial[t + span];
        }
    }
    if (lane == 0u && j < squeeze) {
        let v = partial[t] + b1[j];
        squeezed[j] = v * (1.0 / (1.0 + exp(-v)));
    }
    workgroupBarrier();
    for (var c = t; c < channels; c += 256u) {
        var e = 0.0;
        for (var k = 0u; k < squeeze; k++) {
            e = e + w2[k * channels + c] * squeezed[k];
        }
        gate[c * params.batch + b] = 1.0 / (1.0 + exp(-(e + b2[c])));
    }
}
