//! Remote control of a switcher's OBS (and BELABOX), shared by everything that acts on it on
//! someone's behalf (the dashboard through the API, chat commands through the
//! chat service), so both follow the same rules:
//!
//! - a switcher paused for being over its plan's limit refuses changes;
//! - OBS must be connected (its status says so) before we ask it anything;
//! - every change is announced first as an [`live::Intent`], so obs-link logs
//!   who did it when OBS reports the result.
//!
//! BELABOX commands go to obs-link's BELABOX link as a request ([`Control::belabox`]), Moblin
//! commands to the connected Moblin ([`Control::moblin`]).

use std::time::Duration;

use async_nats::jetstream::kv;
use live::{
    BelaboxAction, BelaboxCommand, BelaboxDetails, BelaboxDetailsReply, BelaboxReply,
    BelaboxStatus, EventDetail, Intent, IntentAction, MoblinAction, MoblinCommand, MoblinReply,
    MoblinStatus, SwitcherStatus,
};
use obs_actions::{ObsClient, audio::AudioInput, sources::SourceItem};
use serde::de::DeserializeOwned;
use uuid::Uuid;

mod obs_stats;
pub use obs_stats::{BITRATE_WINDOW, Dropped, ObsStats, bitrate_kbps};

/// How long the refresh scene stays on air before switching back.
pub const REFRESH_HOLD: Duration = Duration::from_secs(4);

pub const MAX_BITRATE_KBPS: i32 = 100_000;
pub const MAX_RTT_MS: i32 = 60_000;

/// The longest marker description, in characters (YouTube's chat limit).
pub const MAX_MARKER_CHARS: usize = 200;

/// A fader's range in obs-websocket, in dB (-100 is -inf; OBS's own fader stops at 0).
pub const MIN_VOLUME_DB: f64 = -100.0;
pub const MAX_VOLUME_DB: f64 = 26.0;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the switcher is paused: the workspace has more switchers than its plan includes")]
    Paused,
    #[error("OBS isn't connected")]
    ObsOffline,
    /// OBS answered with an error (`comment` says why, when it does).
    #[error("OBS refused the request (code {code})")]
    ObsRefused { code: u32, comment: Option<String> },
    /// OBS didn't answer, or the request couldn't be sent.
    #[error("OBS didn't answer: {0}")]
    ObsUnreachable(obs_actions::Error),
    #[error("no Refresh scene is picked")]
    NoRefreshScene,
    #[error("the Refresh scene is already on air")]
    AlreadyRefreshing,
    #[error("this OBS version doesn't report scene IDs")]
    NoSceneIds,
    #[error("that source isn't in the scene anymore")]
    SourceNotFound,
    #[error("the volume must be between -100 and 26 dB")]
    InvalidVolume,
    #[error("OBS has no such input with audio (anymore)")]
    InputNotFound,
    #[error("the switcher isn't streaming")]
    NotStreaming,
    #[error("a marker's description is at most {MAX_MARKER_CHARS} characters")]
    InvalidMarker,
    #[error("BELABOX Cloud isn't set up for this switcher")]
    BelaboxNotSetUp,
    /// No BELABOX link runs: the browser source isn't connected (or obs-link
    /// didn't answer).
    #[error("the BELABOX link isn't running: is OBS open with the browser source?")]
    BelaboxUnreachable,
    #[error(transparent)]
    BelaboxRefused(live::BelaboxRefusal),
    #[error("Moblin isn't set up for this switcher")]
    MoblinNotSetUp,
    /// No Moblin is connected (or obs-link didn't answer).
    #[error("Moblin isn't connected")]
    MoblinUnreachable,
    #[error(transparent)]
    MoblinRefused(live::MoblinRefusal),
    #[error(transparent)]
    Db(#[from] db::Error),
}

impl From<obs_actions::Error> for Error {
    fn from(error: obs_actions::Error) -> Self {
        match error {
            obs_actions::Error::ObsResponseError { code, comment, .. } => {
                Self::ObsRefused { code, comment }
            }
            error => Self::ObsUnreachable(error),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What controlling a switcher needs; cheap to clone.
#[derive(Clone)]
pub struct Control {
    pub db: db::DbPool,
    pub nats: async_nats::Client,
    /// The status bucket obs-link writes ([`live::status_bucket`]).
    pub status_kv: kv::Store,
    /// The BELABOX status bucket obs-link writes ([`live::belabox_bucket`]).
    pub belabox_kv: kv::Store,
    /// The Moblin status bucket obs-link writes ([`live::moblin_bucket`]).
    pub moblin_kv: kv::Store,
}

impl Control {
    pub fn new(db: db::DbPool, nats: async_nats::Client, stores: &live::Stores) -> Self {
        Self {
            db,
            nats,
            status_kv: stores.status.clone(),
            belabox_kv: stores.belabox.clone(),
            moblin_kv: stores.moblin.clone(),
        }
    }

    /// Asks obs-link's BELABOX or Moblin link (`what`) something and reads its JSON answer.
    /// `None` when it couldn't be asked or understood; that is logged here.
    async fn ask<R: DeserializeOwned>(
        &self,
        what: &str,
        switcher_id: Uuid,
        subject: String,
        payload: Vec<u8>,
    ) -> Option<R> {
        let response = match telemetry::nats::request(&self.nats, subject, payload).await {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(%error, %switcher_id, "{what} request failed");
                return None;
            }
        };
        match serde_json::from_slice(&response.payload) {
            Ok(answer) => Some(answer),
            Err(error) => {
                tracing::warn!(%error, %switcher_id, "Malformed {what} answer");
                None
            }
        }
    }

    /// The switcher's live status, or `None` if its browser source isn't connected.
    pub async fn status(&self, switcher_id: Uuid) -> Option<SwitcherStatus> {
        match self.status_kv.get(live::status_key(switcher_id)).await {
            Ok(value) => value.and_then(|bytes| serde_json::from_slice(&bytes).ok()),
            Err(error) => {
                tracing::warn!(%error, %switcher_id, "Couldn't read a switcher's status");
                None
            }
        }
    }

    /// The switcher's BELABOX status, or `None` if no BELABOX link runs (no
    /// remote key, or the browser source isn't connected).
    pub async fn belabox_status(&self, switcher_id: Uuid) -> Option<BelaboxStatus> {
        match self.belabox_kv.get(live::status_key(switcher_id)).await {
            Ok(value) => value.and_then(|bytes| serde_json::from_slice(&bytes).ok()),
            Err(error) => {
                tracing::warn!(%error, %switcher_id, "Couldn't read a BELABOX status");
                None
            }
        }
    }

    /// Has obs-link carry out `action` on the switcher's BELABOX. It's logged
    /// "by" `by` once BELABOX reports the change.
    pub async fn belabox(&self, switcher_id: Uuid, action: BelaboxAction, by: &str) -> Result<()> {
        self.belabox_ready(switcher_id).await?;

        let command = BelaboxCommand {
            action,
            by: by.to_owned(),
        };
        let payload = serde_json::to_vec(&command).expect("commands serialize");
        let subject = live::belabox_request_subject(switcher_id);
        match self
            .ask::<BelaboxReply>("BELABOX", switcher_id, subject, payload)
            .await
        {
            Some(Ok(())) => Ok(()),
            Some(Err(refusal)) => Err(Error::BelaboxRefused(refusal)),
            None => Err(Error::BelaboxUnreachable),
        }
    }

    /// Has a BELABOX link to talk to: a remote key, not paused, and obs-link running it.
    async fn belabox_ready(&self, switcher_id: Uuid) -> Result<()> {
        let has_key = db::switchers::get(&self.db, switcher_id)
            .await?
            .is_some_and(|switcher| switcher.belabox_remote_key.is_some());
        if !has_key {
            return Err(Error::BelaboxNotSetUp);
        }
        if db::switchers::is_paused(&self.db, switcher_id).await? {
            return Err(Error::Paused);
        }
        if self.belabox_status(switcher_id).await.is_none() {
            return Err(Error::BelaboxUnreachable);
        }
        Ok(())
    }

    /// What the BELABOX settings dialog shows (WiFi, modems, stream settings), asked of
    /// obs-link when needed: it's not in the status every viewer receives.
    pub async fn belabox_details(&self, switcher_id: Uuid) -> Result<BelaboxDetails> {
        self.belabox_ready(switcher_id).await?;

        let subject = live::belabox_details_subject(switcher_id);
        match self
            .ask::<BelaboxDetailsReply>("BELABOX details", switcher_id, subject, Vec::new())
            .await
        {
            Some(Ok(details)) => Ok(details),
            Some(Err(refusal)) => Err(Error::BelaboxRefused(refusal)),
            None => Err(Error::BelaboxUnreachable),
        }
    }

    /// The switcher's Moblin status, or `None` if no Moblin is connected.
    pub async fn moblin_status(&self, switcher_id: Uuid) -> Option<MoblinStatus> {
        match self.moblin_kv.get(live::status_key(switcher_id)).await {
            Ok(value) => value.and_then(|bytes| serde_json::from_slice(&bytes).ok()),
            Err(error) => {
                tracing::warn!(%error, %switcher_id, "Couldn't read a Moblin status");
                None
            }
        }
    }

    /// Has the switcher's connected Moblin carry out `action`. It's logged "by"
    /// `by` once Moblin reports the change.
    pub async fn moblin(&self, switcher_id: Uuid, action: MoblinAction, by: &str) -> Result<()> {
        let set_up = db::switchers::get(&self.db, switcher_id)
            .await?
            .is_some_and(|switcher| switcher.moblin_password.is_some());
        if !set_up {
            return Err(Error::MoblinNotSetUp);
        }
        if db::switchers::is_paused(&self.db, switcher_id).await? {
            return Err(Error::Paused);
        }
        if self.moblin_status(switcher_id).await.is_none() {
            return Err(Error::MoblinUnreachable);
        }

        let command = MoblinCommand {
            action,
            by: by.to_owned(),
        };
        let payload = serde_json::to_vec(&command).expect("commands serialize");
        let subject = live::moblin_request_subject(switcher_id);
        match self
            .ask::<MoblinReply>("Moblin", switcher_id, subject, payload)
            .await
        {
            Some(Ok(())) => Ok(()),
            Some(Err(refusal)) => Err(Error::MoblinRefused(refusal)),
            None => Err(Error::MoblinUnreachable),
        }
    }

    /// An OBS client for the switcher, if its OBS is connected right now.
    pub async fn connected_obs(&self, switcher_id: Uuid) -> Result<ObsClient<async_nats::Client>> {
        let connected = self
            .status(switcher_id)
            .await
            .is_some_and(|status| status.obs_connected);
        if !connected {
            return Err(Error::ObsOffline);
        }
        Ok(ObsClient::new(self.nats.clone(), switcher_id))
    }

    /// Like [`Self::connected_obs`], for changing something: refused while the
    /// switcher is paused for being over the plan's switcher limit.
    pub async fn controllable_obs(
        &self,
        switcher_id: Uuid,
    ) -> Result<ObsClient<async_nats::Client>> {
        if db::switchers::is_paused(&self.db, switcher_id).await? {
            return Err(Error::Paused);
        }
        self.connected_obs(switcher_id).await
    }

    /// Tells obs-link who is about to do `action`, so the resulting OBS change is
    /// attributed to them in the activity feed.
    pub async fn announce(&self, switcher_id: Uuid, action: IntentAction, by: &str) {
        let intent = Intent {
            action,
            by: by.to_owned(),
        };
        match serde_json::to_vec(&intent) {
            Ok(payload) => {
                if let Err(error) = self
                    .nats
                    .publish(live::intent_subject(switcher_id), payload.into())
                    .await
                {
                    tracing::warn!(%error, "Failed to announce an OBS action");
                }
            }
            Err(error) => tracing::warn!(%error, "Failed to serialize an intent"),
        }
    }

    pub async fn switch_scene(&self, switcher_id: Uuid, uuid: Uuid, by: &str) -> Result<()> {
        let obs = self.controllable_obs(switcher_id).await?;
        self.announce(switcher_id, IntentAction::Scene { uuid }, by)
            .await;
        obs.switch_scene_uuid(uuid).await?;
        Ok(())
    }

    /// Starts (`active`) or stops the stream.
    pub async fn stream(&self, switcher_id: Uuid, active: bool, by: &str) -> Result<()> {
        let obs = self.controllable_obs(switcher_id).await?;
        self.announce(switcher_id, IntentAction::Stream { active }, by)
            .await;
        obs.stream(active).await?;
        Ok(())
    }

    /// Starts (`active`) or stops recording.
    pub async fn record(&self, switcher_id: Uuid, active: bool, by: &str) -> Result<()> {
        let obs = self.controllable_obs(switcher_id).await?;
        self.announce(switcher_id, IntentAction::Record { active }, by)
            .await;
        obs.record(active).await?;
        Ok(())
    }

    /// Fixes a frozen feed like NOALBS's `!fix`: the network media sources
    /// playing on the scene on air reconnect (see [`obs_actions::fix`]). Returns
    /// the scene on air and the sources fixed, and logs them in the activity feed.
    pub async fn fix(&self, switcher_id: Uuid, by: &str) -> Result<(String, Vec<String>)> {
        let obs = self.controllable_obs(switcher_id).await?;
        let outcome = obs.fix_media_sources().await?;
        let mut failed = outcome.failed.into_iter();
        if outcome.fixed.is_empty()
            && let Some((_, error)) = failed.next()
        {
            return Err(error.into());
        }
        for (source, error) in failed {
            tracing::warn!(%error, %switcher_id, source, "Couldn't fix a media source");
        }

        if !outcome.fixed.is_empty() {
            let detail = EventDetail::FeedFixed {
                sources: outcome.fixed.clone(),
                by: Some(by.to_owned()),
            };
            if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await
            {
                tracing::warn!(%error, %switcher_id, "Couldn't log a feed fix");
            }
        }
        Ok((outcome.scene, outcome.fixed))
    }

    /// Marks this moment on the switcher's running stream and logs it in the
    /// activity feed. Needs no OBS connection, only a stream (the database's
    /// open one), so a marker made right as OBS starts may come too early.
    pub async fn create_marker(
        &self,
        switcher_id: Uuid,
        description: Option<&str>,
        by: &str,
    ) -> Result<CreatedMarker> {
        let description = clean_marker_description(description)?;
        if db::switchers::is_paused(&self.db, switcher_id).await? {
            return Err(Error::Paused);
        }
        let stream = db::streams::open(&self.db, switcher_id)
            .await?
            .ok_or(Error::NotStreaming)?;

        let at = chrono::Utc::now();
        let marker =
            db::stream_markers::insert(&self.db, stream.id, at, description, Some(by)).await?;
        let offset_seconds = (at - stream.started_at).num_seconds().max(0);

        let detail = EventDetail::MarkerCreated {
            marker_id: marker.id,
            description: marker.description.clone(),
            offset_seconds,
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a marker");
        }
        Ok(CreatedMarker {
            marker,
            offset_seconds,
        })
    }

    /// The scene on air and everything on it, nested scenes and groups included
    /// (see [`ObsClient::scene_tree`]). Allowed while paused: it only reads.
    pub async fn sources(&self, switcher_id: Uuid) -> Result<(String, Vec<SourceItem>)> {
        let obs = self.connected_obs(switcher_id).await?;
        let scene = obs.scene_list().await?.current_program_scene_name;
        let items = obs.scene_tree(&scene).await?;
        Ok((scene, items))
    }

    /// Shows (`enabled`) or hides item `item_id` of `scene` (a group when
    /// `group`), like NOALBS's `!source`, and logs it in the activity feed.
    /// Returns the source's name; nothing changes if it's already that way.
    pub async fn set_source_enabled(
        &self,
        switcher_id: Uuid,
        scene: &str,
        group: bool,
        item_id: i64,
        enabled: bool,
        by: &str,
    ) -> Result<String> {
        let obs = self.controllable_obs(switcher_id).await?;
        let item = obs
            .scene_items(scene, group)
            .await?
            .into_iter()
            .find(|item| item.scene_item_id == item_id)
            .ok_or(Error::SourceNotFound)?;
        if item.scene_item_enabled == enabled {
            return Ok(item.source_name);
        }

        obs.set_scene_item_enabled(scene, item_id, enabled).await?;
        let detail = EventDetail::SourceToggled {
            source: item.source_name.clone(),
            scene: scene.to_owned(),
            enabled,
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a source toggle");
        }
        Ok(item.source_name)
    }

    /// The scene on air and what OBS's Audio Mixer shows for it (see
    /// [`ObsClient::audio_mixer`]). Allowed while paused: it only reads.
    pub async fn audio(&self, switcher_id: Uuid) -> Result<(String, Vec<AudioInput>)> {
        let obs = self.connected_obs(switcher_id).await?;
        let scene = obs.scene_list().await?.current_program_scene_name;
        let tree = obs.scene_tree(&scene).await?;
        let inputs = obs.audio_mixer(&tree).await?;
        Ok((scene, inputs))
    }

    /// Mutes (`muted`) or unmutes an input and logs it in the activity feed;
    /// nothing changes if it's already that way.
    pub async fn set_input_muted(
        &self,
        switcher_id: Uuid,
        input: &str,
        muted: bool,
        by: &str,
    ) -> Result<()> {
        let obs = self.controllable_obs(switcher_id).await?;
        let current = obs.input_muted(input).await.map_err(input_error)?;
        if current == muted {
            return Ok(());
        }

        obs.set_input_muted(input, muted).await?;
        let detail = EventDetail::InputMuted {
            input: input.to_owned(),
            muted,
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a mute");
        }
        Ok(())
    }

    /// Moves an input's fader (dB, [`MIN_VOLUME_DB`] to [`MAX_VOLUME_DB`]). Not
    /// logged: a drag sends many.
    pub async fn set_input_volume(
        &self,
        switcher_id: Uuid,
        input: &str,
        volume_db: f64,
    ) -> Result<()> {
        if !(MIN_VOLUME_DB..=MAX_VOLUME_DB).contains(&volume_db) {
            return Err(Error::InvalidVolume);
        }
        let obs = self.controllable_obs(switcher_id).await?;
        obs.set_input_volume(input, volume_db)
            .await
            .map_err(input_error)
    }

    /// Refreshes the feed: shows the Refresh scene for [`REFRESH_HOLD`], then
    /// switches back to the scene that was on air, unless someone switched
    /// elsewhere meanwhile. Returns once the Refresh scene is on air.
    pub async fn refresh(&self, switcher: &db::switchers::Switcher, by: &str) -> Result<()> {
        let switcher_id = switcher.id;
        let refresh = switcher.scene_refresh.ok_or(Error::NoRefreshScene)?;
        let obs = self.controllable_obs(switcher_id).await?;
        let on_air = obs.scene_list().await?;
        let back = on_air.current_program_scene_uuid.ok_or(Error::NoSceneIds)?;
        if back == refresh {
            return Err(Error::AlreadyRefreshing);
        }

        self.announce(switcher_id, IntentAction::Scene { uuid: refresh }, by)
            .await;
        obs.switch_scene_uuid(refresh).await?;

        let control = self.clone();
        let by = by.to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(REFRESH_HOLD).await;
            match obs.scene_list().await {
                Ok(list) if list.current_program_scene_uuid == Some(refresh) => {}
                Ok(_) => return,
                Err(error) => {
                    tracing::warn!(%error, %switcher_id, "Couldn't check the scene after a refresh");
                    return;
                }
            }
            control
                .announce(switcher_id, IntentAction::Scene { uuid: back }, &by)
                .await;
            if let Err(error) = obs.switch_scene_uuid(back).await {
                tracing::warn!(%error, %switcher_id, "Couldn't switch back after a refresh");
            }
        });
        Ok(())
    }
}

/// An input request's error: OBS refusing a name that isn't an input with
/// audio means it's gone (or never was one).
fn input_error(error: obs_actions::Error) -> Error {
    match error {
        // No such input (600), not an input (602), or it has no audio (604).
        obs_actions::Error::ObsResponseError {
            code: 600 | 602 | 604,
            ..
        } => Error::InputNotFound,
        error => error.into(),
    }
}

/// Checks switching triggers the way the database would, with a message to show.
pub fn check_triggers(triggers: &db::switchers::Triggers) -> std::result::Result<(), &'static str> {
    let in_range = |value: Option<i32>, max: i32| value.is_none_or(|v| (1..=max).contains(&v));

    if !in_range(triggers.low, MAX_BITRATE_KBPS) || !in_range(triggers.offline, MAX_BITRATE_KBPS) {
        return Err("Bitrate triggers must be between 1 and 100,000 kbps");
    }
    if !in_range(triggers.rtt, MAX_RTT_MS) || !in_range(triggers.rtt_offline, MAX_RTT_MS) {
        return Err("RTT triggers must be between 1 and 60,000 ms");
    }
    if let (Some(low), Some(offline)) = (triggers.low, triggers.offline)
        && offline >= low
    {
        return Err("The offline bitrate must be lower than the LOW bitrate");
    }
    if let (Some(rtt), Some(rtt_offline)) = (triggers.rtt, triggers.rtt_offline)
        && rtt_offline <= rtt
    {
        return Err("The offline RTT must be higher than the LOW RTT");
    }
    Ok(())
}

/// A marker just made, and how far into the stream it is.
#[derive(Debug, Clone)]
pub struct CreatedMarker {
    pub marker: db::stream_markers::Marker,
    pub offset_seconds: i64,
}

/// Trims a marker's description; blank means none. Too long is refused.
fn clean_marker_description(description: Option<&str>) -> Result<Option<&str>> {
    let description = description.map(str::trim).filter(|text| !text.is_empty());
    match description {
        Some(text) if text.chars().count() > MAX_MARKER_CHARS => Err(Error::InvalidMarker),
        description => Ok(description),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_descriptions_are_trimmed_and_bounded() {
        assert_eq!(clean_marker_description(None).unwrap(), None);
        assert_eq!(clean_marker_description(Some("  \n ")).unwrap(), None);
        assert_eq!(
            clean_marker_description(Some("  Clutch play ")).unwrap(),
            Some("Clutch play")
        );

        let longest = "é".repeat(MAX_MARKER_CHARS);
        assert!(clean_marker_description(Some(&longest)).is_ok());
        let too_long = "é".repeat(MAX_MARKER_CHARS + 1);
        assert!(matches!(
            clean_marker_description(Some(&too_long)),
            Err(Error::InvalidMarker)
        ));
    }
}
