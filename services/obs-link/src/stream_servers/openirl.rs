//! OpenIRL: `GET stats_url` (e.g. `…/stats/play_<id>`); the `publisher` object is
//! only there while someone is publishing.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
pub struct Stat {
    /// kbps
    pub bitrate: u32,
    /// ms
    pub rtt: f64,
}

pub struct OpenIrl {
    source: Source,
}

impl OpenIrl {
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
    let data: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    // No publisher: the stream is offline.
    let publisher = data.get("publisher").cloned().ok_or(Error::NotReady)?;
    let stat = serde_json::from_value(publisher.clone()).map_err(|_| Error::Deserialize)?;

    Ok((stat, publisher))
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

    if stat.bitrate == 1 {
        return SwitchType::Previous;
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

impl Fetcher for OpenIrl {
    const KIND: &'static str = "OpenIRL";

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
    use super::{parse, switch_type};
    use crate::{
        stream_servers::{Error, test_triggers as triggers},
        switcher::SwitchType,
    };

    #[test]
    fn reads_the_publisher_when_live() {
        let body = br#"{"publisher":{"bitrate":5800,"buffer":1000,"dropped_pkts":0,"latency":2000,"rtt":35.1,"uptime":60}}"#;
        let (stat, _) = parse(body).unwrap();
        assert_eq!(stat.bitrate, 5800);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);

        assert!(matches!(parse(br#"{"status":"ok"}"#), Err(Error::NotReady)));

        let (zero, _) = parse(br#"{"publisher":{"bitrate":0,"rtt":0}}"#).unwrap();
        assert_eq!(switch_type(&zero, &triggers()), SwitchType::Offline);
    }
}
