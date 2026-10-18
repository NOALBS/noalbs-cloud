//! The sources on a scene, nested scenes and groups included, and showing or
//! hiding them (NOALBS's `!source`).

use std::{collections::HashSet, future::Future, pin::Pin};

use crate::{
    ClientTransport, ObsClient,
    error::Result,
    types::{GetSceneItemList, RequestKind, SceneItem},
};

pub(crate) const SCENE_SOURCE: &str = "OBS_SOURCE_TYPE_SCENE";

/// One item of a scene's tree.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceItem {
    /// The scene or group it sits in (what showing or hiding it needs).
    pub parent: String,
    /// Whether `parent` is a group.
    pub parent_is_group: bool,
    pub item_id: i64,
    pub name: String,
    /// Not sent by OBS versions before 30.
    pub uuid: Option<uuid::Uuid>,
    /// The input's kind (e.g. `ffmpeg_source`); `None` for scenes and groups.
    pub input_kind: Option<String>,
    /// A nested scene (not a group).
    pub is_scene: bool,
    pub is_group: bool,
    pub enabled: bool,
    /// 0 on the scene itself, 1 inside a nested scene or group, …
    pub depth: u32,
}

impl SourceItem {
    fn new(item: SceneItem, parent: &str, parent_is_group: bool, depth: u32) -> Self {
        let is_group = item.is_group.unwrap_or(false);
        Self {
            parent: parent.to_owned(),
            parent_is_group,
            item_id: item.scene_item_id,
            is_scene: item.source_type == SCENE_SOURCE && !is_group,
            is_group,
            name: item.source_name,
            uuid: item.source_uuid,
            input_kind: item.input_kind,
            enabled: item.scene_item_enabled,
            depth,
        }
    }
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// The items of a scene or group, bottom first (as obs-websocket lists them).
    pub async fn scene_items(&self, scene: &str, group: bool) -> Result<Vec<SceneItem>> {
        let request = if group {
            RequestKind::GetGroupSceneItemList { scene_name: scene }
        } else {
            RequestKind::GetSceneItemList { scene_name: scene }
        };
        let list: GetSceneItemList = self.send_request(request).await?;
        Ok(list.scene_items)
    }

    /// Everything on `scene`, top first like OBS's Sources dock, with the items
    /// of each nested scene or group right after it. A scene nested twice is
    /// only expanded the first time.
    pub async fn scene_tree(&self, scene: &str) -> Result<Vec<SourceItem>> {
        let mut items = Vec::new();
        let mut visited = HashSet::from([scene.to_owned()]);
        self.walk(scene.to_owned(), false, 0, &mut visited, &mut items)
            .await?;
        Ok(items)
    }

    fn walk<'a>(
        &'a self,
        scene: String,
        group: bool,
        depth: u32,
        visited: &'a mut HashSet<String>,
        out: &'a mut Vec<SourceItem>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            for item in self.scene_items(&scene, group).await?.into_iter().rev() {
                let item = SourceItem::new(item, &scene, group, depth);
                let expand = (item.is_scene || item.is_group) && visited.insert(item.name.clone());
                let (name, is_group) = (item.name.clone(), item.is_group);
                out.push(item);
                if expand {
                    self.walk(name, is_group, depth + 1, visited, out).await?;
                }
            }
            Ok(())
        })
    }

    /// Shows (`true`) or hides an item of a scene or group.
    pub async fn set_scene_item_enabled(
        &self,
        scene: &str,
        item_id: i64,
        enabled: bool,
    ) -> Result<()> {
        self.command(RequestKind::SetSceneItemEnabled {
            scene_name: scene,
            scene_item_id: item_id,
            scene_item_enabled: enabled,
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::fake::{client, input, scene};

    fn obs(kind: &str, data: &Value) -> Value {
        let items = match (kind, data["sceneName"].as_str().unwrap_or_default()) {
            // Bottom first, as obs-websocket lists them.
            ("GetSceneItemList", "LIVE") => vec![
                input(1, "Camera", "ffmpeg_source", true),
                scene(2, "Overlay", true, true),
                scene(3, "Alerts", false, false),
            ],
            ("GetGroupSceneItemList", "Overlay") => vec![input(1, "Chat", "browser_source", false)],
            ("GetSceneItemList", "Alerts") => {
                vec![
                    input(1, "Sound", "ffmpeg_source", true),
                    scene(2, "Overlay", true, true),
                ]
            }
            _ => vec![],
        };
        json!({ "sceneItems": items })
    }

    #[tokio::test]
    async fn trees_list_top_first_with_nested_items_after_their_parent() {
        let obs = client(obs);
        let tree = obs.scene_tree("LIVE").await.unwrap();
        let summary: Vec<_> = tree
            .iter()
            .map(|item| {
                (
                    item.depth,
                    item.name.as_str(),
                    item.parent.as_str(),
                    item.enabled,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (0, "Alerts", "LIVE", false),
                (1, "Overlay", "Alerts", true),
                (2, "Chat", "Overlay", false),
                (1, "Sound", "Alerts", true),
                // Already expanded above.
                (0, "Overlay", "LIVE", true),
                (0, "Camera", "LIVE", true),
            ]
        );
        assert!(tree[0].is_scene && !tree[0].is_group);
        assert!(tree[1].is_group && tree[2].parent_is_group);
        assert_eq!(
            obs.asked(&["GetGroupSceneItemList"]).len(),
            1,
            "each scene is read once"
        );
    }

    #[tokio::test]
    async fn items_in_groups_are_shown_through_the_group() {
        let obs = client(obs);
        obs.set_scene_item_enabled("Overlay", 1, true)
            .await
            .unwrap();
        assert_eq!(
            obs.asked(&["SetSceneItemEnabled"]),
            [(
                "SetSceneItemEnabled".to_owned(),
                json!({ "sceneName": "Overlay", "sceneItemId": 1, "sceneItemEnabled": true })
            )]
        );
    }
}
