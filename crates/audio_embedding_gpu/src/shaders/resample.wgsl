@group(0) @binding(0) var<storage, read> patches: array<PatchInfo>;
@group(0) @binding(1) var<storage, read> pcm: array<f32>;
@group(0) @binding(2) var<storage, read_write> y: array<f32>;

fn sample(info: PatchInfo, idx: i32) -> f32 {
    if (idx < 0 || idx >= i32(info.pcm_len)) {
        return 0.0;
    }
    return pcm[info.pcm_offset + u32(idx)];
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let j = gid.x;
    let pix = gid.y;
    if (j >= PATCH_SAMPLES) {
        return;
    }
    let info = patches[pix];
    var value = 0.0;
    if (j >= info.jlo && j < info.jhi) {
        let t = info.r0 + (j % info.q) * info.p;
        let idx = info.base + i32((j / info.q) * info.p + t / info.q);
        let rem = t % info.q;
        let a = sample(info, idx);
        if (rem == 0u) {
            value = a;
        } else {
            let b = sample(info, idx + 1);
            value = a + (b - a) * (f32(rem) / f32(info.q));
        }
    }
    y[pix * PATCH_SAMPLES + j] = value;
}
