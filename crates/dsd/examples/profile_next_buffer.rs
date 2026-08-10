use std::time::{Duration, Instant};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: profile_next_buffer <path.dsf|.dff> [seconds]");
    let run_secs: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(15);

    let mut source = dsd::DsdSource::open(std::path::Path::new(&path)).expect("open DSD file");
    let params = source.params();
    eprintln!(
        "params: channels={} dsd_rate={} pcm_sample_rate={}",
        params.channels, params.dsd_rate, params.pcm_sample_rate
    );

    let start = Instant::now();
    let deadline = start + Duration::from_secs(run_secs);
    let mut total_samples: u64 = 0;
    let mut loops = 0u32;
    while Instant::now() < deadline {
        while let Some(batch) = source.next_buffer().expect("decode") {
            total_samples += batch.len() as u64;
            if Instant::now() >= deadline {
                break;
            }
        }
        source.seek(0.0).expect("seek");
        loops += 1;
    }

    let elapsed = start.elapsed();
    eprintln!(
        "decoded {total_samples} samples over {loops} loop(s) in {:.2}s -> {:.0} samples/sec",
        elapsed.as_secs_f64(),
        total_samples as f64 / elapsed.as_secs_f64()
    );
}
