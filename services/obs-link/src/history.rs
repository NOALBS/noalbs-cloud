//! Stream history: collects a streaming switcher's samples into
//! [`db::streams::BUCKET_SECONDS`] buckets per server, which the switcher writes
//! to the database as each bucket fills (see `db::streams`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use db::streams::{BUCKET_SECONDS, Bucket, StateSeconds};
use live::{FeedState, ServerSample, StatsSample};
use uuid::Uuid;

/// What to write for one stream: finished buckets and the switcher's state seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Flush {
    pub stream_id: Uuid,
    pub buckets: Vec<Bucket>,
    pub states: StateSeconds,
    pub last_sample_at: DateTime<Utc>,
}

impl Flush {
    pub async fn write(self, pool: &db::DbPool) {
        if let Err(error) = db::streams::write_buckets(
            pool,
            self.stream_id,
            &self.buckets,
            self.states,
            self.last_sample_at,
        )
        .await
        {
            tracing::warn!(%error, stream_id = %self.stream_id, "Failed to write stream history");
        }
    }
}

/// The current bucket of one stream.
#[derive(Debug)]
pub struct Recorder {
    stream_id: Uuid,
    bucket_at: Option<DateTime<Utc>>,
    servers: HashMap<Uuid, Acc>,
    states: StateSeconds,
    last_sample_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct Acc {
    samples: i32,
    bitrate_sum: i64,
    bitrate_min: i32,
    bitrate_max: i32,
    rtt_sum: f64,
    rtt_samples: i32,
    rtt_max: Option<f64>,
    live: i32,
    low: i32,
    offline: i32,
    raw: Option<serde_json::Value>,
}

impl Acc {
    fn add(&mut self, sample: &ServerSample, raw: Option<&serde_json::Value>) {
        let bitrate = i32::try_from(sample.bitrate).unwrap_or(i32::MAX);
        if self.samples == 0 {
            self.bitrate_min = bitrate;
            self.bitrate_max = bitrate;
        } else {
            self.bitrate_min = self.bitrate_min.min(bitrate);
            self.bitrate_max = self.bitrate_max.max(bitrate);
        }
        self.samples += 1;
        self.bitrate_sum += i64::from(bitrate);
        if let Some(rtt) = sample.rtt.filter(|rtt| rtt.is_finite()) {
            self.rtt_sum += rtt;
            self.rtt_samples += 1;
            self.rtt_max = Some(self.rtt_max.map_or(rtt, |max| max.max(rtt)));
        }
        count_state(
            sample.state,
            &mut self.live,
            &mut self.low,
            &mut self.offline,
        );
        if let Some(raw) = raw {
            self.raw = Some(raw.clone());
        }
    }

    fn into_bucket(self, server_id: Uuid, bucket_at: DateTime<Utc>) -> Bucket {
        Bucket {
            server_id,
            bucket_at,
            samples: self.samples,
            bitrate_sum: self.bitrate_sum,
            bitrate_min: self.bitrate_min,
            bitrate_max: self.bitrate_max,
            rtt_sum: self.rtt_sum,
            rtt_samples: self.rtt_samples,
            rtt_max: self.rtt_max,
            live_seconds: self.live,
            low_seconds: self.low,
            offline_seconds: self.offline,
            raw: self.raw,
        }
    }
}

fn count_state(state: FeedState, live: &mut i32, low: &mut i32, offline: &mut i32) {
    match state {
        FeedState::Live => *live += 1,
        FeedState::Low => *low += 1,
        FeedState::Offline => *offline += 1,
        FeedState::Idle => {}
    }
}

/// The start of the bucket `at` falls in.
fn bucket_start(at: DateTime<Utc>) -> DateTime<Utc> {
    let seconds = at.timestamp();
    let start = seconds - seconds.rem_euclid(BUCKET_SECONDS);
    DateTime::from_timestamp(start, 0).unwrap_or(at)
}

impl Recorder {
    pub fn new(stream_id: Uuid) -> Self {
        Self {
            stream_id,
            bucket_at: None,
            servers: HashMap::new(),
            states: StateSeconds::default(),
            last_sample_at: None,
        }
    }

    pub fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    /// Adds one second: every server's sample and raw stats (for servers with
    /// fresh ones), and the switcher's decided `state`. Returns the finished
    /// bucket when `sample` starts a new one.
    pub fn add(
        &mut self,
        sample: &StatsSample,
        raw: &HashMap<Uuid, serde_json::Value>,
        state: FeedState,
    ) -> Option<Flush> {
        let bucket_at = bucket_start(sample.at);
        let finished = match self.bucket_at {
            Some(current) if current != bucket_at => self.take(),
            _ => None,
        };
        self.bucket_at = Some(bucket_at);

        for server in &sample.servers {
            self.servers
                .entry(server.server_id)
                .or_default()
                .add(server, raw.get(&server.server_id));
        }
        let states = &mut self.states;
        count_state(
            state,
            &mut states.live,
            &mut states.low,
            &mut states.offline,
        );
        self.last_sample_at = Some(sample.at);

        finished
    }

    /// Everything collected since the last flush, e.g. when the stream stops.
    pub fn take(&mut self) -> Option<Flush> {
        let last_sample_at = self.last_sample_at.take()?;
        let bucket_at = self.bucket_at.take()?;
        let buckets = self
            .servers
            .drain()
            .map(|(server_id, acc)| acc.into_bucket(server_id, bucket_at))
            .collect();
        Some(Flush {
            stream_id: self.stream_id,
            buckets,
            states: std::mem::take(&mut self.states),
            last_sample_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{DateTime, TimeDelta, Utc};
    use live::{FeedState, ServerSample, StatsSample};
    use uuid::Uuid;

    use super::{Recorder, bucket_start};

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000 + seconds, 250_000_000).unwrap()
    }

    fn sample(seconds: i64, server_id: Uuid, bitrate: u32, rtt: Option<f64>) -> StatsSample {
        StatsSample {
            at: at(seconds),
            servers: vec![ServerSample {
                server_id,
                bitrate,
                rtt,
                state: if bitrate > 1000 {
                    FeedState::Live
                } else {
                    FeedState::Low
                },
            }],
        }
    }

    #[test]
    fn buckets_start_on_whole_ten_seconds() {
        assert_eq!(bucket_start(at(0)), at(0) - TimeDelta::milliseconds(250));
        assert_eq!(bucket_start(at(7)), bucket_start(at(0)));
        assert_eq!(
            bucket_start(at(10)),
            bucket_start(at(0)) + TimeDelta::seconds(10)
        );
    }

    #[test]
    fn a_bucket_is_flushed_when_the_next_one_starts() {
        let server = Uuid::new_v4();
        let mut recorder = Recorder::new(Uuid::new_v4());
        let raw = HashMap::from([(server, serde_json::json!({ "n": 1 }))]);

        for (second, bitrate, rtt) in [
            (0, 5000, Some(80.0)),
            (1, 800, None),
            (2, 6000, Some(120.0)),
        ] {
            let flush = recorder.add(&sample(second, server, bitrate, rtt), &raw, FeedState::Live);
            assert!(flush.is_none());
        }
        let raw = HashMap::from([(server, serde_json::json!({ "n": 2 }))]);
        let flush = recorder
            .add(&sample(10, server, 4000, None), &raw, FeedState::Low)
            .expect("the first bucket is done");

        assert_eq!(flush.buckets.len(), 1);
        let bucket = &flush.buckets[0];
        assert_eq!(bucket.bucket_at, bucket_start(at(0)));
        assert_eq!(bucket.samples, 3);
        assert_eq!(bucket.bitrate_sum, 11_800);
        assert_eq!((bucket.bitrate_min, bucket.bitrate_max), (800, 6000));
        assert_eq!((bucket.rtt_sum, bucket.rtt_samples), (200.0, 2));
        assert_eq!(bucket.rtt_max, Some(120.0));
        assert_eq!((bucket.live_seconds, bucket.low_seconds), (2, 1));
        assert_eq!(bucket.raw, Some(serde_json::json!({ "n": 1 })));
        assert_eq!(flush.states.live, 3);
        assert_eq!(flush.last_sample_at, at(2));

        // The second bucket holds only the new sample.
        let rest = recorder.take().expect("one sample left");
        assert_eq!(rest.buckets[0].samples, 1);
        assert_eq!(rest.buckets[0].raw, Some(serde_json::json!({ "n": 2 })));
        assert_eq!(rest.states.low, 1);
        assert!(recorder.take().is_none());
    }

    #[test]
    fn servers_without_fresh_stats_keep_no_raw_and_idle_isnt_counted() {
        let server = Uuid::new_v4();
        let mut recorder = Recorder::new(Uuid::new_v4());
        recorder.add(
            &sample(0, server, 0, None),
            &HashMap::new(),
            FeedState::Idle,
        );

        let flush = recorder.take().unwrap();
        assert_eq!(flush.buckets[0].raw, None);
        assert_eq!(flush.buckets[0].rtt_max, None);
        assert_eq!(flush.states, Default::default());
    }
}
