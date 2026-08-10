const HTAPS: usize = 48;
const FIFO_SIZE: usize = 16;
const FIFO_MASK: usize = FIFO_SIZE - 1;
const CTABLES: usize = HTAPS.div_ceil(8);

const DSD_SILENCE_MSBF: u8 = 0x69;
const DSD_SILENCE_LSBF: u8 = 0x96;

const HALF_TAPS: [f64; HTAPS] = [
    0.09950731974056658,
    0.09562845727714668,
    0.08819647126516944,
    0.07782552527068175,
    0.06534876523171299,
    0.05172629311427257,
    0.0379429484910187,
    0.02490921351762261,
    0.0133774746265897,
    0.003883043418804416,
    -0.003284703416210726,
    -0.008080250212687497,
    -0.01067241812471033,
    -0.01139427235000863,
    -0.0106813877974587,
    -0.009_007_905_078_766_05,
    -0.006828859761015335,
    -0.004535184322001496,
    -0.002425035959059578,
    -0.0006922187080790708,
    0.0005700762133516592,
    0.001353838005269448,
    0.001713709169690937,
    0.001742046839472948,
    0.001545601648013235,
    0.001226696225277855,
    0.0008704322683580222,
    0.000_538_163_620_053_565,
    0.000266446345425276,
    7.002968738383528e-05,
    -5.279407053811266e-05,
    -0.0001140625650874684,
    -0.0001304796361231895,
    -0.0001189970287491285,
    -9.396247155265073e-05,
    -6.577634378272832e-05,
    -4.07492895872535e-05,
    -2.17407957554587e-05,
    -9.163058931391722e-06,
    -2.017460145032201e-06,
    1.249721855219005e-06,
    2.166655190537392e-06,
    1.930520892991082e-06,
    1.319400334374195e-06,
    7.410039764949091e-07,
    3.423230509967409e-07,
    1.244182214744588e-07,
    3.130441005359396e-08,
];

pub struct DsdTables {
    msbf: Vec<[f64; 256]>,
    lsbf: Vec<[f64; 256]>,
}

impl DsdTables {
    pub fn new() -> Self {
        let mut msbf = vec![[0f64; 256]; CTABLES];
        let mut lsbf = vec![[0f64; 256]; CTABLES];

        for e in 0u32..256 {
            let mut acc = [0f64; CTABLES];
            for m in 0..8usize {
                let bit = (e >> (7 - m)) & 1;
                let sign = if bit == 1 { 1.0 } else { -1.0 };
                for t in 0..CTABLES {
                    acc[t] += sign * HALF_TAPS[t * 8 + m];
                }
            }
            let byte = e as u8;
            let reversed = byte.reverse_bits();
            for t in 0..CTABLES {
                msbf[CTABLES - 1 - t][byte as usize] = acc[t];
                lsbf[CTABLES - 1 - t][reversed as usize] = acc[t];
            }
        }

        Self { msbf, lsbf }
    }
}

impl Default for DsdTables {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ChannelState {
    buf: [u8; FIFO_SIZE],
    pos: usize,
}

impl ChannelState {
    pub fn new(lsbf: bool) -> Self {
        let silence = if lsbf {
            DSD_SILENCE_LSBF
        } else {
            DSD_SILENCE_MSBF
        };
        Self {
            buf: [silence; FIFO_SIZE],
            pos: 0,
        }
    }

    pub fn reset(&mut self, lsbf: bool) {
        *self = Self::new(lsbf);
    }
}

pub fn translate(
    state: &mut ChannelState,
    tables: &DsdTables,
    lsbf: bool,
    input: &[u8],
    output: &mut [f32],
) {
    assert_eq!(input.len(), output.len());

    let ctables: &[[f64; 256]] = if lsbf { &tables.lsbf } else { &tables.msbf };
    let mut buf = state.buf;
    let mut pos = state.pos;

    for (byte, out) in input.iter().zip(output.iter_mut()) {
        buf[pos] = *byte;

        let mid = pos.wrapping_sub(CTABLES) & FIFO_MASK;
        buf[mid] = buf[mid].reverse_bits();

        let mut sum = 0.0f64;
        for k in 0..CTABLES {
            let a = buf[pos.wrapping_sub(k) & FIFO_MASK];
            let b = buf[pos.wrapping_sub(CTABLES * 2 - 1).wrapping_add(k) & FIFO_MASK];
            sum += ctables[k][a as usize] + ctables[k][b as usize];
        }

        *out = sum as f32;
        pos = (pos + 1) & FIFO_MASK;
    }

    state.buf = buf;
    state.pos = pos;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_pattern_stays_near_zero() {
        let tables = DsdTables::new();
        let mut state = ChannelState::new(false);
        let input = vec![DSD_SILENCE_MSBF; FIFO_SIZE * 4];
        let mut output = vec![0f32; input.len()];
        translate(&mut state, &tables, false, &input, &mut output);
        for &s in &output[FIFO_SIZE..] {
            assert!(s.abs() < 1e-3, "expected near-silence, got {s}");
        }
    }

    #[test]
    fn constant_positive_bytes_settle_to_dc_gain() {
        let tables = DsdTables::new();
        let mut state = ChannelState::new(false);
        let input = vec![0xFFu8; FIFO_SIZE * 4];
        let mut output = vec![0f32; input.len()];
        translate(&mut state, &tables, false, &input, &mut output);

        let expected: f64 = 2.0 * HALF_TAPS.iter().sum::<f64>();
        let tail = *output.last().unwrap();
        assert!(
            (tail as f64 - expected).abs() < 1e-3,
            "expected ~{expected}, got {tail}"
        );
    }

    #[test]
    fn constant_negative_bytes_settle_to_negative_dc_gain() {
        let tables = DsdTables::new();
        let mut state = ChannelState::new(false);
        let input = vec![0x00u8; FIFO_SIZE * 4];
        let mut output = vec![0f32; input.len()];
        translate(&mut state, &tables, false, &input, &mut output);

        let expected: f64 = -2.0 * HALF_TAPS.iter().sum::<f64>();
        let tail = *output.last().unwrap();
        assert!(
            (tail as f64 - expected).abs() < 1e-3,
            "expected ~{expected}, got {tail}"
        );
    }

    #[test]
    fn nyquist_alternating_pattern_is_attenuated() {
        // 0xAA repeated = a perfectly continuous 1,0,1,0,... bitstream (no
        // byte-boundary phase glitch, unlike alternating 0x55/0xAA), i.e.
        // the DSD-bit-rate Nyquist tone: the filter should crush it hard.
        let tables = DsdTables::new();
        let mut state = ChannelState::new(false);
        let input = vec![0xAAu8; FIFO_SIZE * 4];
        let mut output = vec![0f32; input.len()];
        translate(&mut state, &tables, false, &input, &mut output);
        for &s in &output[FIFO_SIZE..] {
            assert!(s.abs() < 1e-3, "expected strong attenuation, got {s}");
        }
    }

    #[test]
    fn lsbf_is_bit_reversed_msbf() {
        let tables = DsdTables::new();
        let mut msbf_state = ChannelState::new(false);
        let mut lsbf_state = ChannelState::new(true);

        let msbf_input = vec![0b1100_0101u8; FIFO_SIZE * 3];
        let lsbf_input: Vec<u8> = msbf_input.iter().map(|b| b.reverse_bits()).collect();

        let mut msbf_out = vec![0f32; msbf_input.len()];
        let mut lsbf_out = vec![0f32; lsbf_input.len()];
        translate(&mut msbf_state, &tables, false, &msbf_input, &mut msbf_out);
        translate(&mut lsbf_state, &tables, true, &lsbf_input, &mut lsbf_out);

        for (a, b) in msbf_out.iter().zip(lsbf_out.iter()) {
            assert!((a - b).abs() < 1e-6, "{a} vs {b}");
        }
    }

    #[test]
    fn reset_returns_to_fresh_silence_state() {
        let tables = DsdTables::new();
        let mut state = ChannelState::new(false);
        let input = vec![0xFFu8; FIFO_SIZE * 2];
        let mut output = vec![0f32; input.len()];
        translate(&mut state, &tables, false, &input, &mut output);

        state.reset(false);
        let fresh = ChannelState::new(false);
        assert_eq!(state.buf, fresh.buf);
        assert_eq!(state.pos, fresh.pos);
    }
}
