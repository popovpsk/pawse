mod windows;
pub(crate) use windows::{read, write};

const SERVICE: &str = "pawse";
const ACCOUNT: &str = "data-key";
