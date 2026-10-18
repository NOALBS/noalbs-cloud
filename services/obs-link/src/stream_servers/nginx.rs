//! NGINX with the RTMP module: `GET stats_url` is the `rtmp_stat` XML page; the
//! stream is found by application and stream key. NGINX updates it every few
//! seconds, and a bitrate of 0 means the stream just started.

use serde::Deserialize;

use super::{Error, Fetcher, Result, Source, fetch_body, report};
use crate::switcher::{SwitchType, Triggers};

#[derive(Debug, Deserialize)]
struct RtmpStats {
    server: RtmpServer,
}

#[derive(Debug, Deserialize)]
struct RtmpServer {
    #[serde(default)]
    application: Vec<RtmpApplication>,
}

#[derive(Debug, Deserialize)]
struct RtmpApplication {
    name: String,
    live: Option<RtmpLive>,
}

#[derive(Debug, Deserialize)]
struct RtmpLive {
    #[serde(default)]
    stream: Vec<Stat>,
}

#[derive(Debug, Deserialize)]
pub struct Stat {
    pub name: String,
    /// Video bandwidth in bits per second.
    #[serde(default)]
    pub bw_video: u64,
    /// `<active/>` is present while the stream is being published.
    pub active: Option<()>,
}

impl Stat {
    pub fn bitrate(&self) -> u32 {
        u32::try_from(self.bw_video / 1024).unwrap_or(u32::MAX)
    }
}

pub struct Nginx {
    source: Source,
    application: String,
    key: String,
}

impl Nginx {
    pub async fn run(source: Source) {
        let details = source.server.details();
        super::poll(Self {
            application: details.application.unwrap_or_default(),
            key: details.key.unwrap_or_default(),
            source,
        })
        .await;
    }

    async fn stats(&self) -> Result<Stat> {
        let request = self
            .source
            .reqwest_client
            .get(&self.source.server.stats_url);
        parse(&fetch_body(request).await?, &self.application, &self.key)
    }
}

fn parse(body: &[u8], application: &str, key: &str) -> Result<Stat> {
    let text = std::str::from_utf8(body).map_err(|_| Error::Deserialize)?;
    let stats: RtmpStats = quick_xml::de::from_str(text).map_err(|_| Error::Deserialize)?;

    stats
        .server
        .application
        .into_iter()
        .filter(|app| app.name == application)
        .filter_map(|app| app.live)
        .flat_map(|live| live.stream)
        .find(|stream| stream.name == key)
        .ok_or(Error::InvalidPublisher)
}

pub fn switch_type(stat: &Stat, triggers: &Triggers) -> SwitchType {
    let bitrate = stat.bitrate();

    if stat.active.is_none() {
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

impl Fetcher for Nginx {
    const KIND: &'static str = "NGINX";

    fn source(&self) -> &Source {
        &self.source
    }

    async fn fetch(&mut self) -> Result<()> {
        let stat = self.stats().await?;
        let switch_type = switch_type(&stat, &self.source.triggers().await);
        let raw = serde_json::json!({ "name": stat.name, "bw_video": stat.bw_video, "active": stat.active.is_some() });
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

    const STATS: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<?xml-stylesheet type="text/xsl" href="stat.xsl" ?>
<rtmp>
<nginx_version>1.25.3</nginx_version>
<uptime>3600</uptime>
<server>
<application>
<name>publish</name>
<live>
<stream>
<name>live</name>
<time>12000</time>
<bw_in>6400000</bw_in>
<bw_video>6144000</bw_video>
<client><id>1</id><address>203.0.113.7</address><publishing/><active/></client>
<meta><video><width>1920</width><height>1080</height></video><audio><codec>AAC</codec></audio></meta>
<nclients>1</nclients>
<publishing/>
<active/>
</stream>
<stream>
<name>idle</name>
<bw_video>0</bw_video>
</stream>
<nclients>1</nclients>
</live>
</application>
<application>
<name>other</name>
<live><nclients>0</nclients></live>
</application>
</server>
</rtmp>"#;

    #[test]
    fn finds_the_stream_by_application_and_key() {
        let stat = parse(STATS.as_bytes(), "publish", "live").unwrap();
        assert_eq!(stat.bitrate(), 6000);
        assert!(stat.active.is_some());
        assert_eq!(switch_type(&stat, &triggers()), SwitchType::Normal);

        let idle = parse(STATS.as_bytes(), "publish", "idle").unwrap();
        assert_eq!(switch_type(&idle, &triggers()), SwitchType::Offline);

        assert!(matches!(
            parse(STATS.as_bytes(), "other", "live"),
            Err(Error::InvalidPublisher)
        ));
    }
}
