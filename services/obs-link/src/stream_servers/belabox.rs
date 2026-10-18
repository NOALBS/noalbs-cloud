//! BELABOX Cloud: `GET stats_url` lists publishers; the stream is always `live`.

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

/// The publisher BELABOX Cloud reports the stream under.
const PUBLISHER: &str = "live";

/// The statistics returned by the Belabox stats endpoint
#[derive(serde::Deserialize, Debug, Clone)]
pub struct Stat {
    /// The bitrate of the stream in kbps
    pub bitrate: i64,

    /// The round-trip time of the stream in milliseconds
    pub rtt: f64,
}

pub struct Belabox {
    source: Source,
}

impl Belabox {
    pub async fn run(source: Source) {
        super::poll(Self { source }).await;
    }

    async fn stats(&self) -> Result<(Stat, serde_json::Value)> {
        let request = self
            .source
            .reqwest_client
            .get(&self.source.server.stats_url);
        parse(&fetch_body(request).await?, PUBLISHER)
    }
}

fn parse(body: &[u8], publisher: &str) -> Result<(Stat, serde_json::Value)> {
    let data: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    let publisher = &data["publishers"][publisher];
    if publisher.is_null() {
        return Err(Error::InvalidPublisher);
    }
    let stat = serde_json::from_value(publisher.to_owned()).map_err(|_| Error::Deserialize)?;

    Ok((stat, data))
}

pub fn switch_type(stats: &Stat, triggers: &Triggers) -> SwitchType {
    if let Some(offline) = triggers.offline
        && stats.bitrate > 0
        && stats.bitrate <= offline.into()
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && stats.rtt >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    if stats.bitrate == 0 {
        return SwitchType::Offline;
    }

    if stats.bitrate == 1 {
        return SwitchType::Previous;
    }

    if let Some(low) = triggers.low
        && stats.bitrate <= low.into()
    {
        return SwitchType::Low;
    }

    if let Some(rtt) = triggers.rtt
        && stats.rtt >= rtt.into()
    {
        return SwitchType::Low;
    }

    SwitchType::Normal
}

impl Fetcher for Belabox {
    const KIND: &'static str = "BELABOX";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let (stat, raw) = self.stats().await?;
        tracing::trace!("{:#?}", stat);
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        let bitrate = u32::try_from(stat.bitrate.max(0)).unwrap_or(u32::MAX);
        report(&self.source, bitrate, Some(stat.rtt), switch_type, raw);
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
    fn reads_the_live_publisher() {
        let body = br#"{"publishers":{"live":{"bitrate":6240,"rtt":182.0,"dropped_pkts":0}}}"#;
        let (stat, _) = parse(body, "live").unwrap();
        assert_eq!(stat.bitrate, 6240);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);
        assert!(matches!(parse(body, "other"), Err(Error::InvalidPublisher)));
        assert!(matches!(
            parse(br#"{"publishers":{}}"#, "live"),
            Err(Error::InvalidPublisher)
        ));
    }
}
