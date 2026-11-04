//! Using a source again: adding one OBS already has to another scene (OBS's
//! "Add Existing": the same source, so a change shows everywhere), and
//! duplicating one (a copy with its own settings, filters and placement, like
//! OBS's Duplicate). Both go into scenes only: OBS can't add to a group.
//!
//! A copy is made inside OBS, from the source's own settings: they never leave
//! it, so a copy of a source that plays a local file doesn't show the file.

use serde_json::{Map, Value};

use crate::{
    ClientTransport, ObsClient,
    error::Result,
    sources::SCENE_SOURCE,
    types::{
        CreateInput, CreateSceneItem, GetInputList, GetInputSettings, GetSceneItemTransform,
        GetSourceFilterList, GetSpecialInputs, RequestKind,
    },
};

/// A source OBS has that can be added to a scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingSource {
    pub name: String,
    /// The input's kind; `None` for scenes.
    pub input_kind: Option<String>,
    pub is_scene: bool,
}

/// What [`ObsClient::duplicate_input`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Duplication {
    /// Copied this source.
    Copied(String),
    /// The item isn't in that scene or group (anymore).
    Gone,
    /// It's a scene or a group, which aren't copied here.
    NotAnInput,
}

/// The parts of an item's placement that can be set again (the rest, like its
/// size on screen, follows from them).
const TRANSFORM_KEYS: [&str; 10] = [
    "positionX",
    "positionY",
    "rotation",
    "scaleX",
    "scaleY",
    "alignment",
    "cropLeft",
    "cropRight",
    "cropTop",
    "cropBottom",
];
const BOUNDS_KEYS: [&str; 5] = [
    "boundsType",
    "boundsAlignment",
    "boundsWidth",
    "boundsHeight",
    "cropToBounds",
];

/// An item's placement as `SetSceneItemTransform` takes it. Bounds go only when
/// it has some: OBS refuses a bounds size under 1, which it reports for none.
fn settable_transform(transform: &Value) -> Value {
    let has_bounds = transform["boundsType"]
        .as_str()
        .is_some_and(|bounds| bounds != "OBS_BOUNDS_NONE");
    let keys = TRANSFORM_KEYS
        .iter()
        .chain(if has_bounds { &BOUNDS_KEYS[..] } else { &[] });
    let out: Map<String, Value> = keys
        .filter_map(|&key| Some((key.to_owned(), transform.get(key)?.clone())))
        .collect();
    Value::Object(out)
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// The sources that can be added to a scene: every input but the global
    /// audio devices (they're on every scene already), then every scene.
    pub async fn existing_sources(&self) -> Result<Vec<ExistingSource>> {
        let inputs: GetInputList = self.send_request(RequestKind::GetInputList).await?;
        let special: GetSpecialInputs = self.send_request(RequestKind::GetSpecialInputs).await?;
        let globals = [
            special.desktop1,
            special.desktop2,
            special.mic1,
            special.mic2,
            special.mic3,
            special.mic4,
        ];
        let mut sources: Vec<ExistingSource> = inputs
            .inputs
            .into_iter()
            .filter(|input| {
                !globals
                    .iter()
                    .flatten()
                    .any(|name| *name == input.input_name)
            })
            .map(|input| ExistingSource {
                name: input.input_name,
                input_kind: Some(input.input_kind),
                is_scene: false,
            })
            .collect();
        sources.sort_by_key(|source| source.name.to_lowercase());

        let mut scenes = self.scene_list().await?.scenes;
        // obs-websocket lists scenes bottom first.
        scenes.sort_by(|a, b| b.scene_index.cmp(&a.scene_index));
        sources.extend(scenes.into_iter().map(|scene| ExistingSource {
            name: scene.scene_name,
            input_kind: None,
            is_scene: true,
        }));
        Ok(sources)
    }

    /// Adds source `source` (an input or a scene) to `scene`, on top, where it
    /// sits in the first other scene that has it (OBS's default spot if none
    /// does). OBS refuses a scene within itself, and groups.
    pub async fn add_existing(&self, scene: &str, source: &str, enabled: bool) -> Result<i64> {
        let created: CreateSceneItem = self
            .send_request(RequestKind::CreateSceneItem {
                scene_name: scene,
                source_name: source,
                scene_item_enabled: enabled,
            })
            .await?;
        match self.placement_elsewhere(source, scene).await {
            Ok(Some(transform)) => {
                let placed = self
                    .command(RequestKind::SetSceneItemTransform {
                        scene_name: scene,
                        scene_item_id: created.scene_item_id,
                        scene_item_transform: transform,
                    })
                    .await;
                if let Err(error) = placed {
                    tracing::warn!(%error, "Couldn't place an added source like its other one");
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "Couldn't look for an added source's placement"),
        }
        Ok(created.scene_item_id)
    }

    /// Where `source` sits in the first scene other than `except` that has it
    /// (not in a group: those positions are the group's), settable again.
    async fn placement_elsewhere(&self, source: &str, except: &str) -> Result<Option<Value>> {
        let mut scenes = self.scene_list().await?.scenes;
        scenes.sort_by(|a, b| b.scene_index.cmp(&a.scene_index));
        for other in scenes.iter().filter(|other| other.scene_name != except) {
            let items = self.scene_items(&other.scene_name, false).await?;
            let Some(item) = items.iter().find(|item| item.source_name == source) else {
                continue;
            };
            let transform: GetSceneItemTransform = self
                .send_request(RequestKind::GetSceneItemTransform {
                    scene_name: &other.scene_name,
                    scene_item_id: item.scene_item_id,
                })
                .await?;
            return Ok(Some(settable_transform(&transform.scene_item_transform)));
        }
        Ok(None)
    }

    /// Copies item `item_id` of `from` (a group when `from_group`) into scene
    /// `to` as a new source named `name`: its kind and settings, its filters
    /// and its placement. Only the source itself is needed: if a filter or the
    /// placement can't be copied, that's logged and the copy stays.
    pub async fn duplicate_input(
        &self,
        from: &str,
        from_group: bool,
        item_id: i64,
        name: &str,
        to: &str,
        enabled: bool,
    ) -> Result<Duplication> {
        let items = self.scene_items(from, from_group).await?;
        let Some(item) = items.into_iter().find(|item| item.scene_item_id == item_id) else {
            return Ok(Duplication::Gone);
        };
        if item.source_type == SCENE_SOURCE || item.is_group == Some(true) {
            return Ok(Duplication::NotAnInput);
        }
        let source = item.source_name;

        // Its own settings only: the kind's defaults fill in the rest, as for it.
        let settings: GetInputSettings = self
            .send_request(RequestKind::GetInputSettings {
                input_name: &source,
            })
            .await?;
        let created: CreateInput = self
            .send_request(RequestKind::CreateInput {
                scene_name: to,
                input_name: name,
                input_kind: &settings.input_kind,
                input_settings: settings.input_settings,
                scene_item_enabled: enabled,
            })
            .await?;

        if let Err(error) = self
            .copy_placement(from, item_id, to, created.scene_item_id)
            .await
        {
            tracing::warn!(%error, "Couldn't copy a duplicated source's placement");
        }
        if let Err(error) = self.copy_filters(&source, name).await {
            tracing::warn!(%error, "Couldn't copy a duplicated source's filters");
        }
        Ok(Duplication::Copied(source))
    }

    async fn copy_placement(&self, from: &str, item_id: i64, to: &str, new_id: i64) -> Result<()> {
        let transform: GetSceneItemTransform = self
            .send_request(RequestKind::GetSceneItemTransform {
                scene_name: from,
                scene_item_id: item_id,
            })
            .await?;
        self.command(RequestKind::SetSceneItemTransform {
            scene_name: to,
            scene_item_id: new_id,
            scene_item_transform: settable_transform(&transform.scene_item_transform),
        })
        .await
    }

    async fn copy_filters(&self, from: &str, to: &str) -> Result<()> {
        let list: GetSourceFilterList = self
            .send_request(RequestKind::GetSourceFilterList { source_name: from })
            .await?;
        for filter in list.filters {
            self.command(RequestKind::CreateSourceFilter {
                source_name: to,
                filter_name: &filter.filter_name,
                filter_kind: &filter.filter_kind,
                filter_settings: filter.filter_settings,
            })
            .await?;
            if !filter.filter_enabled {
                self.command(RequestKind::SetSourceFilterEnabled {
                    source_name: to,
                    filter_name: &filter.filter_name,
                    filter_enabled: false,
                })
                .await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::fake::{client, input, scene};

    fn obs(kind: &str, data: &Value) -> Value {
        match kind {
            "GetInputList" => json!({ "inputs": [
                { "inputName": "Mic/Aux", "inputKind": "wasapi_input_capture" },
                { "inputName": "chat", "inputKind": "browser_source" },
                { "inputName": "BELABOX SRT", "inputKind": "ffmpeg_source" },
            ] }),
            "GetSpecialInputs" => json!({ "mic1": "Mic/Aux", "desktop1": null }),
            "GetSceneList" => json!({
                "currentProgramSceneName": "LIVE",
                "currentPreviewSceneName": null,
                "scenes": [
                    { "sceneIndex": 0, "sceneName": "BRB" },
                    { "sceneIndex": 1, "sceneName": "LIVE" },
                ],
            }),
            "GetSceneItemList" | "GetGroupSceneItemList" => json!({ "sceneItems": [
                input(3, "BELABOX SRT", "ffmpeg_source", true),
                scene(4, "Overlay", true, true),
            ] }),
            "GetInputSettings" => json!({
                "inputKind": "ffmpeg_source",
                "inputSettings": { "is_local_file": false, "input": "srt://ingest:4000" },
            }),
            "CreateInput" => json!({ "sceneItemId": 9 }),
            "CreateSceneItem" => json!({ "sceneItemId": 5 }),
            "GetSceneItemTransform" => json!({ "sceneItemTransform": {
                "positionX": 10.0, "positionY": 20.0, "rotation": 0.0, "scaleX": 0.5, "scaleY": 0.5,
                "alignment": 5, "boundsType": "OBS_BOUNDS_NONE", "boundsWidth": 0.0,
                "boundsHeight": 0.0, "boundsAlignment": 0, "cropLeft": 0, "cropRight": 0,
                "cropTop": 0, "cropBottom": 8, "cropToBounds": false,
                "width": 960.0, "height": 540.0, "sourceWidth": 1920.0, "sourceHeight": 1080.0,
            } }),
            "GetSourceFilterList" if data["sourceName"] == "BELABOX SRT" => json!({ "filters": [
                { "filterName": "Sharpen", "filterKind": "sharpness_filter_v2",
                  "filterSettings": { "sharpness": 0.2 }, "filterEnabled": true },
                { "filterName": "Delay", "filterKind": "gpu_delay",
                  "filterSettings": {}, "filterEnabled": false },
            ] }),
            _ => Value::Null,
        }
    }

    #[tokio::test]
    async fn existing_sources_leave_out_the_global_audio_devices() {
        let obs = client(obs);
        let sources = obs.existing_sources().await.unwrap();
        let names: Vec<_> = sources
            .iter()
            .map(|source| (source.name.as_str(), source.is_scene))
            .collect();
        assert_eq!(
            names,
            [
                ("BELABOX SRT", false),
                ("chat", false),
                ("LIVE", true),
                ("BRB", true)
            ]
        );
    }

    #[tokio::test]
    async fn adding_an_existing_source_adds_an_item_of_it() {
        let obs = client(obs);
        assert_eq!(obs.add_existing("BRB", "chat", false).await.unwrap(), 5);
        assert_eq!(
            obs.asked(&["CreateSceneItem"])[0].1,
            json!({ "sceneName": "BRB", "sourceName": "chat", "sceneItemEnabled": false })
        );
        // No other scene has it: OBS's default spot.
        assert!(obs.asked(&["SetSceneItemTransform"]).is_empty());

        // LIVE has it: placed the same.
        obs.add_existing("BRB", "BELABOX SRT", true).await.unwrap();
        let placed = &obs.asked(&["SetSceneItemTransform"])[0].1;
        assert_eq!(placed["sceneName"], "BRB");
        assert_eq!(placed["sceneItemId"], 5);
        assert_eq!(placed["sceneItemTransform"]["positionY"], 20.0);
        assert_eq!(
            obs.asked(&["GetSceneItemTransform"])[0].1["sceneName"],
            "LIVE"
        );
    }

    #[tokio::test]
    async fn duplicates_copy_settings_placement_and_filters() {
        let obs = client(obs);
        assert_eq!(
            obs.duplicate_input("LIVE", false, 3, "BELABOX SRT 2", "BRB", true)
                .await
                .unwrap(),
            Duplication::Copied("BELABOX SRT".into())
        );

        let create = &obs.asked(&["CreateInput"])[0].1;
        assert_eq!(create["sceneName"], "BRB");
        assert_eq!(create["inputName"], "BELABOX SRT 2");
        assert_eq!(create["inputKind"], "ffmpeg_source");
        assert_eq!(create["inputSettings"]["input"], "srt://ingest:4000");

        let transform = &obs.asked(&["SetSceneItemTransform"])[0].1;
        assert_eq!(transform["sceneName"], "BRB");
        assert_eq!(transform["sceneItemId"], 9);
        assert_eq!(transform["sceneItemTransform"]["positionX"], 10.0);
        assert_eq!(transform["sceneItemTransform"]["cropBottom"], 8);
        let set = transform["sceneItemTransform"].as_object().unwrap();
        assert!(!set.contains_key("boundsWidth"), "no bounds: none sent");
        assert!(!set.contains_key("width"), "read-only fields aren't sent");

        let filters = obs.asked(&["CreateSourceFilter", "SetSourceFilterEnabled"]);
        assert_eq!(
            filters,
            [
                (
                    "CreateSourceFilter".to_owned(),
                    json!({ "sourceName": "BELABOX SRT 2", "filterName": "Sharpen",
                            "filterKind": "sharpness_filter_v2", "filterSettings": { "sharpness": 0.2 } })
                ),
                (
                    "CreateSourceFilter".to_owned(),
                    json!({ "sourceName": "BELABOX SRT 2", "filterName": "Delay",
                            "filterKind": "gpu_delay", "filterSettings": {} })
                ),
                (
                    "SetSourceFilterEnabled".to_owned(),
                    json!({ "sourceName": "BELABOX SRT 2", "filterName": "Delay", "filterEnabled": false })
                ),
            ]
        );
    }

    #[tokio::test]
    async fn scenes_groups_and_gone_items_aren_t_duplicated() {
        let obs = client(obs);
        assert_eq!(
            obs.duplicate_input("LIVE", false, 4, "Copy", "LIVE", true)
                .await
                .unwrap(),
            Duplication::NotAnInput
        );
        assert_eq!(
            obs.duplicate_input("LIVE", false, 99, "Copy", "LIVE", true)
                .await
                .unwrap(),
            Duplication::Gone
        );
        assert!(obs.asked(&["CreateInput"]).is_empty());
    }

    #[test]
    fn placement_keeps_bounds_only_when_it_has_some() {
        let with_bounds = settable_transform(&json!({
            "positionX": 0.0, "boundsType": "OBS_BOUNDS_SCALE_INNER", "boundsWidth": 1920.0,
        }));
        assert_eq!(with_bounds["boundsWidth"], 1920.0);
        assert_eq!(with_bounds["boundsType"], "OBS_BOUNDS_SCALE_INNER");
    }
}
