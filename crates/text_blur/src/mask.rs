use crate::plane::Plane;

#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub left: i32,
    pub top: i32,
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct Stamp<'a> {
    pub x: i32,
    pub y: i32,
    pub coverage: &'a Coverage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub plane: Plane,
    pub left: i32,
    pub top: i32,
}

impl Coverage {
    fn is_drawn(&self) -> bool {
        self.width > 0 && self.height > 0 && self.alpha.len() == self.width * self.height
    }
}

impl Mask {
    pub fn compose(stamps: &[Stamp], align: usize) -> Option<Mask> {
        let align = align.max(1) as i32;
        let drawn = || stamps.iter().filter(|stamp| stamp.coverage.is_drawn());
        let (mut left, mut top) = (i32::MAX, i32::MAX);
        let (mut right, mut bottom) = (i32::MIN, i32::MIN);
        for stamp in drawn() {
            let x = stamp.x + stamp.coverage.left;
            let y = stamp.y + stamp.coverage.top;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + stamp.coverage.width as i32);
            bottom = bottom.max(y + stamp.coverage.height as i32);
        }
        if left >= right || top >= bottom {
            return None;
        }
        let left = left.div_euclid(align) * align;
        let top = top.div_euclid(align) * align;
        let right = (right + align - 1).div_euclid(align) * align;
        let bottom = (bottom + align - 1).div_euclid(align) * align;

        let mut plane = Plane::new((right - left) as usize, (bottom - top) as usize);
        for stamp in drawn() {
            let coverage = stamp.coverage;
            let x = (stamp.x + coverage.left - left) as usize;
            let y = (stamp.y + coverage.top - top) as usize;
            for (row, src) in coverage.alpha.chunks_exact(coverage.width).enumerate() {
                let start = (y + row) * plane.width + x;
                for (cell, &a) in plane.data[start..start + coverage.width]
                    .iter_mut()
                    .zip(src)
                {
                    let a = a as f32 / 255.;
                    *cell += a * (1. - *cell);
                }
            }
        }
        Some(Mask { plane, left, top })
    }
}
