//! MediaMTX: `GET stats_url` is the path's API entry (`…/v3/paths/get/<path>`);
//! SRT publishers also get their connection stats (`…/v3/srtconns/get/<id>`)
//! for the RTT. Both requests use basic auth when the server has a username.

use std::time::{Duration, Instant};

use super::{Error, Fetcher, Result, Source, fetch_body, report, with_basic_auth};
use crate::switcher::{SwitchType, Triggers};

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamStats {
    // pub name: String,
    pub ready: bool,
    pub source: Option<PathSource>,
    pub bytes_received: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SrtStats {
    pub id: String,
    pub created: String,
    pub remote_addr: String,
    pub state: String,
    pub path: String,
    pub query: String,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub packets_sent_unique: u64,
    pub packets_received_unique: u64,
    pub packets_send_loss: u64,
    pub packets_received_loss: u64,
    pub packets_retrans: u64,
    pub packets_received_retrans: u64,
    #[serde(rename = "packetsSentACK")]
    pub packets_sent_ack: u64,
    #[serde(rename = "packetsReceivedACK")]
    pub packets_received_ack: u64,
    #[serde(rename = "packetsSentNAK")]
    pub packets_sent_nak: u64,
    #[serde(rename = "packetsReceivedNAK")]
    pub packets_received_nak: u64,
    #[serde(rename = "packetsSentKM")]
    pub packets_sent_km: u64,
    #[serde(rename = "packetsReceivedKM")]
    pub packets_received_km: u64,
    pub us_snd_duration: u64,
    pub packets_send_drop: u64,
    pub packets_received_drop: u64,
    pub packets_received_undecrypt: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent_unique: u64,
    pub bytes_received_unique: u64,
    pub bytes_received_loss: u64,
    pub bytes_retrans: u64,
    pub bytes_received_retrans: u64,
    pub bytes_send_drop: u64,
    pub bytes_received_drop: u64,
    pub bytes_received_undecrypt: u64,
    pub us_packets_send_period: f64,
    pub packets_flow_window: u64,
    pub packets_flight_size: u64,
    #[serde(rename = "msRTT")]
    pub ms_rtt: f64,
    pub mbps_send_rate: f64,
    pub mbps_receive_rate: f64,
    pub mbps_link_capacity: f64,
    pub bytes_avail_send_buf: u64,
    pub bytes_avail_receive_buf: u64,
    #[serde(rename = "mbpsMaxBW")]
    pub mbps_max_bw: f64,
    #[serde(rename = "byteMSS")]
    pub byte_mss: u64,
    pub packets_send_buf: u64,
    pub bytes_send_buf: u64,
    pub ms_send_buf: u64,
    pub ms_send_tsb_pd_delay: u64,
    pub packets_receive_buf: u64,
    pub bytes_receive_buf: u64,
    pub ms_receive_buf: u64,
    pub ms_receive_tsb_pd_delay: u64,
    pub packets_reorder_tolerance: u64,
    pub packets_received_avg_belated_time: u64,
    pub packets_send_loss_rate: f64,
    pub packets_received_loss_rate: f64,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PathSource {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub bitrate: u32,
    pub srt: Option<SrtStats>,
}

// #[derive(Serialize, serde::Deserialize, Debug)]
// pub struct Auth {
//     username: String,
//     password: String,
// }

/// Turns MediaMTX's running byte total into a bitrate. MediaMTX only reports
/// the total received since the publisher connected, so a bitrate needs two
/// readings; until then (and after the publisher reconnects) there is none.
#[derive(Debug, Default)]
pub struct Cache {
    /// The last total bytes received, and when it was read.
    last: Option<(u64, Instant)>,
    /// The latest measured bitrate in kbps.
    bitrate: Option<u32>,
}

impl Cache {
    /// Measure over at least this long, so a slightly early poll doesn't read
    /// as a burst or a drop.
    const MIN_WINDOW: Duration = Duration::from_millis(500);

    /// Records a reading; returns the bitrate once one has been measured.
    pub fn update(&mut self, bytes_received: u64, now: Instant) -> Option<u32> {
        let Some((prev_bytes, prev_at)) = self.last else {
            self.last = Some((bytes_received, now));
            return None;
        };
        if bytes_received < prev_bytes {
            // The publisher reconnected and the total started over.
            *self = Self::default();
            self.last = Some((bytes_received, now));
            return None;
        }

        let elapsed = now.saturating_duration_since(prev_at);
        if elapsed >= Self::MIN_WINDOW {
            let bits = (bytes_received - prev_bytes) as f64 * 8.0;
            self.bitrate = Some((bits / elapsed.as_secs_f64() / 1024.0) as u32);
            self.last = Some((bytes_received, now));
        }
        self.bitrate
    }

    /// Whether this total is the same as the last one read (nothing arrived).
    pub fn unchanged(&self, bytes_received: u64) -> bool {
        self.last.is_some_and(|(bytes, _)| bytes == bytes_received)
    }
}

pub struct Mediamtx {
    source: Source,
    cache: Cache,
}

impl Mediamtx {
    pub async fn run(source: Source) {
        super::poll(Self {
            source,
            cache: Cache::default(),
        })
        .await;
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        with_basic_auth(
            self.source.reqwest_client.get(url),
            &self.source.server.details(),
            &self.source.secrets,
        )
    }

    async fn stats(&mut self) -> Result<Stats> {
        let body = fetch_body(self.get(&self.source.server.stats_url)).await?;
        let stream =
            serde_json::from_slice::<StreamStats>(&body).map_err(|_| Error::Deserialize)?;

        if !stream.ready {
            return Err(Error::NotReady);
        }

        let mut stats = Stats::default();

        if let Some(source) = &stream.source
            && source.kind == "srtConn"
        {
            stats.srt = Some(self.get_srt_stats(&source.id).await?);
        }

        if self.cache.unchanged(stream.bytes_received) {
            return Err(Error::Skipped);
        }
        // No bitrate until two readings are compared; sending a guess (0, or the
        // whole stream's bytes over one second after a restart) would mislead.
        let Some(bitrate) = self.cache.update(stream.bytes_received, Instant::now()) else {
            return Err(Error::NotReady);
        };

        tracing::trace!("{:#?}", stream);
        stats.bitrate = bitrate;

        Ok(stats)
    }

    async fn get_srt_stats(&self, id: &str) -> Result<SrtStats> {
        let base = self
            .source
            .server
            .stats_url
            .split("/v3")
            .next()
            .ok_or(Error::NotAccessible)?;
        let body = fetch_body(self.get(&format!("{base}/v3/srtconns/get/{id}"))).await?;
        let stats = serde_json::from_slice::<SrtStats>(&body).map_err(|_| Error::Deserialize)?;

        tracing::trace!("{:#?}", stats);
        Ok(stats)
    }
}

pub fn switch_type(stats: &Stats, triggers: &Triggers) -> SwitchType {
    let ms_rtt = stats.srt.as_ref().map(|s| s.ms_rtt);

    if let Some(offline) = triggers.offline
        && stats.bitrate > 0
        && stats.bitrate <= offline
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && let Some(ms_rtt) = ms_rtt
        && ms_rtt >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    if stats.bitrate == 0 {
        return SwitchType::Previous;
    }

    if let Some(low) = triggers.low
        && stats.bitrate <= low
    {
        return SwitchType::Low;
    }

    if let Some(rtt) = triggers.rtt
        && let Some(ms_rtt) = ms_rtt
        && ms_rtt >= rtt.into()
    {
        return SwitchType::Low;
    }

    SwitchType::Normal
}

impl Fetcher for Mediamtx {
    const KIND: &'static str = "MediaMTX";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let stats = self.stats().await?;
        let switch_type = switch_type(&stats, &self.source.triggers().await);
        report(
            &self.source,
            stats.bitrate,
            stats.srt.as_ref().map(|srt| srt.ms_rtt),
            switch_type,
            serde_json::to_value(&stats).unwrap_or_default(),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::Cache;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn needs_two_readings_and_ignores_the_running_total() {
        let start = Instant::now();
        let mut cache = Cache::default();
        // A stream that has been live a while: 900 MB received so far.
        assert_eq!(cache.update(900 * MB, start), None);
        // 750 KB in the next second is 6,000 kbps, not 900 MB's worth.
        let bitrate = cache.update(900 * MB + 750 * 1024, start + Duration::from_secs(1));
        assert_eq!(bitrate, Some(6000));
    }

    #[test]
    fn early_polls_keep_the_last_bitrate() {
        let start = Instant::now();
        let mut cache = Cache::default();
        cache.update(0, start);
        cache.update(750 * 1024, start + Duration::from_secs(1));
        let early = cache.update(760 * 1024, start + Duration::from_millis(1100));
        assert_eq!(early, Some(6000));
    }

    #[test]
    fn a_reconnect_starts_over() {
        let start = Instant::now();
        let mut cache = Cache::default();
        cache.update(900 * MB, start);
        cache.update(900 * MB + 750 * 1024, start + Duration::from_secs(1));
        assert_eq!(cache.update(1024, start + Duration::from_secs(2)), None);
        assert_eq!(
            cache.update(1024 + 375 * 1024, start + Duration::from_secs(3)),
            Some(3000)
        );
    }

    #[test]
    fn unchanged_totals_are_spotted() {
        let mut cache = Cache::default();
        assert!(!cache.unchanged(10));
        cache.update(10, Instant::now());
        assert!(cache.unchanged(10));
    }
}
