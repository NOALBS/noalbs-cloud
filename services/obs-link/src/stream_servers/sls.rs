//! SRT Live Server (SLS): `GET stats_url` lists publishers by stream ID. An
//! optional API key goes in the `Authorization` header.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
pub struct Stat {
    /// kbps
    pub bitrate: i64,
    /// ms
    pub rtt: f64,
}

pub struct SrtLiveServer {
    source: Source,
    /// The stream ID being published, e.g. `publish/live/feed1`.
    publisher: String,
}

impl SrtLiveServer {
    pub async fn run(source: Source) {
        let publisher = source.server.details().publisher.unwrap_or_default();
        super::poll(Self { source, publisher }).await;
    }

    async fn stats(&self) -> Result<(Stat, serde_json::Value)> {
        let mut request = self
            .source
            .reqwest_client
            .get(&self.source.server.stats_url);
        if let Some(api_key) = &self.source.secrets.api_key {
            request = request.header(reqwest::header::AUTHORIZATION, api_key);
        }

        parse(&fetch_body(request).await?, &self.publisher)
    }
}

fn parse(body: &[u8], publisher: &str) -> Result<(Stat, serde_json::Value)> {
    let data: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    let publisher = data["publishers"][publisher].clone();
    if publisher.is_null() {
        return Err(Error::InvalidPublisher);
    }
    let stat = serde_json::from_value(publisher.clone()).map_err(|_| Error::Deserialize)?;

    Ok((stat, publisher))
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    if let Some(offline) = triggers.offline
        && stat.bitrate > 0
        && stat.bitrate <= offline.into()
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && stat.rtt >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    if stat.bitrate == 0 {
        return SwitchType::Previous;
    }

    if let Some(low) = triggers.low
        && stat.bitrate <= low.into()
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

impl Fetcher for SrtLiveServer {
    const KIND: &'static str = "SLS";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let (stat, raw) = self.stats().await?;
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
    fn reads_the_publisher_and_switches() {
        let body =
            br#"{"publishers":{"publish/live/feed1":{"bitrate":6000,"rtt":120.5,"uptime":10}}}"#;
        let (stat, _) = parse(body, "publish/live/feed1").unwrap();
        assert_eq!(stat.bitrate, 6000);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);

        assert!(matches!(
            parse(body, "publish/live/other"),
            Err(Error::InvalidPublisher)
        ));

        let low = parse(br#"{"publishers":{"p":{"bitrate":600,"rtt":80}}}"#, "p")
            .unwrap()
            .0;
        assert_eq!(switch_type(&low, &triggers()), SwitchType::Low);
        let laggy = parse(br#"{"publishers":{"p":{"bitrate":6000,"rtt":3000}}}"#, "p")
            .unwrap()
            .0;
        assert_eq!(switch_type(&laggy, &triggers()), SwitchType::Low);
        let empty = parse(br#"{"publishers":{"p":{"bitrate":0,"rtt":0}}}"#, "p")
            .unwrap()
            .0;
        assert_eq!(switch_type(&empty, &triggers()), SwitchType::Previous);
        let dying = parse(br#"{"publishers":{"p":{"bitrate":100,"rtt":90}}}"#, "p")
            .unwrap()
            .0;
        assert_eq!(switch_type(&dying, &triggers()), SwitchType::Offline);
    }
}
