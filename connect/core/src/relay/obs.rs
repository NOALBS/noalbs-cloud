use std::{collections::HashMap, time::Duration};

use futures_util::{SinkExt, StreamExt};
use obs_protocol::{
    EVENT_SUBSCRIPTION_OUTPUTS, Event, GetVersion, Identify, ObsMessage, ObsRequest, OutputStatus,
    Request, RequestKind, RequestResponse,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::time::{Instant, timeout_at};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::{OutputStopOutcome, RelayError, Socket, StopOutputs, extract_request_id, hash_base64};

pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const RAW_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RAW_REQUESTS: usize = 1024;
const OUTPUT_STOP_TIMEOUT: Duration = Duration::from_secs(25);
const OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(super) async fn connect_obs(port: u16, password: &str) -> Result<Socket, RelayError> {
    connect_obs_with_subscriptions(port, password, None).await
}

async fn connect_obs_with_subscriptions(
    port: u16,
    password: &str,
    event_subscriptions: Option<u32>,
) -> Result<Socket, RelayError> {
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
            event_subscriptions,
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

    pub(super) fn complete(&self, message: &Value) -> Result<Option<Value>, RelayError> {
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

pub async fn stop_active_streams(port: u16, password: &str) -> Result<StopOutputs, RelayError> {
    stop_outputs(port, password, OUTPUT_STOP_TIMEOUT).await
}

async fn stop_outputs(
    port: u16,
    password: &str,
    budget: Duration,
) -> Result<StopOutputs, RelayError> {
    let deadline = Instant::now() + budget;
    let mut socket = timeout_at(
        deadline.min(Instant::now() + REQUEST_TIMEOUT),
        connect_obs_with_subscriptions(port, password, Some(EVENT_SUBSCRIPTION_OUTPUTS)),
    )
    .await
    .map_err(|_| RelayError::ObsTimeout)??;

    let mut stream = OutputShutdown::new(Output::Stream);
    let mut recording = OutputShutdown::new(Output::Recording);
    let mut connection_error = None;
    'shutdown: while Instant::now() < deadline {
        for output in [&mut stream, &mut recording] {
            if timeout_at(deadline, output.advance(&mut socket))
                .await
                .is_err()
            {
                break 'shutdown;
            }
        }
        let Some(wake_at) = [stream.wake_at(), recording.wake_at()]
            .into_iter()
            .flatten()
            .min()
        else {
            break;
        };
        let message = match timeout_at(wake_at.min(deadline), read_json(&mut socket)).await {
            Ok(Ok(message)) => message,
            Ok(Err(error)) => {
                connection_error = Some(error);
                break;
            }
            Err(_) => continue,
        };
        if message.get("op").and_then(Value::as_u64) == Some(5) {
            // OBS sets outputActive=false on STOPPING too.
            const STOPPED: &str = "OBS_WEBSOCKET_OUTPUT_STOPPED";
            match Event::deserialize(&message["d"]) {
                Ok(Event::StreamStateChanged(event)) if event.output_state == STOPPED => {
                    stream.stopped();
                }
                Ok(Event::RecordStateChanged(event)) if event.output_state == STOPPED => {
                    recording.stopped();
                }
                _ => {}
            }
        } else {
            stream.reply(&message);
            recording.reply(&message);
        }
    }
    let interrupted = connection_error.is_some();
    Ok(StopOutputs {
        stream: stream.finish(&mut connection_error, interrupted),
        recording: recording.finish(&mut connection_error, interrupted),
    })
}

#[derive(Clone, Copy)]
enum ShutdownRequest {
    InitialStatus,
    Stop,
    PollStatus,
}

enum ShutdownState {
    Ready(ShutdownRequest, Instant),
    Pending(ShutdownRequest, PendingCall),
    Done(Result<OutputStopOutcome, RelayError>),
}

struct OutputShutdown {
    output: Output,
    state: ShutdownState,
    stopped_before_ack: bool,
}

enum Output {
    Stream,
    Recording,
}

impl OutputShutdown {
    fn new(output: Output) -> Self {
        Self {
            output,
            state: ShutdownState::Ready(ShutdownRequest::InitialStatus, Instant::now()),
            stopped_before_ack: false,
        }
    }

    fn wake_at(&self) -> Option<Instant> {
        match &self.state {
            ShutdownState::Ready(_, at) => Some(*at),
            ShutdownState::Pending(_, call) => Some(call.deadline),
            ShutdownState::Done(_) => None,
        }
    }

    async fn advance(&mut self, socket: &mut Socket) {
        use ShutdownRequest::{PollStatus, Stop};
        use ShutdownState::{Done, Pending, Ready};

        match &self.state {
            Ready(request, at) if Instant::now() >= *at => {
                let request = *request;
                let kind = match (&self.output, request) {
                    (Output::Stream, Stop) => RequestKind::StopStream,
                    (Output::Recording, Stop) => RequestKind::StopRecord,
                    (Output::Stream, _) => RequestKind::GetStreamStatus,
                    (Output::Recording, _) => RequestKind::GetRecordStatus,
                };
                self.state = match PendingCall::send(socket, kind).await {
                    Ok(call) => Pending(request, call),
                    Err(error) => Done(Err(error)),
                };
            }
            Pending(request, call) if call.expired() => {
                self.state = match request {
                    PollStatus => Self::poll_later(),
                    _ => Done(Err(RelayError::ObsTimeout)),
                };
            }
            _ => {}
        }
    }

    fn stopped(&mut self) {
        match self.state {
            ShutdownState::Pending(ShutdownRequest::Stop, _) => self.stopped_before_ack = true,
            ShutdownState::Pending(ShutdownRequest::PollStatus, _)
            | ShutdownState::Ready(ShutdownRequest::PollStatus, _) => {
                self.state = ShutdownState::Done(Ok(OutputStopOutcome::Stopped));
            }
            _ => {}
        }
    }

    fn reply(&mut self, message: &Value) {
        use ShutdownRequest::{InitialStatus, PollStatus, Stop};
        use ShutdownState::{Done, Pending, Ready};

        let Pending(request, call) = &self.state else {
            return;
        };
        if !call.matches(message) || call.expired() {
            return;
        }
        let response = call.complete(message);
        self.state = match request {
            Stop => match response {
                Ok(_) if self.stopped_before_ack => Done(Ok(OutputStopOutcome::Stopped)),
                Ok(_) => Self::poll_later(),
                Err(error) => Done(Err(error)),
            },
            InitialStatus | PollStatus => {
                match response.and_then(decode_response::<OutputStatus>) {
                    Ok(status) => match (request, status.output_active) {
                        (InitialStatus, false) => Done(Ok(OutputStopOutcome::AlreadyInactive)),
                        (InitialStatus, true) => Ready(Stop, Instant::now()),
                        (PollStatus, false) => Done(Ok(OutputStopOutcome::Stopped)),
                        (PollStatus, true) => Self::poll_later(),
                        (Stop, _) => unreachable!(),
                    },
                    Err(error) => Done(Err(error)),
                }
            }
        };
    }

    fn poll_later() -> ShutdownState {
        ShutdownState::Ready(
            ShutdownRequest::PollStatus,
            Instant::now() + OUTPUT_POLL_INTERVAL,
        )
    }

    fn finish(
        self,
        connection_error: &mut Option<RelayError>,
        interrupted: bool,
    ) -> Result<OutputStopOutcome, RelayError> {
        match self.state {
            ShutdownState::Done(result) => result,
            _ if interrupted => Err(connection_error.take().unwrap_or(RelayError::ObsClosed)),
            _ => Err(RelayError::ObsOutputStopTimeout),
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
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{WebSocketStream, accept_async};

    use super::*;

    async fn send_mock(socket: &mut WebSocketStream<TcpStream>, message: Value) {
        socket
            .send(Message::text(message.to_string()))
            .await
            .unwrap();
    }

    async fn receive_mock(socket: &mut WebSocketStream<TcpStream>) -> Value {
        serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap()
    }

    async fn mock_reply(socket: &mut WebSocketStream<TcpStream>, kind: &str, active: Option<bool>) {
        let request = receive_mock(socket).await;
        assert_eq!(request["d"]["requestType"], kind);
        send_mock(
            socket,
            json!({"op":7,"d":{"requestId":request["d"]["requestId"],"requestType":kind,
                "requestStatus":{"code":100,"result":true},
                "responseData":active.map(|active| json!({"outputActive":active}))}}),
        )
        .await;
    }

    async fn mock_event(socket: &mut WebSocketStream<TcpStream>, event: &str, state: &str) {
        send_mock(
            socket,
            json!({"op":5,"d":{"eventType":event,"eventIntent":EVENT_SUBSCRIPTION_OUTPUTS,
                "eventData":{"outputActive":false,"outputState":state}}}),
        )
        .await;
    }

    async fn acknowledge_stops(listener: TcpListener) -> WebSocketStream<TcpStream> {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        send_mock(
            &mut socket,
            json!({"op":0,"d":{"rpcVersion":1,"obsWebSocketVersion":"5.4.2"}}),
        )
        .await;
        assert_eq!(
            receive_mock(&mut socket).await,
            json!({"op":1,"d":{"rpcVersion":1,"eventSubscriptions":EVENT_SUBSCRIPTION_OUTPUTS}}),
        );
        send_mock(&mut socket, json!({"op":2,"d":{"negotiatedRpcVersion":1}})).await;
        mock_reply(&mut socket, "GetStreamStatus", Some(true)).await;
        mock_reply(&mut socket, "GetRecordStatus", Some(true)).await;
        mock_reply(&mut socket, "StopStream", None).await;
        mock_reply(&mut socket, "StopRecord", None).await;
        socket
    }

    #[tokio::test]
    async fn output_shutdown_waits_for_stopped_event_and_uses_status_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut socket = acknowledge_stops(listener).await;
            mock_event(
                &mut socket,
                "StreamStateChanged",
                "OBS_WEBSOCKET_OUTPUT_STOPPING",
            )
            .await;
            mock_reply(&mut socket, "GetStreamStatus", Some(true)).await;
            mock_event(
                &mut socket,
                "StreamStateChanged",
                "OBS_WEBSOCKET_OUTPUT_STOPPED",
            )
            .await;
            mock_reply(&mut socket, "GetRecordStatus", Some(false)).await;
        });
        let result = stop_outputs(port, "", Duration::from_secs(5))
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(result.stream.unwrap(), OutputStopOutcome::Stopped);
        assert_eq!(result.recording.unwrap(), OutputStopOutcome::Stopped);
    }

    #[tokio::test]
    async fn output_shutdown_reports_timeout_without_losing_other_output_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut socket = acknowledge_stops(listener).await;
            mock_event(
                &mut socket,
                "RecordStateChanged",
                "OBS_WEBSOCKET_OUTPUT_STOPPED",
            )
            .await;
            mock_event(
                &mut socket,
                "StreamStateChanged",
                "OBS_WEBSOCKET_OUTPUT_STOPPING",
            )
            .await;
            mock_reply(&mut socket, "GetStreamStatus", Some(true)).await;
            let _ = socket.next().await;
        });
        let result = stop_outputs(port, "", Duration::from_secs(2))
            .await
            .unwrap();
        server.await.unwrap();
        assert!(matches!(
            result.stream,
            Err(RelayError::ObsOutputStopTimeout)
        ));
        assert_eq!(result.recording.unwrap(), OutputStopOutcome::Stopped);
    }

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
