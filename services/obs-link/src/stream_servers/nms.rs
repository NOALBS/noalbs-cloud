//! Node Media Server: `GET <stats_url>/<application>/<key>` (e.g.
//! `http://host:8000/api/streams/live/feed1`), with optional basic auth.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, join_url, report, with_basic_auth};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub is_live: bool,
    /// kbps
    #[serde(default)]
    pub bitrate: u64,
}

impl Stat {
    pub fn bitrate(&self) -> u32 {
        u32::try_from(self.bitrate).unwrap_or(u32::MAX)
    }
}

pub struct NodeMediaServer {
    source: Source,
    url: String,
}

impl NodeMediaServer {
    pub async fn run(source: Source) {
        let details = source.server.details();
        let url = join_url(
            &source.server.stats_url,
            &[
                details.application.as_deref().unwrap_or_default(),
                details.key.as_deref().unwrap_or_default(),
            ],
        );
        super::poll(Self { source, url }).await;
    }

    async fn stats(&self) -> Result<(Stat, serde_json::Value)> {
        let request = with_basic_auth(
            self.source.reqwest_client.get(&self.url),
            &self.source.server.details(),
            &self.source.secrets,
        );
        let body = fetch_body(request).await?;
        let raw: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| Error::Deserialize)?;
        let stat = serde_json::from_value(raw.clone()).map_err(|_| Error::Deserialize)?;

        Ok((stat, raw))
    }
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    let bitrate = stat.bitrate();

    if !stat.is_live {
        return SwitchType::Offline;
    }

    if let Some(offline) = triggers.offline
        && bitrate > 0
        && bitrate <= offline
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

    SwitchType::Normal
}

impl Fetcher for NodeMediaServer {
    const KIND: &'static str = "NMS";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let (stat, raw) = self.stats().await?;
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        report(&self.source, stat.bitrate(), None, switch_type, raw);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Stat, switch_type};
    use crate::{stream_servers::test_triggers as triggers, switcher::SwitchType};

    fn stat(json: &str) -> Stat {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn switches_on_live_state_and_bitrate() {
        let live = stat(r#"{"isLive":true,"viewers":3,"duration":120,"bitrate":6000}"#);
        assert_eq!(switch_type(&live, &triggers()), SwitchType::Normal);
        let low = stat(r#"{"isLive":true,"bitrate":500}"#);
        assert_eq!(switch_type(&low, &triggers()), SwitchType::Low);
        let gone = stat(r#"{"isLive":false,"bitrate":6000}"#);
        assert_eq!(switch_type(&gone, &triggers()), SwitchType::Offline);
        let starting = stat(r#"{"isLive":true,"bitrate":0}"#);
        assert_eq!(switch_type(&starting, &triggers()), SwitchType::Previous);
    }
}
