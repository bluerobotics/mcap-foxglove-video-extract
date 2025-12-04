use std::io::Cursor;

use cdr::LittleEndian;
use cdr::de::Deserializer;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Timestamp {
    pub sec: u32,
    pub nsec: u32,
}

impl Timestamp {
    pub fn as_nanos(&self) -> u64 {
        (self.sec as u64) * 1_000_000_000 + (self.nsec as u64)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompressedVideo {
    pub timestamp: Timestamp,
    #[allow(dead_code)]
    pub frame_id: String,
    pub data: Vec<u8>,
    #[allow(dead_code)]
    pub format: String,
}

pub fn decode_compressed_video(data: &[u8]) -> cdr::Result<CompressedVideo> {
    cdr::deserialize::<CompressedVideo>(data).or_else(|_err| {
        // Fallback for payloads without encapsulation header: assume little-endian.
        let mut de =
            Deserializer::<_, cdr::Infinite, LittleEndian>::new(Cursor::new(data), cdr::Infinite);
        serde::Deserialize::deserialize(&mut de)
    })
}
