mod gain;
mod measure;
mod meter;
mod stage;

pub use gain::{HEADROOM_DB, REFERENCE_LUFS, applied_db, factor};
pub use measure::{Estimate, MIN_WINDOWS, Span, WINDOW, WINDOWS, full, quick};
pub use meter::{Measurement, Meter};
pub use stage::{GainStage, Switch};

#[cfg(test)]
mod tests;
