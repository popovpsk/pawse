struct Params {
    channels: u32,
    batch: u32,
    plane: u32,
    pad0: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.channels * params.batch) {
        return;
    }
    let b = i / params.channels;
    let c = i % params.channels;
    let start = (c * params.batch + b) * params.plane;
    var sum = 0.0;
    for (var s = 0u; s < params.plane; s++) {
        sum = sum + src[start + s];
    }
    dst[b * params.channels + c] = sum / f32(params.plane);
}
