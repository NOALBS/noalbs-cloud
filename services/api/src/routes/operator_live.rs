use std::{cmp::Ordering, collections::HashMap};

use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    app::ApiState,
    routes::{ApiResult, stream_servers::StreamServerKindDto},
};

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ActiveServerDto {
    pub id: Uuid,
    pub name: String,
    pub kind: StreamServerKindDto,
    pub bitrate: Option<u32>,
    pub rtt: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Attention {
    NoFeed,
    Low,
    ObsLost,
}

pub fn matches_filter(
    filter: Option<super::organization_ops::LiveFilter>,
    streaming: bool,
    obs_connected: bool,
    attention: Option<Attention>,
) -> bool {
    match filter {
        Some(super::organization_ops::LiveFilter::Attention) => attention.is_some(),
        Some(super::organization_ops::LiveFilter::Live) => streaming,
        Some(super::organization_ops::LiveFilter::Obs) => obs_connected,
        None => true,
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OfflineSwitcherDto {
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<super::workspaces::WorkspaceRole>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub paused: bool,
}

pub fn offline_page(items: &mut Vec<OfflineSwitcherDto>, q: Option<&str>) -> i64 {
    items.retain(|item| {
        matches_search(
            q,
            &[
                &item.switcher_name,
                &item.workspace_name,
                &item.workspace_slug,
            ],
        )
    });
    items.sort_by(|a, b| {
        b.last_seen_at
            .cmp(&a.last_seen_at)
            .then_with(|| a.switcher_name.cmp(&b.switcher_name))
            .then_with(|| a.switcher_id.cmp(&b.switcher_id))
    });
    let total = items.len() as i64;
    items.truncate(100);
    total
}

pub fn attention(status: &live::SwitcherStatus) -> (Option<Attention>, Option<DateTime<Utc>>) {
    if let Some(lost) = &status.obs_lost
        && lost.stream_started_at.is_some()
    {
        return (Some(Attention::ObsLost), Some(lost.since));
    }
    let attention = match (status.streaming, status.state) {
        (true, live::FeedState::Offline) => Some(Attention::NoFeed),
        (true, live::FeedState::Low) => Some(Attention::Low),
        _ => None,
    };
    (attention, attention.and(status.state_since))
}

pub fn compare(
    a: (
        Option<Attention>,
        Option<DateTime<Utc>>,
        bool,
        Option<DateTime<Utc>>,
        &str,
    ),
    b: (
        Option<Attention>,
        Option<DateTime<Utc>>,
        bool,
        Option<DateTime<Utc>>,
        &str,
    ),
) -> Ordering {
    let key = |(attention, since, streaming, started, _): (
        Option<Attention>,
        Option<DateTime<Utc>>,
        bool,
        Option<DateTime<Utc>>,
        &str,
    )| {
        (
            if attention.is_some() {
                0
            } else if streaming {
                1
            } else {
                2
            },
            if attention.is_some() {
                since
            } else if streaming {
                started
            } else {
                None
            },
        )
    };
    key(a).cmp(&key(b)).then_with(|| a.4.cmp(b.4))
}

pub fn matches_search(q: Option<&str>, texts: &[&str]) -> bool {
    q.map(str::trim).filter(|q| !q.is_empty()).is_none_or(|q| {
        texts
            .iter()
            .any(|text| text.to_lowercase().contains(&q.to_lowercase()))
    })
}

pub fn fresh_server(
    sample: &live::StatsSample,
    server_id: Uuid,
    now: DateTime<Utc>,
) -> Option<&live::ServerSample> {
    let age = now.signed_duration_since(sample.at);
    if age < chrono::Duration::zero() || age > chrono::Duration::seconds(5) {
        return None;
    }
    sample
        .servers
        .iter()
        .find(|server| server.server_id == server_id)
}

pub async fn servers(
    state: &ApiState,
    statuses: &HashMap<Uuid, live::SwitcherStatus>,
) -> ApiResult<HashMap<Uuid, ActiveServerDto>> {
    let ids: Vec<_> = statuses
        .values()
        .filter_map(|status| status.active_server_id)
        .collect();
    let names: HashMap<_, _> = db::stream_servers::names(&state.db, &ids)
        .await?
        .into_iter()
        .map(|server| (server.id, server))
        .collect();
    let active: Vec<_> = statuses
        .iter()
        .filter_map(|(&id, status)| status.active_server_id.map(|server| (id, server)))
        .collect();
    let results = stream::iter(active)
        .map(|(id, server_id)| {
            let names = &names;
            async move {
                let server = names.get(&server_id)?;
                let message = state
                    .stores
                    .stats
                    .get_last_raw_message_by_subject(&live::stats_subject(id))
                    .await
                    .ok();
                let sample = message.and_then(|message| {
                    serde_json::from_slice::<live::StatsSample>(&message.payload).ok()
                });
                let numbers = sample
                    .as_ref()
                    .and_then(|sample| fresh_server(sample, server_id, Utc::now()));
                Some((
                    id,
                    ActiveServerDto {
                        id: server.id,
                        name: server.name.clone(),
                        kind: server.kind.into(),
                        bitrate: numbers.map(|sample| sample.bitrate),
                        rtt: numbers.and_then(|sample| sample.rtt),
                    },
                ))
            }
        })
        .buffer_unordered(32)
        .collect::<Vec<_>>()
        .await;
    Ok(results.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use live::{FeedState, ServerSample, StatsSample, SwitcherStatus};
    use serde_json::json;

    #[test]
    fn live_items_serialize_operator_fields() {
        let now = Utc::now();
        let item = super::super::my_workspaces::MyLiveSwitcherDto {
            switcher_id: Uuid::nil(),
            switcher_name: "Main".into(),
            workspace_id: Uuid::nil(),
            workspace_name: "Studio".into(),
            workspace_slug: "studio".into(),
            role: super::super::workspaces::WorkspaceRole::Operator,
            obs_connected: false,
            streaming: false,
            recording: false,
            paused: false,
            state: FeedState::Idle,
            stream_started_at: None,
            scene: None,
            state_since: Some(now),
            stops_at: None,
            obs_lost: Some(live::ObsLost {
                since: now,
                stream_started_at: Some(now),
            }),
            attention: Some(Attention::ObsLost),
            attention_since: Some(now),
            server: None,
        };
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["attention"], "obs_lost");
        assert_eq!(value["obs_lost"]["since"], json!(now));
        assert_eq!(value["state_since"], json!(now));
        assert_eq!(value["attention_since"], json!(now));
        assert!(value["stops_at"].is_null());
        assert!(value["server"].is_null());
        let item = super::super::organization_ops::LiveSwitcherDto {
            switcher_id: Uuid::nil(),
            switcher_name: "Main".into(),
            workspace_id: Uuid::nil(),
            workspace_name: "Studio".into(),
            workspace_slug: "studio".into(),
            obs_connected: true,
            streaming: true,
            recording: false,
            paused: false,
            state: FeedState::Offline,
            stream_started_at: Some(now),
            scene: None,
            state_since: Some(now),
            stops_at: Some(now),
            obs_lost: None,
            attention: Some(Attention::NoFeed),
            attention_since: Some(now),
            server: None,
        };
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["attention"], "no_feed");
        assert_eq!(value["stops_at"], json!(now));
        assert!(value["obs_lost"].is_null());
        assert!(value.get("role").is_none());
    }

    #[test]
    fn attention_only_for_streaming_or_obs_lost_mid_stream() {
        let now = Utc::now();
        let mut status = SwitcherStatus::connected(true);
        status.state = FeedState::Offline;
        status.state_since = Some(now);
        assert_eq!(attention(&status), (None, None));
        status.streaming = true;
        assert_eq!(attention(&status), (Some(Attention::NoFeed), Some(now)));
        status.state = FeedState::Low;
        assert_eq!(attention(&status).0, Some(Attention::Low));
        status.state = FeedState::Live;
        assert_eq!(attention(&status), (None, None));
        status.streaming = false;
        status.obs_lost = Some(live::ObsLost {
            since: now,
            stream_started_at: None,
        });
        assert_eq!(attention(&status), (None, None));
        status.obs_lost.as_mut().unwrap().stream_started_at = Some(now);
        assert_eq!(attention(&status), (Some(Attention::ObsLost), Some(now)));
    }

    #[test]
    fn attention_then_stream_duration_then_name() {
        let old = Utc::now() - chrono::Duration::minutes(5);
        let new = Utc::now();
        let mut rows = vec![
            (None, None, false, None, "B"),
            (None, None, true, Some(new), "new stream"),
            (
                Some(Attention::Low),
                Some(new),
                true,
                Some(old),
                "new attention",
            ),
            (None, None, false, None, "A"),
            (
                Some(Attention::ObsLost),
                Some(old),
                false,
                None,
                "old attention",
            ),
            (None, None, true, Some(old), "old stream"),
        ];
        rows.sort_by(|a, b| compare(*a, *b));
        assert_eq!(
            rows.iter().map(|row| row.4).collect::<Vec<_>>(),
            [
                "old attention",
                "new attention",
                "old stream",
                "new stream",
                "A",
                "B"
            ]
        );
    }

    #[test]
    fn stale_missing_and_future_samples_are_ignored() {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let mut sample = StatsSample {
            at: now,
            servers: vec![ServerSample {
                server_id: id,
                bitrate: 1200,
                rtt: Some(20.0),
                state: FeedState::Live,
            }],
        };
        assert_eq!(fresh_server(&sample, id, now).unwrap().bitrate, 1200);
        assert!(fresh_server(&sample, Uuid::new_v4(), now).is_none());
        sample.at = now - chrono::Duration::seconds(6);
        assert!(fresh_server(&sample, id, now).is_none());
        sample.at = now + chrono::Duration::seconds(1);
        assert!(fresh_server(&sample, id, now).is_none());
        sample.servers.clear();
        sample.at = now;
        assert!(fresh_server(&sample, id, now).is_none());
    }

    #[test]
    fn offline_search_order_and_cap() {
        let now = Utc::now();
        let mut rows: Vec<_> = (0..105)
            .map(|i| OfflineSwitcherDto {
                switcher_id: Uuid::new_v4(),
                switcher_name: format!("Main {i}"),
                workspace_id: Uuid::new_v4(),
                workspace_name: "Studio".into(),
                workspace_slug: "studio".into(),
                role: None,
                last_seen_at: if i == 0 {
                    None
                } else {
                    Some(now - chrono::Duration::seconds(i))
                },
                paused: false,
            })
            .collect();
        assert_eq!(offline_page(&mut rows, Some(" STUDIO ")), 105);
        assert_eq!(rows.len(), 100);
        assert_eq!(
            rows[0].last_seen_at,
            Some(now - chrono::Duration::seconds(1))
        );
        assert!(rows.iter().all(|row| row.last_seen_at.is_some()));
        assert_eq!(offline_page(&mut rows, Some("missing")), 0);
    }

    #[test]
    fn active_server_and_attention_serialization() {
        let server = ActiveServerDto {
            id: Uuid::nil(),
            name: "Uplink".into(),
            kind: StreamServerKindDto::Belabox,
            bitrate: None,
            rtt: None,
        };
        assert_eq!(
            serde_json::to_value(server).unwrap(),
            json!({"id": Uuid::nil(), "name": "Uplink", "kind": "belabox", "bitrate": null, "rtt": null})
        );
        assert_eq!(serde_json::to_value(Attention::NoFeed).unwrap(), "no_feed");
        assert_eq!(
            serde_json::to_value(Attention::ObsLost).unwrap(),
            "obs_lost"
        );
    }
}
