//! Stills of a switcher's OBS output for the Live page's preview.
//!
//! Every frame comes from the streamer's OBS computer, over its upload, so a
//! frame fetched in the last [`REUSE`] with the same size and quality is handed
//! to anyone else watching instead of asking OBS again.
//!
//! While the switcher isn't streaming OBS can show what the streamer keeps to
//! themselves (a webcam at home), so then only those the switcher's
//! [`PreviewOffAir`] names get frames ([`allowed_off_air`]). Who is watching is kept
//! for a few seconds, by every API replica ([`Watchers`]), so the Live page can show it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::body::Bytes;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::shutdown::ShutdownHandler;
use db::switchers::PreviewOffAir;
use futures_util::StreamExt;
use live::PreviewWatching;
use uuid::Uuid;

/// A frame this fresh is shared between viewers.
pub const REUSE: Duration = Duration::from_millis(250);
/// Frames older than this are dropped from the cache.
const KEEP: Duration = Duration::from_secs(5);
/// Someone who fetched a frame this recently is watching. The slowest rate
/// asks every 2 seconds, and a hidden tab stops asking.
const WATCHING: Duration = Duration::from_secs(10);
/// A watcher is announced to the other replicas again this often while they
/// keep fetching frames: well within [`WATCHING`], even at the slowest rate.
const ANNOUNCE: Duration = Duration::from_secs(3);
/// How long to wait before subscribing again when following watchers fails.
const RESUBSCRIBE: Duration = Duration::from_secs(5);

/// Who asks for a frame, for [`allowed_off_air`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    /// The workspace's owner, or the organization's team for a client workspace.
    Owner,
    /// A member with the admin role.
    Admin,
    /// A member with the operator role.
    Operator,
    /// NOALBS support.
    Support,
}

/// Whether `viewer` gets frames while the switcher isn't streaming (while it
/// streams everyone does: it's what viewers see). Never support.
pub fn allowed_off_air(off_air: PreviewOffAir, viewer: Viewer) -> bool {
    match (off_air, viewer) {
        (_, Viewer::Support) | (PreviewOffAir::Nobody, _) => false,
        (PreviewOffAir::Owners, viewer) => viewer == Viewer::Owner,
        (PreviewOffAir::Admins, viewer) => matches!(viewer, Viewer::Owner | Viewer::Admin),
        (PreviewOffAir::Everyone, _) => true,
    }
}

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

/// Who fetched a switcher's frames in the last [`WATCHING`], by user, with the
/// name the Live page shows.
///
/// Each API replica serves some of the frames, so whoever serves one announces
/// the watcher over NATS ([`announce`], at most every [`ANNOUNCE`]) and every
/// replica keeps the whole list from those announcements ([`spawn_follow`]).
#[derive(Default)]
pub struct Watchers(Mutex<HashMap<Uuid, HashMap<Uuid, (Instant, String)>>>);

/// What [`Watchers::due`] says to do for a frame someone fetched.
#[derive(Debug, PartialEq, Eq)]
pub enum Announce {
    /// Announced in the last [`ANNOUNCE`]: nothing to do.
    Recently,
    /// Still watching: announce them again, by this name.
    Again(String),
    /// Not watching yet: look up their name and announce them.
    New,
}

impl Watchers {
    pub fn due(&self, switcher_id: Uuid, user_id: Uuid) -> Announce {
        let watchers = self.0.lock().expect("preview watchers");
        match watchers
            .get(&switcher_id)
            .and_then(|users| users.get(&user_id))
        {
            Some((at, _)) if at.elapsed() < ANNOUNCE => Announce::Recently,
            Some((at, name)) if at.elapsed() < WATCHING => Announce::Again(name.clone()),
            _ => Announce::New,
        }
    }

    pub fn add(&self, switcher_id: Uuid, user_id: Uuid, name: String) {
        let mut watchers = self.0.lock().expect("preview watchers");
        watchers.retain(|_, users| {
            users.retain(|_, (at, _)| at.elapsed() < WATCHING);
            !users.is_empty()
        });
        watchers
            .entry(switcher_id)
            .or_default()
            .insert(user_id, (Instant::now(), name));
    }

    /// Who watches now, by name.
    pub fn of(&self, switcher_id: Uuid) -> Vec<(Uuid, String)> {
        let watchers = self.0.lock().expect("preview watchers");
        let mut list: Vec<_> = watchers
            .get(&switcher_id)
            .into_iter()
            .flatten()
            .filter(|(_, (at, _))| at.elapsed() < WATCHING)
            .map(|(user_id, (_, name))| (*user_id, name.clone()))
            .collect();
        list.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        list
    }
}

/// Tells every replica (this one at once, so its own list doesn't wait on NATS)
/// that `watching.user_id` watches.
pub async fn announce(nats: &async_nats::Client, watchers: &Watchers, watching: PreviewWatching) {
    watchers.add(
        watching.switcher_id,
        watching.user_id,
        watching.name.clone(),
    );
    let payload = match serde_json::to_vec(&watching) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(%error, "Couldn't serialize a preview watcher");
            return;
        }
    };
    let subject = live::preview_watching_subject(watching.switcher_id);
    if let Err(error) = nats.publish(subject, payload.into()).await {
        tracing::warn!(%error, "Couldn't announce a preview watcher");
    }
}

/// Keeps `watchers` up to date with every replica's [`announce`]ments until
/// shutdown, subscribing again if the subscription ends.
pub fn spawn_follow(
    nats: async_nats::Client,
    watchers: Arc<Watchers>,
    mut shutdown: ShutdownHandler,
) {
    tokio::spawn(async move {
        loop {
            match nats.subscribe(live::PREVIEW_WATCHING_SUBJECTS).await {
                Ok(mut messages) => loop {
                    tokio::select! {
                        () = shutdown.changed() => return,
                        message = messages.next() => match message {
                            Some(message) => {
                                match serde_json::from_slice::<PreviewWatching>(&message.payload) {
                                    Ok(watching) => watchers.add(
                                        watching.switcher_id,
                                        watching.user_id,
                                        watching.name,
                                    ),
                                    Err(error) => {
                                        tracing::warn!(%error, "Couldn't read a preview watcher");
                                    }
                                }
                            }
                            None => break,
                        },
                    }
                },
                Err(error) => tracing::warn!(%error, "Couldn't follow preview watchers"),
            }
            tokio::select! {
                () = shutdown.changed() => return,
                () = tokio::time::sleep(RESUBSCRIBE) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use uuid::Uuid;

    use db::switchers::PreviewOffAir;

    use super::{Announce, Cache, Viewer, Watchers, allowed_off_air, decode_data_uri, settings};

    #[test]
    fn off_air_follows_the_setting_and_never_shows_support() {
        let off_air = |setting| {
            [
                Viewer::Owner,
                Viewer::Admin,
                Viewer::Operator,
                Viewer::Support,
            ]
            .map(|viewer| allowed_off_air(setting, viewer))
        };
        assert_eq!(off_air(PreviewOffAir::Nobody), [false; 4]);
        assert_eq!(off_air(PreviewOffAir::Owners), [true, false, false, false]);
        assert_eq!(off_air(PreviewOffAir::Admins), [true, true, false, false]);
        assert_eq!(off_air(PreviewOffAir::Everyone), [true, true, true, false]);
    }

    #[test]
    fn watchers_are_listed_per_switcher_by_name() {
        let watchers = Watchers::default();
        let (switcher, sam, alex) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(watchers.due(switcher, sam), Announce::New);
        watchers.add(switcher, sam, "sam".to_owned());
        watchers.add(switcher, alex, "Alex".to_owned());
        assert_eq!(watchers.due(switcher, sam), Announce::Recently);
        assert_eq!(watchers.due(Uuid::new_v4(), sam), Announce::New);
        assert_eq!(
            watchers.of(switcher),
            [(alex, "Alex".to_owned()), (sam, "sam".to_owned())]
        );
        assert!(watchers.of(Uuid::new_v4()).is_empty());
    }

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
