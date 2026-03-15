/// Handle to an enabled non-control endpoint, returned by `ep_enable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpHandle(pub(crate) u32);
