use audio_embedding::{BANDS, DIM, PATCH, STYLES};

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
pub(crate) enum Net {
    Multi,
    Styles,
}

pub(crate) const NETS: [Net; 2] = [Net::Multi, Net::Styles];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Layer(Net, Layer),
    ClassifierWeight,
    ClassifierBias,
    ProbeRows,
    ProbeStyles,
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
            Role::Layer(
                _,
                Layer::StemWeight
                    | Layer::ExpandWeight(_)
                    | Layer::DepthwiseWeight(_)
                    | Layer::SqueezeWeight(_)
                    | Layer::ExciteWeight(_)
                    | Layer::ProjectWeight(_)
                    | Layer::HeadWeight
            ) | Role::ClassifierWeight
        )
    }

    pub(crate) fn transposed(&self) -> bool {
        matches!(
            self.role,
            Role::Layer(
                _,
                Layer::ExpandWeight(_)
                    | Layer::SqueezeWeight(_)
                    | Layer::ExciteWeight(_)
                    | Layer::ProjectWeight(_)
                    | Layer::HeadWeight
            ) | Role::ClassifierWeight
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

fn network(net: Net, list: &mut Vec<Tensor>) {
    let role = |layer| Role::Layer(net, layer);
    list.push(conv(role(Layer::StemWeight), STEM, 1, STEM_KERNEL));
    list.push(bias(role(Layer::StemBias), STEM));
    for (ix, b) in BLOCKS.iter().enumerate() {
        if b.expands() {
            list.push(conv(role(Layer::ExpandWeight(ix)), b.expanded, b.input, 1));
            list.push(bias(role(Layer::ExpandBias(ix)), b.expanded));
        }
        list.push(conv(
            role(Layer::DepthwiseWeight(ix)),
            b.expanded,
            1,
            b.kernel,
        ));
        list.push(bias(role(Layer::DepthwiseBias(ix)), b.expanded));
        list.push(conv(
            role(Layer::SqueezeWeight(ix)),
            b.squeeze,
            b.expanded,
            1,
        ));
        list.push(bias(role(Layer::SqueezeBias(ix)), b.squeeze));
        list.push(conv(
            role(Layer::ExciteWeight(ix)),
            b.expanded,
            b.squeeze,
            1,
        ));
        list.push(bias(role(Layer::ExciteBias(ix)), b.expanded));
        list.push(conv(
            role(Layer::ProjectWeight(ix)),
            b.output,
            b.expanded,
            1,
        ));
        list.push(bias(role(Layer::ProjectBias(ix)), b.output));
    }
    list.push(conv(role(Layer::HeadWeight), DIM, HEAD_INPUT, 1));
    list.push(bias(role(Layer::HeadBias), DIM));
}

pub(crate) fn tensors() -> Vec<Tensor> {
    let mut list = Vec::new();
    for net in NETS {
        network(net, &mut list);
    }
    list.push(conv(Role::ClassifierWeight, STYLES, DIM, 1));
    list.push(bias(Role::ClassifierBias, STYLES));
    list.push(Tensor {
        role: Role::ProbeRows,
        onnx_shape: [BATCH, DIM, 1, 1],
    });
    list.push(Tensor {
        role: Role::ProbeStyles,
        onnx_shape: [BATCH, STYLES, 1, 1],
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_network_has_the_onnx_graph_shape() {
        let all = tensors();
        assert_eq!(all.iter().filter(|t| t.is_weight()).count(), 81 * 2 + 1);
        assert_eq!(all.len(), 81 * 2 * 2 + 2 + 2);
        let multi = all
            .iter()
            .filter(|t| matches!(t.role, Role::Layer(Net::Multi, _)));
        let styles = all
            .iter()
            .filter(|t| matches!(t.role, Role::Layer(Net::Styles, _)));
        assert!(multi.map(|t| t.onnx_shape).eq(styles.map(|t| t.onnx_shape)));
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
