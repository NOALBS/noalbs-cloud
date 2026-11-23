use common::programs::ProgramStatus;
use futures_util::SinkExt;
use serde::Serialize;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::{ConnectLogUpload, LinkConfig, PcObsState, RelayError, Socket};

#[derive(Serialize)]
pub(super) struct CloudMessage<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
    #[serde(flatten)]
    body: CloudBody<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum CloudBody<'a> {
    Identify(Identify<'a>),
    ObsWsConnected(bool),
    PcState { obs: PcObsState },
    ProgramsState(Vec<ProgramStatus>),
    ObsRawResponse(Value),
    ConnectCommandResult(CommandResult),
    LogUpload(ConnectLogUpload),
    ObsRawEvent(Value),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Identify<'a> {
    token: &'a str,
    version: &'a str,
    client: &'static str,
    capabilities: [&'static str; 5],
    os: &'a str,
    obs_version: Option<&'a str>,
}

#[derive(Serialize)]
pub(super) struct CommandResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    log: Option<ConnectLogUpload>,
}

impl From<Result<Option<ConnectLogUpload>, String>> for CommandResult {
    fn from(result: Result<Option<ConnectLogUpload>, String>) -> Self {
        match result {
            Ok(log) => Self {
                ok: true,
                error: None,
                log,
            },
            Err(error) => Self {
                ok: false,
                error: Some(error),
                log: None,
            },
        }
    }
}

pub(super) fn identify_payload<'a>(
    config: &'a LinkConfig,
    obs_version: Option<&'a str>,
) -> CloudMessage<'a> {
    CloudMessage {
        token: Some(&config.device_token),
        body: CloudBody::Identify(Identify {
            token: &config.device_token,
            version: &config.version,
            client: "connect",
            capabilities: [
                "obs_process",
                "obs_control",
                "obs_logs",
                "programs",
                "keepalive",
            ],
            os: &config.os,
            obs_version,
        }),
    }
}

pub(super) async fn send_cloud(
    socket: &mut Socket,
    token: Option<&str>,
    body: CloudBody<'_>,
) -> Result<(), RelayError> {
    send_message(socket, CloudMessage { token, body }).await
}

pub(super) async fn send_message(
    socket: &mut Socket,
    message: CloudMessage<'_>,
) -> Result<(), RelayError> {
    socket
        .send(Message::Text(serde_json::to_string(&message)?.into()))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, to_value};

    use super::*;

    #[test]
    fn outbound_cloud_messages_keep_obs_link_wire_shapes() {
        let log = ConnectLogUpload {
            kind: "obs_log".into(),
            file_name: "obs.log".into(),
            modified_at: None,
            truncated: false,
            content_gzip_base64: "payload".into(),
        };
        let log_json = json!({
            "kind":"obs_log","fileName":"obs.log","modifiedAt":null,
            "truncated":false,"contentGzipBase64":"payload"
        });
        let event = json!({"op":5,"d":{"eventType":"SomeFutureEvent"}});
        let response = json!({"op":9,"d":{"requestId":"batch","results":[]}});
        for (token, body, expected) in [
            (
                None,
                CloudBody::ObsWsConnected(false),
                json!({"obsWsConnected":false}),
            ),
            (
                None,
                CloudBody::PcState {
                    obs: PcObsState::Connected,
                },
                json!({"pcState":{"obs":"connected"}}),
            ),
            (
                None,
                CloudBody::ProgramsState(Vec::new()),
                json!({"programsState":[]}),
            ),
            (
                None,
                CloudBody::ObsRawEvent(event.clone()),
                json!({"obsRawEvent":event}),
            ),
            (
                Some("batch"),
                CloudBody::ObsRawResponse(response.clone()),
                json!({"token":"batch","obsRawResponse":response}),
            ),
            (
                None,
                CloudBody::LogUpload(log.clone()),
                json!({"logUpload":log_json}),
            ),
            (
                Some("upload"),
                CloudBody::ConnectCommandResult(Ok(Some(log)).into()),
                json!({"token":"upload","connectCommandResult":{"ok":true,"log":log_json}}),
            ),
            (
                Some("start"),
                CloudBody::ConnectCommandResult(Ok(None).into()),
                json!({"token":"start","connectCommandResult":{"ok":true}}),
            ),
            (
                Some("stop"),
                CloudBody::ConnectCommandResult(Err("OBS refused".into()).into()),
                json!({"token":"stop","connectCommandResult":{"ok":false,"error":"OBS refused"}}),
            ),
        ] {
            assert_eq!(to_value(CloudMessage { token, body }).unwrap(), expected);
        }
    }
}
