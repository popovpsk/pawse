struct PatchInfo {
    pcm_offset: u32,
    pcm_len: u32,
    base: i32,
    r0: u32,
    jlo: u32,
    jhi: u32,
    p: u32,
    q: u32,
    frames: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}
