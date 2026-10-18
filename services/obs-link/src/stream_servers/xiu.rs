//! Xiu: `POST stats_url` (its `query_stream` API) with the RTMP application and
//! stream name; the answer has the publisher's receive bitrate.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
struct XiuResponse {
    error_code: i32,
    #[serde(default)]
    data: Vec<XiuStreamInfo>,
}

#[derive(Debug, Deserialize)]
struct XiuStreamInfo {
    publisher: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct Stat {
    /// kbps
    #[serde(rename = "recv_bitrate(kbits/s)")]
    pub recv_bitrate: u64,
}

impl Stat {
    pub fn bitrate(&self) -> u32 {
        u32::try_from(self.recv_bitrate).unwrap_or(u32::MAX)
    }
}

pub struct Xiu {
    source: Source,
    query: serde_json::Value,
}

impl Xiu {
    pub async fn run(source: Source) {
        let details = source.server.details();
        let query = serde_json::json!({
            "identifier": {
                "rtmp": {
                    "app_name": details.application.unwrap_or_default(),
                    "stream_name": details.key.unwrap_or_default(),
                }
            }
        });
        super::poll(Self { source, query }).await;
    }

    async fn stats(&self) -> Result<(Stat, serde_json::Value)> {
        let request = self
            .source
            .reqwest_client
            .post(&self.source.server.stats_url)
            .json(&self.query);
        parse(&fetch_body(request).await?)
    }
}

fn parse(body: &[u8]) -> Result<(Stat, serde_json::Value)> {
    let response: XiuResponse = serde_json::from_slice(body).map_err(|_| Error::Deserialize)?;
    if response.error_code != 0 {
        return Err(Error::NotAccessible);
    }
    let publisher = response
        .data
        .into_iter()
        .next()
        .ok_or(Error::NotReady)?
        .publisher;
    let stat = serde_json::from_value(publisher.clone()).map_err(|_| Error::Deserialize)?;

    Ok((stat, publisher))
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    let bitrate = stat.bitrate();

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

impl Fetcher for Xiu {
    const KIND: &'static str = "Xiu";

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
    use super::{parse, switch_type};
    use crate::{
        stream_servers::{Error, test_triggers as triggers},
        switcher::SwitchType,
    };

    #[test]
    fn reads_the_first_publisher() {
        let body = br#"{"error_code":0,"desp":"succ","data":[{"subscriber_count":0,"publisher":{
            "id":"1","identifier":{"rtmp":{"app_name":"live","stream_name":"feed1"}},"start_time":"x",
            "recv_bitrate(kbits/s)":6000,"video":null,"audio":null}}]}"#;
        let (stat, _) = parse(body).unwrap();
        assert_eq!(stat.bitrate(), 6000);
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);

        assert!(matches!(
            parse(br#"{"error_code":0,"desp":"succ","data":[]}"#),
            Err(Error::NotReady)
        ));
        assert!(matches!(
            parse(br#"{"error_code":1,"desp":"no stream","data":[]}"#),
            Err(Error::NotAccessible)
        ));
    }
}
