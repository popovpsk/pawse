struct Params {
    m: u32,
    n: u32,
    k: u32,
    hw: u32,
    batch: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

override LOAD: u32 = 0u;
override BIAS: bool = true;
override SWISH: bool = false;
override RESIDUAL: bool = false;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read> src: array<f32>;
@group(0) @binding(4) var<storage, read> gate: array<f32>;
@group(0) @binding(5) var<storage, read> residual: array<f32>;
@group(0) @binding(6) var<storage, read_write> dst: array<f32>;

const TK: u32 = 16u;

var<workgroup> sa: array<vec4<f32>, 256>;
var<workgroup> sb: array<f32, 1024>;

fn a_at(k: u32, m: u32) -> f32 {
    if (k < params.k && m < params.m) {
        return a[k * params.m + m];
    }
    return 0.0;
}

fn b_at(k: u32, n: u32) -> f32 {
    if (k >= params.k || n >= params.n) {
        return 0.0;
    }
    let v = src[k * params.n + n];
    if (LOAD == 1u) {
        return v * gate[k * params.batch + n / params.hw];
    }
    return v;
}

fn frame_at(k: u32, n: u32) -> f32 {
    if (k >= params.k || n >= params.n) {
        return 0.0;
    }
    return src[(n / PATCH_FRAMES) * PATCH_SAMPLES + (n % PATCH_FRAMES) * HOP + k];
}

fn finish(m: u32, n: u32, value: f32) {
    if (m >= params.m || n >= params.n) {
        return;
    }
    var v = value;
    if (BIAS) {
        v = v + bias[m];
    }
    if (SWISH) {
        v = v * (1.0 / (1.0 + exp(-v)));
    }
    if (RESIDUAL) {
        v = v + residual[m * params.n + n];
    }
    dst[m * params.n + n] = v;
}

fn finish_row(m: u32, n: u32, row: vec4<f32>) {
    finish(m, n, row.x);
    finish(m, n + 1u, row.y);
    finish(m, n + 2u, row.z);
    finish(m, n + 3u, row.w);
}

@compute @workgroup_size(16, 16)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let t = lid.y * 16u + lid.x;
    let m0 = wid.y * 64u;
    let n0 = wid.x * 64u;
    let kk_load = t / 16u;
    let q = t % 16u;
    var acc0 = vec4<f32>(0.0);
    var acc1 = vec4<f32>(0.0);
    var acc2 = vec4<f32>(0.0);
    var acc3 = vec4<f32>(0.0);
    for (var k0 = 0u; k0 < params.k; k0 += TK) {
        let gk = k0 + kk_load;
        let gm = m0 + q * 4u;
        sa[t] = vec4<f32>(a_at(gk, gm), a_at(gk, gm + 1u), a_at(gk, gm + 2u), a_at(gk, gm + 3u));
        if (LOAD == 2u) {
            let n_local = t / 4u;
            let kq = (t % 4u) * 4u;
            for (var i = 0u; i < 4u; i++) {
                sb[(kq + i) * 64u + n_local] = frame_at(k0 + kq + i, n0 + n_local);
            }
        } else {
            let gn = n0 + q * 4u;
            for (var i = 0u; i < 4u; i++) {
                sb[kk_load * 64u + q * 4u + i] = b_at(gk, gn + i);
            }
        }
        workgroupBarrier();
        var p0 = vec4<f32>(0.0);
        var p1 = vec4<f32>(0.0);
        var p2 = vec4<f32>(0.0);
        var p3 = vec4<f32>(0.0);
        for (var kk = 0u; kk < TK; kk++) {
            let av = sa[kk * 16u + lid.y];
            let ib = kk * 64u + lid.x * 4u;
            let bv = vec4<f32>(sb[ib], sb[ib + 1u], sb[ib + 2u], sb[ib + 3u]);
            p0 = p0 + av.x * bv;
            p1 = p1 + av.y * bv;
            p2 = p2 + av.z * bv;
            p3 = p3 + av.w * bv;
        }
        acc0 = acc0 + p0;
        acc1 = acc1 + p1;
        acc2 = acc2 + p2;
        acc3 = acc3 + p3;
        workgroupBarrier();
    }
    let m = m0 + lid.y * 4u;
    let n = n0 + lid.x * 4u;
    finish_row(m, n, acc0);
    finish_row(m + 1u, n, acc1);
    finish_row(m + 2u, n, acc2);
    finish_row(m + 3u, n, acc3);
}
