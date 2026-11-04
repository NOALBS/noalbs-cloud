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
use obs_actions::{
    ObsClient,
    audio::AudioInput,
    inputs::{self, EditableKind, InputDetails, Removal, SourceSettings},
    placement::{self, Layout, Placement},
    reuse::{Duplication, ExistingSource},
    sources::SourceItem,
};
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
    #[error("only media, VLC, browser and text sources can be edited here")]
    NotEditable,
    #[error("OBS doesn't have that kind of source")]
    KindUnavailable,
    #[error(transparent)]
    InvalidSource(inputs::Invalid),
    #[error("a source or scene already has that name")]
    NameTaken,
    #[error("groups, and what's in them, can only be removed in OBS")]
    NotRemovable,
    #[error("scenes and groups can't be duplicated here")]
    NotDuplicable,
    #[error("a scene can't be added to itself")]
    SceneInItself,
    #[error(transparent)]
    InvalidPlacement(placement::Invalid),
    #[error("that scene isn't in OBS anymore")]
    SceneNotFound,
    #[error("the scene on air can't be removed")]
    SceneOnAir,
    #[error("the switcher uses that scene as its {0} scene")]
    SceneInUse(SceneRole),
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

    /// The kinds of source this OBS can add from the dashboard (VLC needs OBS's
    /// VLC plugin). Allowed while paused: it only reads.
    pub async fn source_kinds(&self, switcher_id: Uuid) -> Result<Vec<EditableKind>> {
        let obs = self.connected_obs(switcher_id).await?;
        Ok(obs.editable_kinds().await?)
    }

    /// A source's settings, for editing; [`Error::NotEditable`] unless it's a
    /// media, VLC, browser or text source. Allowed while paused: it only reads.
    pub async fn source_details(&self, switcher_id: Uuid, name: &str) -> Result<InputDetails> {
        let obs = self.connected_obs(switcher_id).await?;
        obs.input_details(name)
            .await
            .map_err(source_error)?
            .ok_or(Error::NotEditable)
    }

    /// Adds a source to `scene` and logs it in the activity feed. Returns its
    /// (trimmed) name.
    pub async fn create_source(
        &self,
        switcher_id: Uuid,
        scene: &str,
        name: &str,
        settings: &SourceSettings,
        enabled: bool,
        by: &str,
    ) -> Result<String> {
        let name = inputs::clean_name(name).map_err(Error::InvalidSource)?;
        settings.check(true).map_err(Error::InvalidSource)?;
        let obs = self.controllable_obs(switcher_id).await?;
        let created = obs
            .create_input(scene, name, settings, enabled)
            .await
            .map_err(source_error)?
            .ok_or(Error::KindUnavailable)?;
        // It's there already; a wrong spot can be fixed with Position.
        let placement = settings.kind().default_placement();
        if let Err(error) = obs
            .place_item(scene, created.scene_item_id, &placement)
            .await
        {
            tracing::warn!(%error, %switcher_id, "Couldn't place a new source");
        }

        let detail = EventDetail::SourceCreated {
            source: name.to_owned(),
            scene: scene.to_owned(),
            copied_from: None,
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a new source");
        }
        Ok(name.to_owned())
    }

    /// Changes a media, VLC, browser or text source's settings (a `None` URL keeps
    /// what it plays) and renames it to `new_name` if that differs, then logs it.
    /// Returns its name now.
    pub async fn edit_source(
        &self,
        switcher_id: Uuid,
        name: &str,
        new_name: &str,
        settings: &SourceSettings,
        by: &str,
    ) -> Result<String> {
        let new_name = inputs::clean_name(new_name).map_err(Error::InvalidSource)?;
        settings.check(false).map_err(Error::InvalidSource)?;
        let obs = self.controllable_obs(switcher_id).await?;
        let updated = obs
            .update_input(name, settings)
            .await
            .map_err(source_error)?;
        if !updated {
            return Err(Error::NotEditable);
        }
        let renamed = new_name != name;
        if renamed {
            obs.rename_input(name, new_name)
                .await
                .map_err(source_error)?;
        }

        let detail = EventDetail::SourceEdited {
            source: new_name.to_owned(),
            renamed_from: renamed.then(|| name.to_owned()),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a source change");
        }
        Ok(new_name.to_owned())
    }

    /// The sources OBS has that can be added to a scene (see
    /// [`ObsClient::existing_sources`]). Allowed while paused: it only reads.
    pub async fn existing_sources(&self, switcher_id: Uuid) -> Result<Vec<ExistingSource>> {
        let obs = self.connected_obs(switcher_id).await?;
        Ok(obs.existing_sources().await?)
    }

    /// Adds source `source` OBS already has (an input or a scene) to `scene`,
    /// on top, and logs it: the same source, so changes show in both.
    pub async fn add_existing_source(
        &self,
        switcher_id: Uuid,
        scene: &str,
        source: &str,
        enabled: bool,
        by: &str,
    ) -> Result<()> {
        if scene == source {
            return Err(Error::SceneInItself);
        }
        let obs = self.controllable_obs(switcher_id).await?;
        obs.add_existing(scene, source, enabled)
            .await
            .map_err(source_error)?;

        let detail = EventDetail::SourceAdded {
            source: source.to_owned(),
            scene: scene.to_owned(),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log an added source");
        }
        Ok(())
    }

    /// Copies item `item_id` of `from` (a group when `from_group`) into scene
    /// `to` as `name` (see [`ObsClient::duplicate_input`]) and logs it.
    /// Returns the copy's (trimmed) name.
    #[allow(clippy::too_many_arguments)]
    pub async fn duplicate_source(
        &self,
        switcher_id: Uuid,
        from: &str,
        from_group: bool,
        item_id: i64,
        name: &str,
        to: &str,
        enabled: bool,
        by: &str,
    ) -> Result<String> {
        let name = inputs::clean_name(name).map_err(Error::InvalidSource)?;
        let obs = self.controllable_obs(switcher_id).await?;
        let source = match obs
            .duplicate_input(from, from_group, item_id, name, to, enabled)
            .await
            .map_err(source_error)?
        {
            Duplication::Copied(source) => source,
            Duplication::Gone => return Err(Error::SourceNotFound),
            Duplication::NotAnInput => return Err(Error::NotDuplicable),
        };

        let detail = EventDetail::SourceCreated {
            source: name.to_owned(),
            scene: to.to_owned(),
            copied_from: Some(source),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a duplicated source");
        }
        Ok(name.to_owned())
    }

    /// Where everything in `scene` (a group when `group`) is on screen now,
    /// top first. Allowed while paused: it only reads.
    pub async fn scene_layout(
        &self,
        switcher_id: Uuid,
        scene: &str,
        group: bool,
    ) -> Result<Layout> {
        let obs = self.connected_obs(switcher_id).await?;
        Ok(obs.layout(scene, group).await?)
    }

    /// Puts item `item_id` of `scene` (a group when `group`) where `placement`
    /// says and logs it. Returns the source's name.
    pub async fn place_source(
        &self,
        switcher_id: Uuid,
        scene: &str,
        group: bool,
        item_id: i64,
        placement: &Placement,
        by: &str,
    ) -> Result<String> {
        placement.check().map_err(Error::InvalidPlacement)?;
        let obs = self.controllable_obs(switcher_id).await?;
        let source = obs
            .place(scene, group, item_id, placement)
            .await?
            .ok_or(Error::SourceNotFound)?;

        let detail = EventDetail::SourcePlaced {
            source: source.clone(),
            scene: scene.to_owned(),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a placed source");
        }
        Ok(source)
    }

    /// Removes item `item_id` from `scene` (a group when `group`: refused, like
    /// groups themselves, see [`ObsClient::remove_scene_item`]) and logs it.
    /// Returns the source's name.
    pub async fn remove_source(
        &self,
        switcher_id: Uuid,
        scene: &str,
        group: bool,
        item_id: i64,
        by: &str,
    ) -> Result<String> {
        if group {
            return Err(Error::NotRemovable);
        }
        let obs = self.controllable_obs(switcher_id).await?;
        let source = match obs.remove_scene_item(scene, item_id).await? {
            Removal::Removed(source) => source,
            Removal::Group => return Err(Error::NotRemovable),
            Removal::Gone => return Err(Error::SourceNotFound),
        };

        let detail = EventDetail::SourceRemoved {
            source: source.clone(),
            scene: scene.to_owned(),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a removed source");
        }
        Ok(source)
    }

    /// Moves item `item_id` of `scene` (a group when `group`) to `position`
    /// in OBS's Sources dock, counted from the top. Not logged: arranging
    /// takes many.
    pub async fn move_source(
        &self,
        switcher_id: Uuid,
        scene: &str,
        group: bool,
        item_id: i64,
        position: usize,
    ) -> Result<()> {
        let obs = self.controllable_obs(switcher_id).await?;
        obs.move_scene_item(scene, group, item_id, position)
            .await?
            .ok_or(Error::SourceNotFound)?;
        Ok(())
    }

    /// Adds an empty scene to OBS and logs it. Returns its (trimmed) name.
    pub async fn create_scene(&self, switcher_id: Uuid, name: &str, by: &str) -> Result<String> {
        let name = inputs::clean_name(name).map_err(Error::InvalidSource)?;
        let obs = self.controllable_obs(switcher_id).await?;
        obs.create_scene(name).await.map_err(source_error)?;

        let detail = EventDetail::SceneCreated {
            scene: name.to_owned(),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a new scene");
        }
        Ok(name.to_owned())
    }

    /// Removes scene `uuid` from OBS, and what's on it, and logs it. Refused
    /// for the scene on air and for the scenes the switcher switches to (it
    /// would stop working): another one is picked first. Returns its name.
    pub async fn remove_scene(&self, switcher_id: Uuid, uuid: Uuid, by: &str) -> Result<String> {
        let switcher = db::switchers::get(&self.db, switcher_id)
            .await?
            .ok_or(Error::SceneNotFound)?;
        if let Some(role) = SceneRole::of(&switcher, uuid) {
            return Err(Error::SceneInUse(role));
        }
        let obs = self.controllable_obs(switcher_id).await?;
        let list = obs.scene_list().await?;
        if list.current_program_scene_uuid == Some(uuid) {
            return Err(Error::SceneOnAir);
        }
        let name = list
            .scenes
            .into_iter()
            .find(|scene| scene.scene_uuid == Some(uuid))
            .ok_or(Error::SceneNotFound)?
            .scene_name;
        obs.remove_scene(&name).await?;

        let detail = EventDetail::SceneRemoved {
            scene: name.clone(),
            by: Some(by.to_owned()),
        };
        if let Err(error) = live::record_event(&self.db, &self.nats, switcher_id, detail).await {
            tracing::warn!(%error, %switcher_id, "Couldn't log a removed scene");
        }
        Ok(name)
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

/// What the switcher uses a scene for (switching to it, or for a button).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneRole {
    Live,
    Low,
    Offline,
    Starting,
    Ending,
    Privacy,
    Refresh,
}

impl SceneRole {
    /// The role `switcher` gives scene `uuid`, if any.
    pub fn of(switcher: &db::switchers::Switcher, uuid: Uuid) -> Option<Self> {
        [
            (switcher.scene_live, Self::Live),
            (switcher.scene_low, Self::Low),
            (switcher.scene_offline, Self::Offline),
            (switcher.scene_starting, Self::Starting),
            (switcher.scene_ending, Self::Ending),
            (switcher.scene_privacy, Self::Privacy),
            (switcher.scene_refresh, Self::Refresh),
        ]
        .into_iter()
        .find_map(|(scene, role)| (scene == Some(uuid)).then_some(role))
    }
}

impl std::fmt::Display for SceneRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Live => "LIVE",
            Self::Low => "LOW",
            Self::Offline => "BRB",
            Self::Starting => "Starting",
            Self::Ending => "Ending",
            Self::Privacy => "Privacy",
            Self::Refresh => "Refresh",
        })
    }
}

/// A source request's error: OBS refusing a name it doesn't know means the
/// source is gone; one it already has, that the name is taken.
fn source_error(error: obs_actions::Error) -> Error {
    match error {
        obs_actions::Error::ObsResponseError { code: 600, .. } => Error::SourceNotFound,
        error if inputs::is_name_taken(&error) => Error::NameTaken,
        error => error.into(),
    }
}

/// Why switching triggers can't be used. The chat bot says these in the switcher's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TriggerError {
    #[error("Bitrate triggers must be between 1 and 100,000 kbps")]
    BitrateRange,
    #[error("RTT triggers must be between 1 and 60,000 ms")]
    RttRange,
    #[error("The offline bitrate must be lower than the LOW bitrate")]
    OfflineBitrate,
    #[error("The offline RTT must be higher than the LOW RTT")]
    OfflineRtt,
}

/// Checks switching triggers the way the database would.
pub fn check_triggers(triggers: &db::switchers::Triggers) -> std::result::Result<(), TriggerError> {
    let in_range = |value: Option<i32>, max: i32| value.is_none_or(|v| (1..=max).contains(&v));

    if !in_range(triggers.low, MAX_BITRATE_KBPS) || !in_range(triggers.offline, MAX_BITRATE_KBPS) {
        return Err(TriggerError::BitrateRange);
    }
    if !in_range(triggers.rtt, MAX_RTT_MS) || !in_range(triggers.rtt_offline, MAX_RTT_MS) {
        return Err(TriggerError::RttRange);
    }
    if let (Some(low), Some(offline)) = (triggers.low, triggers.offline)
        && offline >= low
    {
        return Err(TriggerError::OfflineBitrate);
    }
    if let (Some(rtt), Some(rtt_offline)) = (triggers.rtt, triggers.rtt_offline)
        && rtt_offline <= rtt
    {
        return Err(TriggerError::OfflineRtt);
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
