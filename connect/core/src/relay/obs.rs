use std::{collections::HashMap, time::Duration};

use futures_util::{SinkExt, StreamExt};
use obs_protocol::{
    GetVersion, Identify, ObsMessage, ObsRequest, Request, RequestKind, RequestResponse,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::{RelayError, Socket, extract_request_id, hash_base64};

pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RAW_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RAW_REQUESTS: usize = 1024;

pub(super) async fn connect_obs(port: u16, password: &str) -> Result<Socket, RelayError> {
    let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}")).await?;
    let ObsMessage::Hello(hello) = serde_json::from_value(read_json(&mut socket).await?)? else {
        return Err(RelayError::ObsHandshake);
    };
    let authentication = hello.authentication.map(|auth| {
        let secret = hash_base64(format!("{password}{}", auth.salt).as_bytes());
        hash_base64(format!("{secret}{}", auth.challenge).as_bytes())
    });
    send_obs(
        &mut socket,
        ObsRequest::Identify(Identify {
            rpc_version: 1,
            authentication,
            event_subscriptions: None,
        }),
    )
    .await?;
    if !matches!(
        serde_json::from_value(read_json(&mut socket).await?),
        Ok(ObsMessage::Identified(_))
    ) {
        return Err(RelayError::ObsAuthentication);
    }
    Ok(socket)
}

async fn send_obs(socket: &mut Socket, request: ObsRequest<'_>) -> Result<(), RelayError> {
    socket
        .send(Message::Text(serde_json::to_string(&request)?.into()))
        .await?;
    Ok(())
}

pub(super) struct PendingCall {
    id: String,
    deadline: Instant,
}

impl PendingCall {
    pub(super) async fn send(
        socket: &mut Socket,
        kind: RequestKind<'_>,
    ) -> Result<Self, RelayError> {
        let call = Self {
            id: format!("noalbs-connect-{}", uuid::Uuid::new_v4()),
            deadline: Instant::now() + REQUEST_TIMEOUT,
        };
        timeout_at(
            call.deadline,
            send_obs(socket, ObsRequest::Request(Request { id: &call.id, kind })),
        )
        .await
        .map_err(|_| RelayError::ObsTimeout)??;
        Ok(call)
    }

    pub(super) fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    pub(super) fn matches(&self, message: &Value) -> bool {
        message.get("op").and_then(Value::as_u64) == Some(7)
            && extract_request_id(message) == Some(self.id.as_str())
    }

    pub(super) fn complete(self, message: &Value) -> Result<Option<Value>, RelayError> {
        if self.expired() {
            return Err(RelayError::ObsTimeout);
        }
        let response = RequestResponse::deserialize(message.get("d").unwrap_or(&Value::Null))?;
        if !response.request_status.result {
            return Err(RelayError::ObsRejected {
                request: response.request_type,
                code: response.request_status.code,
                comment: response.request_status.comment,
            });
        }
        Ok(response.response_data)
    }
}

pub(super) fn decode_response<T: DeserializeOwned>(data: Option<Value>) -> Result<T, RelayError> {
    Ok(serde_json::from_value(data.unwrap_or(Value::Null))?)
}

pub(super) async fn request_obs(
    socket: &mut Socket,
    kind: RequestKind<'_>,
) -> Result<Option<Value>, RelayError> {
    let call = PendingCall::send(socket, kind).await?;
    timeout_at(call.deadline, async {
        loop {
            let response = read_json(socket).await?;
            if call.matches(&response) {
                return call.complete(&response);
            }
        }
    })
    .await
    .map_err(|_| RelayError::ObsTimeout)?
}

pub(super) async fn get_obs_version(socket: &mut Socket) -> Result<Option<String>, RelayError> {
    let version: GetVersion = decode_response(request_obs(socket, RequestKind::GetVersion).await?)?;
    Ok(Some(version.obs_version))
}

async fn read_json(socket: &mut Socket) -> Result<Value, RelayError> {
    loop {
        match socket.next().await.ok_or(RelayError::ObsClosed)?? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Close(_) => return Err(RelayError::ObsClosed),
            _ => {}
        }
    }
}

#[derive(Default)]
pub(super) struct RawRequests(HashMap<(u64, String), (String, Instant)>);

impl RawRequests {
    pub(super) fn track(&mut self, request: &Value, token: Option<&str>) -> bool {
        let Some((token, id)) = token.zip(extract_request_id(request)) else {
            return true;
        };
        let reply_op = match request.get("op").and_then(Value::as_u64) {
            Some(6) => 7,
            Some(8) => 9,
            _ => return true,
        };
        if self.0.len() >= MAX_RAW_REQUESTS {
            self.expire();
            if self.0.len() >= MAX_RAW_REQUESTS {
                return false;
            }
        }
        match self.0.entry((reply_op, id.to_owned())) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((token.to_owned(), Instant::now() + RAW_REQUEST_TIMEOUT));
                true
            }
        }
    }

    pub(super) fn resolve(&mut self, reply: &Value) -> Option<String> {
        let op = reply.get("op")?.as_u64()?;
        let id = extract_request_id(reply)?;
        let (token, deadline) = self.0.remove(&(op, id.to_owned()))?;
        (deadline > Instant::now()).then_some(token)
    }

    pub(super) fn expire(&mut self) {
        let now = Instant::now();
        self.0.retain(|_, (_, deadline)| *deadline > now);
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn raw_requests_are_bounded_and_expired_replies_do_not_resolve() {
        let mut pending = RawRequests::default();
        for id in 0..MAX_RAW_REQUESTS {
            assert!(pending.track(
                &json!({"op":6,"d":{"requestId":id.to_string()}}),
                Some("token")
            ));
        }
        let overflow = json!({"op":8,"d":{"requestId":"overflow"}});
        assert!(!pending.track(&overflow, Some("overflow")));
        for (_, deadline) in pending.0.values_mut() {
            *deadline = Instant::now();
        }
        assert!(
            pending
                .resolve(&json!({"op":7,"d":{"requestId":"0"}}))
                .is_none()
        );
        pending.expire();
        assert!(pending.0.is_empty());
        assert!(pending.track(&overflow, Some("batch")));
        assert!(
            pending
                .resolve(&json!({"op":7,"d":{"requestId":"overflow"}}))
                .is_none()
        );
        assert_eq!(
            pending
                .resolve(&json!({"op":9,"d":{"requestId":"overflow"}}))
                .as_deref(),
            Some("batch")
        );
    }
}
