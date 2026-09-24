use std::time::Duration;

use audio_common::{AudioBatch, AudioSource, StreamParams};
use audio_decoder::{Decoder, MediaStream};

const AHEAD_BATCHES: usize = 96;
const SEND_SLICE: Duration = Duration::from_millis(50);

pub type Interrupt = Box<dyn Fn() + Send + Sync>;

type Item = (u64, Result<Option<AudioBatch>, String>);

enum Control {
    Seek { epoch: u64, position: f32 },
}

pub(crate) enum Poll {
    Ready(AudioBatch),
    Pending,
    Ended,
    Failed(String),
}

pub struct StreamingSource {
    params: StreamParams,
    duration: Option<Duration>,
    control: flume::Sender<Control>,
    batches: flume::Receiver<Item>,
    epoch: u64,
    interrupt: Interrupt,
}

impl std::fmt::Debug for StreamingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingSource")
            .field("params", &self.params)
            .field("duration", &self.duration)
            .finish_non_exhaustive()
    }
}

impl StreamingSource {
    pub fn open(
        stream: Box<dyn MediaStream>,
        extension: Option<String>,
        interrupt: Interrupt,
    ) -> Result<Self, String> {
        let (opened_tx, opened_rx) = flume::bounded(1);
        let (control_tx, control_rx) = flume::unbounded();
        let (batch_tx, batch_rx) = flume::bounded(AHEAD_BATCHES);
        std::thread::Builder::new()
            .name("stream-decoder".into())
            .spawn(move || {
                let decoder = match Decoder::open_stream(stream, extension.as_deref()) {
                    Ok(decoder) => decoder,
                    Err(e) => {
                        let _ = opened_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                if opened_tx
                    .send(Ok((decoder.params(), decoder.duration())))
                    .is_err()
                {
                    return;
                }
                drop(opened_tx);
                run_decoder(decoder, control_rx, batch_tx);
            })
            .map_err(|e| e.to_string())?;
        match opened_rx.recv() {
            Ok(Ok((params, duration))) => Ok(Self {
                params,
                duration,
                control: control_tx,
                batches: batch_rx,
                epoch: 0,
                interrupt,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the stream decoder stopped".into()),
        }
    }

    pub fn params(&self) -> StreamParams {
        self.params
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub(crate) fn seek(&mut self, position: f32) -> Result<(), String> {
        self.epoch += 1;
        self.control
            .send(Control::Seek {
                epoch: self.epoch,
                position,
            })
            .map_err(|_| "the stream decoder stopped".to_string())
    }

    pub(crate) fn poll(&mut self) -> Poll {
        loop {
            match self.batches.try_recv() {
                Ok((epoch, _)) if epoch != self.epoch => continue,
                Ok((_, Ok(Some(batch)))) => return Poll::Ready(batch),
                Ok((_, Ok(None))) => return Poll::Ended,
                Ok((_, Err(e))) => return Poll::Failed(e),
                Err(flume::TryRecvError::Empty) => return Poll::Pending,
                Err(flume::TryRecvError::Disconnected) => {
                    return Poll::Failed("the stream decoder stopped".into());
                }
            }
        }
    }
}

impl Drop for StreamingSource {
    fn drop(&mut self) {
        (self.interrupt)();
    }
}

fn run_decoder(
    mut decoder: Decoder,
    control: flume::Receiver<Control>,
    batches: flume::Sender<Item>,
) {
    let mut epoch = 0;
    let mut finished = false;
    loop {
        let command = if finished {
            match control.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            }
        } else {
            match control.try_recv() {
                Ok(command) => Some(command),
                Err(flume::TryRecvError::Empty) => None,
                Err(flume::TryRecvError::Disconnected) => return,
            }
        };
        let item = match command {
            Some(Control::Seek {
                epoch: next,
                position,
            }) => {
                epoch = next;
                finished = false;
                match decoder.seek(position) {
                    Ok(_) => continue,
                    Err(e) => Err(e.to_string()),
                }
            }
            None => decoder.next_buffer().map_err(|e| e.to_string()),
        };
        finished = !matches!(item, Ok(Some(_)));
        if !deliver(&batches, &control, (epoch, item)) {
            return;
        }
    }
}

fn deliver(batches: &flume::Sender<Item>, control: &flume::Receiver<Control>, item: Item) -> bool {
    let mut item = item;
    loop {
        match batches.send_timeout(item, SEND_SLICE) {
            Ok(()) => return true,
            Err(flume::SendTimeoutError::Timeout(back)) => {
                if !control.is_empty() {
                    return true;
                }
                item = back;
            }
            Err(flume::SendTimeoutError::Disconnected(_)) => return false,
        }
    }
}

pub(crate) enum Source {
    File(Decoder),
    Stream(StreamingSource),
}

impl Source {
    pub(crate) fn params(&self) -> StreamParams {
        match self {
            Source::File(decoder) => decoder.params(),
            Source::Stream(stream) => stream.params(),
        }
    }

    pub(crate) fn duration(&self) -> Option<Duration> {
        match self {
            Source::File(decoder) => decoder.duration(),
            Source::Stream(stream) => stream.duration(),
        }
    }

    pub(crate) fn seek(&mut self, position: f32) -> Result<(), String> {
        match self {
            Source::File(decoder) => decoder
                .seek(position)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Source::Stream(stream) => stream.seek(position),
        }
    }

    pub(crate) fn poll(&mut self) -> Poll {
        match self {
            Source::File(decoder) => match decoder.next_buffer() {
                Ok(Some(batch)) => Poll::Ready(batch),
                Ok(None) => Poll::Ended,
                Err(e) => Poll::Failed(e.to_string()),
            },
            Source::Stream(stream) => stream.poll(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Instant;

    use audio_common::AudioSamples;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        PathBuf::from(root).join("../../fixtures").join(name)
    }

    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        changed: Condvar,
        aborted: AtomicBool,
    }

    impl Gate {
        fn open(&self) {
            *self.open.lock().unwrap() = true;
            self.changed.notify_all();
        }

        fn abort(&self) {
            self.aborted.store(true, Ordering::SeqCst);
            self.changed.notify_all();
        }
    }

    struct GatedStream {
        bytes: std::io::Cursor<Vec<u8>>,
        free: u64,
        gate: Arc<Gate>,
        reads: Arc<AtomicUsize>,
    }

    impl GatedStream {
        fn blocked(&self) -> bool {
            let pos = self.bytes.position();
            let len = self.bytes.get_ref().len() as u64;
            pos >= self.free && pos + 4096 < len
        }
    }

    impl Read for GatedStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.blocked() {
                let mut open = self.gate.open.lock().unwrap();
                while !*open {
                    if self.gate.aborted.load(Ordering::SeqCst) {
                        return Err(std::io::Error::other("aborted"));
                    }
                    open = self
                        .gate
                        .changed
                        .wait_timeout(open, Duration::from_millis(20))
                        .unwrap()
                        .0;
                }
            }
            let limit = if *self.gate.open.lock().unwrap() {
                buf.len()
            } else {
                buf.len()
                    .min(self.free.saturating_sub(self.bytes.position()) as usize)
            };
            let limit = if limit == 0 { buf.len() } else { limit };
            self.bytes.read(&mut buf[..limit])
        }
    }

    impl Seek for GatedStream {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.bytes.seek(pos)
        }
    }

    impl MediaStream for GatedStream {
        fn byte_len(&self) -> Option<u64> {
            Some(self.bytes.get_ref().len() as u64)
        }
    }

    fn long_wav(seconds: u32) -> Vec<u8> {
        let rate = 44_100u32;
        let data_len = seconds * rate * 4;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..seconds * rate * 2 {
            bytes.extend_from_slice(&((i % 2000) as i16 - 1000).to_le_bytes());
        }
        bytes
    }

    fn gated(bytes: Vec<u8>, free: u64) -> (Box<dyn MediaStream>, Arc<Gate>, Arc<AtomicUsize>) {
        let gate = Arc::new(Gate::default());
        let reads = Arc::new(AtomicUsize::new(0));
        let stream = GatedStream {
            bytes: std::io::Cursor::new(bytes),
            free,
            gate: Arc::clone(&gate),
            reads: Arc::clone(&reads),
        };
        (Box::new(stream), gate, reads)
    }

    fn next(source: &mut StreamingSource) -> Poll {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match source.poll() {
                Poll::Pending if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                other => return other,
            }
        }
    }

    fn samples(batch: AudioBatch) -> Vec<i16> {
        match batch.data {
            AudioSamples::S16(samples) => samples,
            _ => panic!("expected S16"),
        }
    }

    #[test]
    fn the_stream_decodes_the_same_samples_as_the_file() {
        let name = "sine_440_16_44_stereo.wav";
        let (stream, gate, _) = gated(std::fs::read(fixture(name)).unwrap(), u64::MAX);
        gate.open();
        let mut source =
            StreamingSource::open(stream, Some("wav".into()), Box::new(|| {})).unwrap();
        let mut file = Decoder::open(&fixture(name)).unwrap();
        assert_eq!(source.params(), file.params());
        assert_eq!(source.duration(), file.duration());
        let mut from_file = Vec::new();
        while let Some(batch) = file.next_buffer().unwrap() {
            from_file.extend(samples(batch));
        }
        let mut streamed = Vec::new();
        loop {
            match next(&mut source) {
                Poll::Ready(batch) => streamed.extend(samples(batch)),
                Poll::Ended => break,
                Poll::Pending => panic!("stalled"),
                Poll::Failed(e) => panic!("{e}"),
            }
        }
        assert_eq!(streamed, from_file);
    }

    #[test]
    fn a_starved_stream_reports_pending_instead_of_blocking() {
        let (stream, gate, _) = gated(long_wav(30), 256 * 1024);
        let aborter = Arc::clone(&gate);
        let mut source = StreamingSource::open(
            stream,
            Some("wav".into()),
            Box::new(move || aborter.abort()),
        )
        .unwrap();
        let began = Instant::now();
        let mut pending = false;
        for _ in 0..50 {
            match source.poll() {
                Poll::Pending => pending = true,
                Poll::Ready(_) => continue,
                _ => break,
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(pending);
        assert!(began.elapsed() < Duration::from_secs(1));
        gate.open();
        assert!(matches!(next(&mut source), Poll::Ready(_)));
    }

    #[test]
    fn a_seek_discards_batches_decoded_before_it() {
        let name = "1khz_16_44_1.wav";
        let (stream, gate, _) = gated(std::fs::read(fixture(name)).unwrap(), u64::MAX);
        gate.open();
        let mut source =
            StreamingSource::open(stream, Some("wav".into()), Box::new(|| {})).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        source.seek(0.5).unwrap();
        let mut file = Decoder::open(&fixture(name)).unwrap();
        file.seek(0.5).unwrap();
        let Poll::Ready(first) = next(&mut source) else {
            panic!("no batch after seek");
        };
        assert_eq!(
            samples(first),
            samples(file.next_buffer().unwrap().unwrap())
        );
    }

    #[test]
    fn dropping_the_source_interrupts_a_blocked_read() {
        let (stream, gate, reads) = gated(long_wav(30), 256 * 1024);
        let aborter = Arc::clone(&gate);
        let source = StreamingSource::open(
            stream,
            Some("wav".into()),
            Box::new(move || aborter.abort()),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        drop(source);
        assert!(gate.aborted.load(Ordering::SeqCst));
        let seen = reads.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(reads.load(Ordering::SeqCst), seen);
    }

    #[test]
    fn a_stream_that_is_not_audio_fails_to_open() {
        let stream = GatedStream {
            bytes: std::io::Cursor::new(vec![7u8; 4096]),
            free: u64::MAX,
            gate: Arc::new(Gate::default()),
            reads: Arc::new(AtomicUsize::new(0)),
        };
        assert!(StreamingSource::open(Box::new(stream), None, Box::new(|| {})).is_err());
    }
}
