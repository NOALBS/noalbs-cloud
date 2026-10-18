//! Library for interacting with the OBS link service via NATS messaging.
//!
//! This crate provides a client for interacting with the OBS link service via NATS messaging. It
//! allows sending requests to the OBS link service and handling responses.

use std::time::Duration;

use async_nats::{Client, ConnectOptions};

pub mod audio;
pub mod error;
#[cfg(test)]
mod fake;
pub mod fix;
pub mod sources;
pub mod types;
pub use error::Error;

use error::Result;
use serde::de;

use crate::types::{ObsMessage, ObsRequest, RequestKind};

const DEFAULT_NATS_URL: &str = "nats://localhost:4222";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct Config {
    /// NATS server URL.
    pub nats_url: String,

    /// The timeout for NATS requests.
    ///
    /// If a request takes longer than this duration, it will be considered failed.
    pub request_timeout: Duration,

    /// The ID of the switcher that this ObsClient instance is responsible for
    ///
    /// The current switcher implementation doesn't use this field, but other
    /// services do use it to identify which switcher to send requests to.  
    pub switcher_id: uuid::Uuid,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            nats_url: DEFAULT_NATS_URL.to_string(),
            request_timeout: REQUEST_TIMEOUT,
            switcher_id: "00000000-0000-0000-0000-000000000000".parse().unwrap(),
        }
    }
}

#[async_trait::async_trait]
pub trait ClientTransport: Send + Sync {
    async fn request(&self, payload: String, switcher_id: uuid::Uuid) -> Result<ObsMessage>;
}

#[async_trait::async_trait]
impl ClientTransport for Client {
    async fn request(&self, payload: String, switcher_id: uuid::Uuid) -> Result<ObsMessage> {
        let subject = format!("obslink.{}.request", switcher_id);
        let response = telemetry::nats::request(self, subject, payload).await?;

        let response = serde_json::from_slice::<ObsMessage>(&response.payload)?;

        Ok(response)
    }
}

/// ObsClient is a client for interacting with the OBS link service via NATS messaging.
#[derive(Clone)]
pub struct ObsClient<CT: ClientTransport> {
    transport: CT,
    switcher_id: uuid::Uuid,
}

impl ObsClient<Client> {
    pub async fn new_nats(config: Config) -> Result<Self> {
        let transport = ConnectOptions::new()
            .request_timeout(Some(config.request_timeout))
            .connect(&config.nats_url)
            .await?;

        Ok(Self {
            transport,
            switcher_id: config.switcher_id,
        })
    }

    /// Creates a new ObsClient with the default configuration.
    pub async fn new_with_default_config() -> Result<Self> {
        let config = Config::default();
        Self::new_nats(config).await
    }
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// Creates a new ObsClient with the given configuration.
    ///
    /// This allows you to reuse an existing NATS client for the transport,
    /// which can be useful if you want to share a single NATS connection across
    /// multiple ObsClient instances.
    pub fn new(transport: CT, switcher_id: uuid::Uuid) -> Self {
        Self {
            transport,
            switcher_id,
        }
    }

    pub async fn send_request<'a, T>(&self, request_kind: RequestKind<'a>) -> Result<T>
    where
        T: de::DeserializeOwned + 'static,
    {
        let id = nanoid::nanoid!();
        let request = ObsRequest::Request(types::Request {
            id: &id,
            kind: request_kind,
        });

        let request_json = serde_json::to_string(&request)?;
        let response = self
            .transport
            .request(request_json, self.switcher_id)
            .await?;

        if let ObsMessage::RequestResponse(rs) = response {
            if rs.request_id != id {
                return Err(Error::RequestIdMismatch {
                    expected: id,
                    actual: rs.request_id,
                });
            }

            if !rs.request_status.result {
                return Err(Error::ObsResponseError {
                    code: rs.request_status.code,
                    result: rs.request_status.result,
                    comment: rs.request_status.comment,
                });
            }

            // Requests like StartStream have no response data; `null` then
            // deserializes into `()` or `Value`, and fails for real types.
            let data = rs.response_data.unwrap_or(serde_json::Value::Null);
            return Ok(serde_json::from_value::<T>(data)?);
        }

        Err(Error::ObsRequestError("Invalid response".to_string()))
    }

    /// Sends a request whose response data isn't needed; only checks it succeeded.
    pub async fn command(&self, request_kind: RequestKind<'_>) -> Result<()> {
        self.send_request::<serde_json::Value>(request_kind)
            .await
            .map(|_| ())
    }

    pub async fn switch_scene(&self, scene_name: &str) -> Result<()> {
        self.command(RequestKind::SetCurrentProgramScene { scene_name })
            .await
    }

    pub async fn switch_scene_uuid(&self, scene_uuid: uuid::Uuid) -> Result<()> {
        self.command(RequestKind::SetCurrentProgramSceneByUuid { scene_uuid })
            .await
    }

    pub async fn scene_list(&self) -> Result<types::GetSceneList> {
        self.send_request(RequestKind::GetSceneList).await
    }

    pub async fn stats(&self) -> Result<types::GetStats> {
        self.send_request(RequestKind::GetStats).await
    }

    pub async fn stream_status(&self) -> Result<types::GetStreamStatus> {
        self.send_request(RequestKind::GetStreamStatus).await
    }

    pub async fn recording_status(&self) -> Result<types::GetRecordStatus> {
        self.send_request(RequestKind::GetRecordStatus).await
    }

    /// A JPEG still of a scene (by UUID, else name), `width` wide, as a `data:` URI.
    pub async fn scene_screenshot(
        &self,
        scene: SceneSource<'_>,
        width: u32,
        quality: i32,
    ) -> Result<String> {
        let (source_name, source_uuid) = match scene {
            SceneSource::Uuid(uuid) => (None, Some(uuid)),
            SceneSource::Name(name) => (Some(name), None),
        };
        let screenshot: types::GetSourceScreenshot = self
            .send_request(RequestKind::GetSourceScreenshot {
                source_name,
                source_uuid,
                image_format: "jpg",
                image_width: Some(width),
                image_compression_quality: Some(quality),
            })
            .await?;
        Ok(screenshot.image_data)
    }

    /// Starts (`true`) or stops (`false`) streaming.
    pub async fn stream(&self, active: bool) -> Result<()> {
        self.command(if active {
            RequestKind::StartStream
        } else {
            RequestKind::StopStream
        })
        .await
    }

    /// Starts (`true`) or stops (`false`) recording.
    pub async fn record(&self, active: bool) -> Result<()> {
        self.command(if active {
            RequestKind::StartRecord
        } else {
            RequestKind::StopRecord
        })
        .await
    }

    /// Pauses (`true`) or resumes (`false`) recording.
    pub async fn pause_recording(&self, pause: bool) -> Result<()> {
        self.command(if pause {
            RequestKind::PauseRecord
        } else {
            RequestKind::ResumeRecord
        })
        .await
    }
}

/// Which scene to capture: by UUID (OBS 30+), or by name on older versions.
#[derive(Debug, Clone, Copy)]
pub enum SceneSource<'a> {
    Uuid(uuid::Uuid),
    Name(&'a str),
}

#[cfg(test)]
mod tests {
    use crate::types::{ObsRequest, Request, RequestKind};

    #[test]
    fn screenshots_ask_for_a_scaled_jpeg_by_uuid() {
        let uuid: uuid::Uuid = "11111111-1111-4111-8111-111111111111".parse().unwrap();
        let request = ObsRequest::Request(Request {
            id: "r1",
            kind: RequestKind::GetSourceScreenshot {
                source_name: None,
                source_uuid: Some(uuid),
                image_format: "jpg",
                image_width: Some(640),
                image_compression_quality: Some(60),
            },
        });
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "op": 6,
                "d": {
                    "requestId": "r1",
                    "requestType": "GetSourceScreenshot",
                    "requestData": {
                        "sourceUuid": uuid,
                        "imageFormat": "jpg",
                        "imageWidth": 640,
                        "imageCompressionQuality": 60,
                    },
                },
            })
        );
    }
}
