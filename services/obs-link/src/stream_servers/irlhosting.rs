//! IRLHosting: `GET stats_url` answers for either an SRT or an RTMP service
//! (`"service"` says which). SRT streams are found by publisher (stream ID),
//! RTMP ones by application and stream key.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
#[serde(tag = "service")]
enum Stats {
    #[serde(rename = "SRT")]
    Srt {
        #[serde(default)]
        publishers: Vec<SrtPublisher>,
    },
    #[serde(rename = "RTMP")]
    Rtmp {
        #[serde(default)]
        applications: Vec<RtmpApplication>,
    },
}

#[derive(Debug, Deserialize)]
struct SrtPublisher {
    stream: String,
    /// kbps
    bitrate: u64,
    /// ms
    rtt: f64,
}

#[derive(Debug, Deserialize)]
struct RtmpApplication {
    name: String,
    live: RtmpLive,
}

#[derive(Debug, Deserialize)]
struct RtmpLive {
    #[serde(default)]
    streams: Vec<RtmpStream>,
}

#[derive(Debug, Deserialize)]
struct RtmpStream {
    name: String,
    bytes: RtmpBytes,
}

#[derive(Debug, Deserialize)]
struct RtmpBytes {
    incoming: u64,
}

/// The stream's numbers; RTMP has no RTT.
#[derive(Debug, PartialEq)]
pub enum Stat {
    Srt { bitrate: u32, rtt: f64 },
    Rtmp { bitrate: u32 },
}

impl Stat {
    pub fn bitrate(&self) -> u32 {
        match self {
            Self::Srt { bitrate, .. } | Self::Rtmp { bitrate } => *bitrate,
        }
    }

    pub fn rtt(&self) -> Option<f64> {
        match self {
            Self::Srt { rtt, .. } => Some(*rtt),
            Self::Rtmp { .. } => None,
        }
    }
}

/// Which stream to read: an SRT publisher, or an RTMP application + key.
#[derive(Debug, Default)]
pub struct Target {
    pub publisher: Option<String>,
    pub application: Option<String>,
    pub key: Option<String>,
}

pub struct Irlhosting {
    source: Source,
    target: Target,
}

impl Irlhosting {
    pub async fn run(source: Source) {
        let details = source.server.details();
        let target = Target {
            publisher: details.publisher,
            application: details.application,
            key: details.key,
        };
        super::poll(Self { source, target }).await;
    }

    async fn stats(&self) -> Result<(Stat, serde_json::Value)> {
        let request = self
            .source
            .reqwest_client
            .get(&self.source.server.stats_url);
        parse(&fetch_body(request).await?, &self.target)
    }
}

fn parse(body: &[u8], target: &Target) -> Result<(Stat, serde_json::Value)> {
    let raw: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    let stats: Stats = serde_json::from_value(raw.clone()).map_err(|_| Error::Deserialize)?;

    let stat = match stats {
        Stats::Srt { publishers } => {
            let wanted = target.publisher.as_deref().ok_or(Error::InvalidPublisher)?;
            let publisher = publishers
                .into_iter()
                .find(|publisher| publisher.stream == wanted)
                .ok_or(Error::InvalidPublisher)?;
            Stat::Srt {
                bitrate: u32::try_from(publisher.bitrate).unwrap_or(u32::MAX),
                rtt: publisher.rtt,
            }
        }
        Stats::Rtmp { applications } => {
            let (Some(application), Some(key)) = (&target.application, &target.key) else {
                return Err(Error::InvalidPublisher);
            };
            let stream = applications
                .into_iter()
                .filter(|app| app.name == *application)
                .flat_map(|app| app.live.streams)
                .find(|stream| stream.name == *key)
                .ok_or(Error::InvalidPublisher)?;
            // As NOALBS v2 reads it.
            Stat::Rtmp {
                bitrate: u32::try_from(stream.bytes.incoming / 1024).unwrap_or(u32::MAX),
            }
        }
    };

    Ok((stat, raw))
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    let bitrate = stat.bitrate();

    if let Some(offline) = triggers.offline
        && bitrate > 0
        && bitrate <= offline
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && let Some(rtt) = stat.rtt()
        && rtt >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    if bitrate == 0 {
        return SwitchType::Previous;
    }

    if let Some(low) = triggers.low
        && bitrate <= low
    {
        return SwitchType::Low;
    }

    if let Some(rtt_low) = triggers.rtt
        && let Some(rtt) = stat.rtt()
        && rtt >= rtt_low.into()
    {
        return SwitchType::Low;
    }

    SwitchType::Normal
}

impl Fetcher for Irlhosting {
    const KIND: &'static str = "IRLHosting";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let (stat, raw) = self.stats().await?;
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        report(&self.source, stat.bitrate(), stat.rtt(), switch_type, raw);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Stat, Target, parse, switch_type};
    use crate::{
        stream_servers::{Error, test_triggers as triggers},
        switcher::SwitchType,
    };

    #[test]
    fn reads_srt_publishers() {
        let body = br#"{"service":"SRT","status":"ok","timestamp":"x","publishers":[
            {"stream":"publish/live/feed1","bitrate":5500,"bitrate_mbps":"5.5","bytesReceived":1,"bytesDropped":0,
             "bytesLost":0,"mbpsBandwidth":20.0,"mbpsRecvRate":5.5,"rtt":3100.0,"uptime":"1m",
             "packetStats":{"packetsDropped":0,"packetsLost":0},"buffer":"1000"}],
            "overall":{"totalPublishers":1}}"#;
        let target = Target {
            publisher: Some("publish/live/feed1".into()),
            ..Target::default()
        };
        let (stat, _) = parse(body, &target).unwrap();
        assert_eq!(
            stat,
            Stat::Srt {
                bitrate: 5500,
                rtt: 3100.0
            }
        );
        // RTT over the LOW trigger.
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Low);

        assert!(matches!(
            parse(body, &Target::default()),
            Err(Error::InvalidPublisher)
        ));
    }

    #[test]
    fn reads_rtmp_streams() {
        let body = br#"{"service":"RTMP","timestamp":"x","server":{},"applications":[
            {"name":"publish","live":{"totalClients":1,"streams":[
                {"name":"live","duration":"1m","bandwidth":{"incoming":"6M","outgoing":"0"},
                 "bytes":{"incoming":6291456,"outgoing":0},"status":"live"}]}}]}"#;
        let target = Target {
            application: Some("publish".into()),
            key: Some("live".into()),
            ..Target::default()
        };
        let (stat, _) = parse(body, &target).unwrap();
        assert_eq!(stat, Stat::Rtmp { bitrate: 6144 });
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);
    }
}
