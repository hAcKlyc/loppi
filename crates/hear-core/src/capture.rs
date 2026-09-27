use std::time::{Instant, SystemTime};

/// The callback's arrival anchors both clock domains; the sample duration is
/// subtracted when calculating the beginning of this packet.
pub struct AudioPacket {
    pub at: Instant,
    pub wall_at: SystemTime,
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: usize,
}
