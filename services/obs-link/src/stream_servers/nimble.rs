//! Nimble Streamer: `stats_url` is the API base (e.g. `http://host:8082`).
//! `/manage/srt_receiver_stats` gives the SRT listener's state and RTT, and
//! `/manage/rtmp_status` the bitrate of the stream Nimble makes from it (the
//! SRT receiver's own rate is averaged and less useful), found by application
//! and stream name.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, join_url, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SrtStats {
    #[serde(default)]
    srt_receivers: Vec<SrtReceiver>,
}

#[derive(Debug, Deserialize)]
struct SrtReceiver {
    id: String,
    state: String,
    stats: SrtReceiverStats,
}

#[derive(Debug, Deserialize)]
struct SrtReceiverStats {
    link: SrtLink,
}

#[derive(Debug, Deserialize)]
struct SrtLink {
    rtt: f64,
}

#[derive(Debug, Deserialize)]
struct RtmpApp {
    app: String,
    #[serde(default)]
    streams: Vec<RtmpStream>,
}

#[derive(Debug, Deserialize)]
struct RtmpStream {
    strm: String,
    /// Bits per second, as a string.
    bandwidth: String,
}

#[derive(Debug, PartialEq)]
pub struct Stat {
    /// kbps
    pub bitrate: u32,
    /// ms
    pub rtt: f64,
}

pub struct Nimble {
    source: Source,
    /// The SRT listener's ID, usually `ip:port`.
    listener_id: String,
    application: String,
    key: String,
}

impl Nimble {
    pub async fn run(source: Source) {
        let details = source.server.details();
        super::poll(Self {
            listener_id: details.listener_id.unwrap_or_default(),
            application: details.application.unwrap_or_default(),
            key: details.key.unwrap_or_default(),
            source,
        })
        .await;
    }

    async fn stats(&self) -> Result<Stat> {
        let base = &self.source.server.stats_url;
        let client = &self.source.reqwest_client;

        let srt = fetch_body(client.get(join_url(base, &["manage", "srt_receiver_stats"]))).await?;
        let rtt = srt_rtt(&srt, &self.listener_id)?;

        let rtmp = fetch_body(client.get(join_url(base, &["manage", "rtmp_status"]))).await?;
        let bitrate = rtmp_bitrate(&rtmp, &self.application, &self.key)?;

        Ok(Stat { bitrate, rtt })
    }
}

fn srt_rtt(body: &[u8], listener_id: &str) -> Result<f64> {
    let stats: SrtStats = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    let receiver = stats
        .srt_receivers
        .into_iter()
        .find(|receiver| receiver.id.contains(listener_id))
        .ok_or(Error::InvalidPublisher)?;
    if receiver.state == "disconnected" {
        return Err(Error::NotReady);
    }

    Ok(receiver.stats.link.rtt)
}

fn rtmp_bitrate(body: &[u8], application: &str, key: &str) -> Result<u32> {
    let apps: Vec<RtmpApp> = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    let stream = apps
        .into_iter()
        .filter(|app| app.app == application)
        .flat_map(|app| app.streams)
        .find(|stream| stream.strm == key)
        .ok_or(Error::InvalidPublisher)?;
    let bits: u64 = stream.bandwidth.parse().map_err(|_| Error::Deserialize)?;

    Ok(u32::try_from(bits / 1024).unwrap_or(u32::MAX))
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    if let Some(offline) = triggers.offline
        && stat.bitrate > 0
        && stat.bitrate <= offline
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && stat.rtt >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    // As in NOALBS v2: Nimble reports 0 while the stream settles in.
    if stat.bitrate == 0 {
        return SwitchType::Normal;
    }

    if let Some(low) = triggers.low
        && stat.bitrate <= low
    {
        return SwitchType::Low;
    }

    if let Some(rtt) = triggers.rtt
        && stat.rtt >= rtt.into()
    {
        return SwitchType::Low;
    }

    SwitchType::Normal
}

impl Fetcher for Nimble {
    const KIND: &'static str = "Nimble";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let stat = self.stats().await?;
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        let raw = serde_json::json!({ "bitrate": stat.bitrate, "rtt": stat.rtt });
        report(&self.source, stat.bitrate, Some(stat.rtt), switch_type, raw);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Stat, rtmp_bitrate, srt_rtt, switch_type};
    use crate::{
        stream_servers::{Error, test_triggers as triggers},
        switcher::SwitchType,
    };

    const SRT: &str = r#"{"SrtReceivers":[
        {"id":"0.0.0.0:2020","state":"connected","stats":{"time":1,"window":{"flow":1,"congestion":1,"flight":1},
         "link":{"rtt":42.5,"mbpsBandwidth":20.0,"mbpsMaxBandwidth":1000},
         "recv":{"packetsReceived":1,"packetsLost":0,"packetsDropped":0,"packetsBelated":0,"NAKsSent":0,
                 "bytesReceived":1,"bytesLost":0,"bytesDropped":0,"mbpsRate":6.1}}},
        {"id":"0.0.0.0:3030","state":"disconnected","stats":{"link":{"rtt":0}}}
    ]}"#;

    const RTMP: &str = r#"[{"app":"live","streams":[
        {"acodec":"aac","vcodec":"avc1","publish_time":"1","bandwidth":"6291456","protocol":"SRT","resolution":"1920x1080","strm":"feed1"}
    ]}]"#;

    #[test]
    fn combines_srt_rtt_with_rtmp_bitrate() {
        assert_eq!(srt_rtt(SRT.as_bytes(), "0.0.0.0:2020").unwrap(), 42.5);
        assert!(matches!(
            srt_rtt(SRT.as_bytes(), "0.0.0.0:3030"),
            Err(Error::NotReady)
        ));
        assert!(matches!(
            srt_rtt(SRT.as_bytes(), "9.9.9.9:1"),
            Err(Error::InvalidPublisher)
        ));

        assert_eq!(
            rtmp_bitrate(RTMP.as_bytes(), "live", "feed1").unwrap(),
            6144
        );
        assert!(rtmp_bitrate(RTMP.as_bytes(), "live", "other").is_err());
    }

    #[test]
    fn switches_like_noalbs() {
        let stat = |bitrate, rtt| Stat { bitrate, rtt };
        assert_eq!(
            switch_type(&stat(6000, 40.0), &triggers()),
            SwitchType::Normal
        );
        assert_eq!(switch_type(&stat(600, 40.0), &triggers()), SwitchType::Low);
        assert_eq!(
            switch_type(&stat(100, 40.0), &triggers()),
            SwitchType::Offline
        );
        assert_eq!(
            switch_type(&stat(6000, 3000.0), &triggers()),
            SwitchType::Low
        );
        assert_eq!(switch_type(&stat(0, 40.0), &triggers()), SwitchType::Normal);
    }
}
