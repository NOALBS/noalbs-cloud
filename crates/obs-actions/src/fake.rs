//! A fake OBS for tests: answers requests from a canned function and records
//! what was asked.

use std::sync::Mutex;

use serde_json::{Value, json};

use crate::{
    ClientTransport, ObsClient,
    error::Result,
    types::{ObsMessage, RequestResponse, RequestStatus},
};

type Answer = fn(&str, &Value) -> Value;

pub struct FakeObs {
    answer: Answer,
    pub requests: Mutex<Vec<(String, Value)>>,
}

pub fn client(answer: Answer) -> ObsClient<FakeObs> {
    ObsClient::new(
        FakeObs {
            answer,
            requests: Mutex::default(),
        },
        uuid::Uuid::nil(),
    )
}

impl ObsClient<FakeObs> {
    /// The requests of these types, in order, with their data.
    pub fn asked(&self, kinds: &[&str]) -> Vec<(String, Value)> {
        self.transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(kind, _)| kinds.contains(&kind.as_str()))
            .cloned()
            .collect()
    }
}

const FAILURE: &str = "$failure";

/// An answer that makes OBS refuse the request with this status code.
pub fn failure(code: u32) -> Value {
    json!({ FAILURE: code })
}

/// A scene item as `GetSceneItemList` lists it.
pub fn input(id: i64, name: &str, kind: &str, enabled: bool) -> Value {
    json!({
        "sceneItemId": id, "sceneItemEnabled": enabled, "sourceName": name,
        "sourceType": "OBS_SOURCE_TYPE_INPUT", "inputKind": kind, "isGroup": null,
    })
}

/// A nested scene (or group) as `GetSceneItemList` lists it.
pub fn scene(id: i64, name: &str, group: bool, enabled: bool) -> Value {
    json!({
        "sceneItemId": id, "sceneItemEnabled": enabled, "sourceName": name,
        "sourceType": "OBS_SOURCE_TYPE_SCENE", "inputKind": null, "isGroup": group,
    })
}

#[async_trait::async_trait]
impl ClientTransport for FakeObs {
    async fn request(&self, payload: String, _: uuid::Uuid) -> Result<ObsMessage> {
        let request: Value = serde_json::from_str(&payload)?;
        let d = &request["d"];
        let kind = d["requestType"].as_str().unwrap().to_owned();
        let data = d["requestData"].clone();
        let response_data = (self.answer)(&kind, &data);
        self.requests.lock().unwrap().push((kind.clone(), data));
        let failed = response_data[FAILURE].as_u64();
        Ok(ObsMessage::RequestResponse(RequestResponse {
            request_type: kind,
            request_id: d["requestId"].as_str().unwrap().to_owned(),
            request_status: RequestStatus {
                code: failed.map_or(100, |code| code as u32),
                result: failed.is_none(),
                comment: None,
            },
            response_data: failed.is_none().then_some(response_data),
        }))
    }
}
