mod mask;
mod mix;
pub mod plane;
mod pyramid;

#[cfg(test)]
mod tests;

pub use mask::{Coverage, Mask, Stamp};
pub use mix::{Mix, mix};
pub use pyramid::{Level, Pyramid, bake, level_factor};

pub const LEVEL_SIGMAS: [f32; 4] = [1., 2., 4., 8.];
pub const LEVELS: usize = LEVEL_SIGMAS.len();
pub const MAX_SIGMA: f32 = LEVEL_SIGMAS[LEVELS - 1];
