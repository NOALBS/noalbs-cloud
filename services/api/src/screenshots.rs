//! Stills of a switcher's OBS output for the Live page's preview.
//!
//! Every frame comes from the streamer's OBS computer, over its upload, so a
//! frame fetched in the last [`REUSE`] with the same size and quality is handed
//! to anyone else watching instead of asking OBS again.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::body::Bytes;
use base64::{Engine, engine::general_purpose::STANDARD};
use uuid::Uuid;

/// A frame this fresh is shared between viewers.
pub const REUSE: Duration = Duration::from_millis(250);
/// Frames older than this are dropped from the cache.
const KEEP: Duration = Duration::from_secs(5);

pub const MIN_WIDTH: u32 = 160;
/// Far below NATS's 1 MB message limit even at high quality.
pub const MAX_WIDTH: u32 = 1280;
pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_QUALITY: u8 = 60;

/// The requested width and JPEG quality, within bounds.
pub fn settings(width: Option<u32>, quality: Option<u8>) -> (u32, u8) {
    (
        width.unwrap_or(DEFAULT_WIDTH).clamp(MIN_WIDTH, MAX_WIDTH),
        quality.unwrap_or(DEFAULT_QUALITY).clamp(1, 100),
    )
}

/// The image in a `data:image/…;base64,…` URI.
pub fn decode_data_uri(uri: &str) -> Option<Vec<u8>> {
    let (header, data) = uri.split_once(',')?;
    if !header.starts_with("data:image/") || !header.ends_with(";base64") {
        return None;
    }
    STANDARD.decode(data).ok()
}

type Key = (Uuid, u32, u8);

#[derive(Default)]
pub struct Cache(Mutex<HashMap<Key, (Instant, Bytes)>>);

impl Cache {
    pub fn fresh(&self, key: Key) -> Option<Bytes> {
        let frames = self.0.lock().expect("screenshot cache");
        frames
            .get(&key)
            .filter(|(at, _)| at.elapsed() < REUSE)
            .map(|(_, frame)| frame.clone())
    }

    pub fn store(&self, key: Key, frame: Bytes) {
        let mut frames = self.0.lock().expect("screenshot cache");
        frames.retain(|_, (at, _)| at.elapsed() < KEEP);
        frames.insert(key, (Instant::now(), frame));
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use uuid::Uuid;

    use super::{Cache, decode_data_uri, settings};

    #[test]
    fn sizes_and_qualities_stay_in_bounds() {
        assert_eq!(settings(None, None), (640, 60));
        assert_eq!(settings(Some(4096), Some(0)), (1280, 1));
        assert_eq!(settings(Some(10), Some(250)), (160, 100));
    }

    #[test]
    fn data_uris_decode_to_the_image() {
        assert_eq!(
            decode_data_uri("data:image/jpg;base64,/9j/4A=="),
            Some(vec![0xff, 0xd8, 0xff, 0xe0])
        );
        assert_eq!(decode_data_uri("data:text/plain;base64,aGk="), None);
        assert_eq!(decode_data_uri("data:image/png,raw"), None);
        assert_eq!(decode_data_uri("no comma"), None);
    }

    #[test]
    fn fresh_frames_are_shared_per_size_and_quality() {
        let cache = Cache::default();
        let switcher = Uuid::new_v4();
        cache.store((switcher, 640, 60), Bytes::from_static(b"frame"));
        assert_eq!(
            cache.fresh((switcher, 640, 60)),
            Some(Bytes::from_static(b"frame"))
        );
        assert_eq!(cache.fresh((switcher, 320, 60)), None);
        assert_eq!(cache.fresh((Uuid::new_v4(), 640, 60)), None);
    }
}
