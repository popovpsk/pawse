use audio_embedding::{BANDS, DIM, PATCH};

pub(crate) const BATCH: usize = 64;
pub(crate) const STEM: usize = 32;
pub(crate) const STEM_KERNEL: usize = 3;
pub(crate) const HEAD_INPUT: usize = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) kernel: usize,
    pub(crate) stride: usize,
    pub(crate) input: usize,
    pub(crate) output: usize,
    pub(crate) expanded: usize,
    pub(crate) squeeze: usize,
}

const fn block(
    kernel: usize,
    stride: usize,
    input: usize,
    output: usize,
    expand: usize,
    squeeze: usize,
) -> Block {
    Block {
        kernel,
        stride,
        input,
        output,
        expanded: input * expand,
        squeeze,
    }
}

pub(crate) const BLOCKS: [Block; 16] = [
    block(3, 1, 32, 16, 1, 8),
    block(3, 2, 16, 24, 6, 4),
    block(3, 1, 24, 24, 6, 6),
    block(5, 2, 24, 40, 6, 6),
    block(5, 1, 40, 40, 6, 10),
    block(3, 2, 40, 80, 6, 10),
    block(3, 1, 80, 80, 6, 20),
    block(3, 1, 80, 80, 6, 20),
    block(5, 1, 80, 112, 6, 20),
    block(5, 1, 112, 112, 6, 28),
    block(5, 1, 112, 112, 6, 28),
    block(5, 2, 112, 192, 6, 28),
    block(5, 1, 192, 192, 6, 48),
    block(5, 1, 192, 192, 6, 48),
    block(5, 1, 192, 192, 6, 48),
    block(3, 1, 192, 320, 6, 48),
];

impl Block {
    pub(crate) fn expands(&self) -> bool {
        self.expanded != self.input
    }

    pub(crate) fn residual(&self) -> bool {
        self.stride == 1 && self.input == self.output
    }
}

pub(crate) fn conv_out(size: usize, kernel: usize, stride: usize) -> usize {
    (size + 2 * (kernel / 2) - kernel) / stride + 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plane {
    pub(crate) h: usize,
    pub(crate) w: usize,
}

impl Plane {
    pub(crate) fn area(self) -> usize {
        self.h * self.w
    }

    fn after(self, kernel: usize, stride: usize) -> Plane {
        Plane {
            h: conv_out(self.h, kernel, stride),
            w: conv_out(self.w, kernel, stride),
        }
    }
}

pub(crate) const INPUT: Plane = Plane { h: PATCH, w: BANDS };

pub(crate) fn stem_plane() -> Plane {
    INPUT.after(STEM_KERNEL, 2)
}

pub(crate) fn planes() -> Vec<(Plane, Plane)> {
    let mut plane = stem_plane();
    BLOCKS
        .iter()
        .map(|b| {
            let out = plane.after(b.kernel, b.stride);
            let pair = (plane, out);
            plane = out;
            pair
        })
        .collect()
}

pub(crate) fn head_plane() -> Plane {
    planes().last().map_or(stem_plane(), |p| p.1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    StemWeight,
    StemBias,
    ExpandWeight(usize),
    ExpandBias(usize),
    DepthwiseWeight(usize),
    DepthwiseBias(usize),
    SqueezeWeight(usize),
    SqueezeBias(usize),
    ExciteWeight(usize),
    ExciteBias(usize),
    ProjectWeight(usize),
    ProjectBias(usize),
    HeadWeight,
    HeadBias,
    ProbeRows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tensor {
    pub(crate) role: Role,
    pub(crate) onnx_shape: [usize; 4],
}

impl Tensor {
    pub(crate) fn len(&self) -> usize {
        self.onnx_shape.iter().product()
    }

    #[cfg(test)]
    pub(crate) fn is_weight(&self) -> bool {
        matches!(
            self.role,
            Role::StemWeight
                | Role::ExpandWeight(_)
                | Role::DepthwiseWeight(_)
                | Role::SqueezeWeight(_)
                | Role::ExciteWeight(_)
                | Role::ProjectWeight(_)
                | Role::HeadWeight
        )
    }

    pub(crate) fn transposed(&self) -> bool {
        matches!(
            self.role,
            Role::ExpandWeight(_)
                | Role::SqueezeWeight(_)
                | Role::ExciteWeight(_)
                | Role::ProjectWeight(_)
                | Role::HeadWeight
        )
    }
}

fn conv(role: Role, out: usize, input: usize, kernel: usize) -> Tensor {
    Tensor {
        role,
        onnx_shape: [out, input, kernel, kernel],
    }
}

fn bias(role: Role, len: usize) -> Tensor {
    Tensor {
        role,
        onnx_shape: [len, 1, 1, 1],
    }
}

pub(crate) fn tensors() -> Vec<Tensor> {
    let mut list = vec![
        conv(Role::StemWeight, STEM, 1, STEM_KERNEL),
        bias(Role::StemBias, STEM),
    ];
    for (ix, b) in BLOCKS.iter().enumerate() {
        if b.expands() {
            list.push(conv(Role::ExpandWeight(ix), b.expanded, b.input, 1));
            list.push(bias(Role::ExpandBias(ix), b.expanded));
        }
        list.push(conv(Role::DepthwiseWeight(ix), b.expanded, 1, b.kernel));
        list.push(bias(Role::DepthwiseBias(ix), b.expanded));
        list.push(conv(Role::SqueezeWeight(ix), b.squeeze, b.expanded, 1));
        list.push(bias(Role::SqueezeBias(ix), b.squeeze));
        list.push(conv(Role::ExciteWeight(ix), b.expanded, b.squeeze, 1));
        list.push(bias(Role::ExciteBias(ix), b.expanded));
        list.push(conv(Role::ProjectWeight(ix), b.output, b.expanded, 1));
        list.push(bias(Role::ProjectBias(ix), b.output));
    }
    list.push(conv(Role::HeadWeight, DIM, HEAD_INPUT, 1));
    list.push(bias(Role::HeadBias, DIM));
    list.push(Tensor {
        role: Role::ProbeRows,
        onnx_shape: [BATCH, DIM, 1, 1],
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_has_the_onnx_graph_shape() {
        let all = tensors();
        assert_eq!(all.iter().filter(|t| t.is_weight()).count(), 81);
        assert_eq!(all.len(), 81 * 2 + 1);
        assert_eq!(stem_plane(), Plane { h: 64, w: 48 });
        assert_eq!(head_plane(), Plane { h: 4, w: 3 });
        assert_eq!(BLOCKS.last().unwrap().output, HEAD_INPUT);
        let residuals = BLOCKS.iter().filter(|b| b.residual()).count();
        assert_eq!(residuals, 9);
        let sizes: Vec<(usize, usize)> = planes().iter().map(|(_, o)| (o.h, o.w)).collect();
        assert_eq!(sizes[1], (32, 24));
        assert_eq!(sizes[3], (16, 12));
        assert_eq!(sizes[5], (8, 6));
        assert_eq!(sizes[11], (4, 3));
    }

    #[test]
    fn blocks_chain_their_channels() {
        let mut channels = STEM;
        for b in BLOCKS {
            assert_eq!(b.input, channels);
            channels = b.output;
        }
    }
}
