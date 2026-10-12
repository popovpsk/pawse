struct Params {
    n: u32,
    bins: u32,
    bands: u32,
    pad0: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> patches: array<PatchInfo>;
@group(0) @binding(2) var<storage, read> spectrum: array<f32>;
@group(0) @binding(3) var<storage, read> weights: array<f32>;
@group(0) @binding(4) var<storage, read> filters: array<u32>;
@group(0) @binding(5) var<storage, read_write> mel: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = gid.x;
    let band = gid.y;
    if (n >= params.n || band >= params.bands) {
        return;
    }
    var value = 0.0;
    if (n % PATCH_FRAMES < patches[n / PATCH_FRAMES].frames) {
        let first = filters[band * 3u];
        let count = filters[band * 3u + 1u];
        let offset = filters[band * 3u + 2u];
        var energy = 0.0;
        for (var i = 0u; i < count; i++) {
            let bin = first + i;
            let re = spectrum[bin * params.n + n];
            let im = spectrum[(params.bins + bin) * params.n + n];
            energy = energy + weights[offset + i] * (re * re + im * im);
        }
        value = log2(1.0 + 10000.0 * energy) * 0.30102999566398120;
    }
    mel[n * params.bands + band] = value;
}
