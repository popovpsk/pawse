#[path = "../src/onnx.rs"]
mod onnx;

use std::path::PathBuf;

use audio_embedding::Prepared;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let (Some(multi), Some(styles), Some(out)) = (args.next(), args.next(), args.next()) else {
        return Err(
            "usage: convert <discogs_multi_embeddings-effnet-bs64-1.onnx> \
             <discogs-effnet-bsdynamic-1.onnx> <out>"
                .into(),
        );
    };
    let probe = audio_embedding_gpu::probe_input();
    let mel: Vec<f32> = probe.iter().flat_map(Prepared::reference_mel).collect();
    let tensors = onnx::weights(&multi, &styles, &mel)?;
    let bytes = audio_embedding_gpu::weights::encode(&tensors)?;
    std::fs::write(&out, &bytes)?;
    println!("{}: {} bytes", out.display(), bytes.len());
    Ok(())
}
