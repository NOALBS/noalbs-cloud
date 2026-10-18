//! OpenRTMP: `GET stats_url` (`…/stats?key=<stats key>`) answers JSON while the
//! stream is live and plain text while it's offline.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
pub struct Stat {
    pub bitrate_kbps: f64,
    pub rtt_ms: f64,
}

impl Stat {
    pub fn bitrate(&self) -> u32 {
        self.bitrate_kbps.max(0.0).round() as u32
    }
}

pub struct OpenRtmp {
    source: Source,
}

impl OpenRtmp {
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
    // Not JSON: the stream is offline.
    let raw: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::NotReady)?;
    let stat = serde_json::from_value(raw.clone()).map_err(|_| Error::Deserialize)?;

    Ok((stat, raw))
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    if let Some(offline) = triggers.offline
        && stat.bitrate_kbps > 0.0
        && stat.bitrate_kbps <= offline.into()
    {
        return SwitchType::Offline;
    }

    if let Some(rtt_offline) = triggers.rtt_offline
        && stat.rtt_ms >= rtt_offline.into()
    {
        return SwitchType::Offline;
    }

    if stat.bitrate_kbps == 0.0 {
        return SwitchType::Previous;
    }

    if let Some(low) = triggers.low
        && stat.bitrate_kbps <= low.into()
    {
        return SwitchType::Low;
    }

    if let Some(rtt) = triggers.rtt
        && stat.rtt_ms >= rtt.into()
    {
        return SwitchType::Low;
    }

    SwitchType::Normal
}

impl Fetcher for OpenRtmp {
    const KIND: &'static str = "OpenRTMP";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let (stat, raw) = self.stats().await?;
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        report(
            &self.source,
            stat.bitrate(),
            Some(stat.rtt_ms),
            switch_type,
            raw,
        );
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
    fn json_while_live_text_while_offline() {
        let body = br#"{"uptime":90,"bitrate_kbps":6012.4,"rtt_ms":48.0,"bytes_in":123,"video":null,"audio":null}"#;
        let (stat, _) = parse(body).unwrap();
        assert_eq!(stat.bitrate(), 6012);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);

        assert!(matches!(parse(b"stream offline"), Err(Error::NotReady)));
    }
}
