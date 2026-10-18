//! RIST receiver: `GET stats_url` has live per-peer stats; bitrates are summed
//! and RTTs averaged over the peers (bonded links).

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
struct RistStats {
    #[serde(rename = "receiver-stats")]
    receiver_stats: Option<ReceiverStats>,
}

#[derive(Debug, Deserialize)]
struct ReceiverStats {
    flowinstant: Flowinstant,
}

#[derive(Debug, Deserialize)]
struct Flowinstant {
    #[serde(default)]
    peers: Vec<Peer>,
}

#[derive(Debug, Deserialize)]
struct Peer {
    stats: PeerStats,
}

#[derive(Debug, Deserialize)]
struct PeerStats {
    rtt: f64,
    /// bits per second
    bitrate: u64,
}

#[derive(Debug, PartialEq)]
pub struct Stat {
    /// kbps, all peers together
    pub bitrate: u32,
    /// ms, averaged over the peers
    pub rtt: f64,
}

pub struct Rist {
    source: Source,
}

impl Rist {
    pub async fn run(source: Source) {
        super::poll(Self { source }).await;
    }

    async fn stats(&self) -> Result<(Stat, serde_json::Value)> {
        let request = self
            .source
            .reqwest_client
            .get(&self.source.server.stats_url);
        parse(&fetch_body(request).await?)
    }
}

fn parse(body: &[u8]) -> Result<(Stat, serde_json::Value)> {
    let raw: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    let stats: RistStats = serde_json::from_value(raw.clone()).map_err(|_| Error::Deserialize)?;
    let peers = stats
        .receiver_stats
        .ok_or(Error::NotReady)?
        .flowinstant
        .peers;
    if peers.is_empty() {
        return Err(Error::NotReady);
    }

    let bits: u64 = peers.iter().map(|peer| peer.stats.bitrate).sum();
    let rtt = peers.iter().map(|peer| peer.stats.rtt).sum::<f64>() / peers.len() as f64;
    let stat = Stat {
        bitrate: u32::try_from(bits / 1024).unwrap_or(u32::MAX),
        rtt,
    };

    Ok((stat, raw))
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

    if stat.bitrate == 0 {
        return SwitchType::Offline;
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

impl Fetcher for Rist {
    const KIND: &'static str = "RIST";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let (stat, raw) = self.stats().await?;
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        report(&self.source, stat.bitrate, Some(stat.rtt), switch_type, raw);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Stat, parse, switch_type};
    use crate::{
        stream_servers::{Error, test_triggers as triggers},
        switcher::SwitchType,
    };

    #[test]
    fn sums_bitrates_and_averages_rtt_over_peers() {
        let body = br#"{"receiver-stats":{"flowinstant":{"peers":[
            {"stats":{"rtt":40.0,"avg_rtt":40.0,"bitrate":3145728,"avg_bitrate":3145728}},
            {"stats":{"rtt":60.0,"avg_rtt":60.0,"bitrate":3145728,"avg_bitrate":3145728}}
        ]}}}"#;
        let (stat, _) = parse(body).unwrap();
        assert_eq!(
            stat,
            Stat {
                bitrate: 6144,
                rtt: 50.0
            }
        );
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);

        assert!(matches!(
            parse(br#"{"sender-stats":{}}"#),
            Err(Error::NotReady)
        ));
        assert!(matches!(
            parse(br#"{"receiver-stats":{"flowinstant":{"peers":[]}}}"#),
            Err(Error::NotReady)
        ));
    }
}
