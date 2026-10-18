//! OBS's stats, like NOALBS's `!serverinfo`: bitrate, frame rate, dropped
//! frames by cause (network, rendering lag, encoding lag), CPU, memory, disk.

use std::time::Duration;

use live::ObsFrames;
use obs_actions::types::{GetStats, GetStreamStatus};
use uuid::Uuid;

use crate::{Control, Error, Result};

/// How far apart the two readings for a bitrate are, like NOALBS.
pub const BITRATE_WINDOW: Duration = Duration::from_secs(2);

/// Frames dropped (or skipped) for one cause, out of all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dropped {
    pub frames: u64,
    pub total: u64,
}

impl Dropped {
    pub fn percent(self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.frames as f64 / self.total as f64 * 100.0
        }
    }
}

/// One reading of OBS's stats.
#[derive(Debug, Clone, PartialEq)]
pub struct ObsStats {
    pub streaming: bool,
    pub reconnecting: bool,
    /// What the stream output sent so far, and for how long it has run: two
    /// readings give the bitrate ([`bitrate_kbps`]).
    pub stream_bytes: u64,
    pub stream_duration_ms: u64,
    /// 0–1: how far the stream output is behind.
    pub congestion: f64,
    /// Dropped by the stream output (this stream).
    pub network: Dropped,
    /// Missed due to rendering lag.
    pub rendering: Dropped,
    /// Skipped due to encoding lag.
    pub encoding: Dropped,
    /// `rendering` and `encoding` count this stream's frames; otherwise they
    /// count since OBS started (not streaming, or no switcher runs).
    pub this_stream: bool,
    pub fps: f64,
    /// ms per frame.
    pub frame_render_ms: f64,
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub disk_free_mb: f64,
}

impl ObsStats {
    /// `start`: the counters when the stream started, if known.
    pub fn new(stats: &GetStats, stream: &GetStreamStatus, start: Option<ObsFrames>) -> Self {
        let now = ObsFrames {
            render_total: stats.render_total_frames,
            render_skipped: stats.render_skipped_frames,
            output_total: stats.output_total_frames,
            output_skipped: stats.output_skipped_frames,
        };
        let start = start.filter(|_| stream.output_active);
        let frames = start.map_or(now, |start| now.since(start));
        Self {
            streaming: stream.output_active,
            reconnecting: stream.output_reconnecting,
            stream_bytes: stream.output_bytes,
            stream_duration_ms: u64::from(stream.output_duration),
            congestion: f64::from(stream.output_congestion),
            network: Dropped {
                frames: u64::from(stream.output_skipped_frames),
                total: u64::from(stream.output_total_frames),
            },
            rendering: Dropped {
                frames: frames.render_skipped,
                total: frames.render_total,
            },
            encoding: Dropped {
                frames: frames.output_skipped,
                total: frames.output_total,
            },
            this_stream: start.is_some(),
            fps: stats.active_fps,
            frame_render_ms: stats.average_frame_render_time,
            cpu_percent: stats.cpu_usage,
            memory_mb: stats.memory_usage,
            disk_free_mb: stats.available_disk_space,
        }
    }
}

/// The stream's bitrate between two readings, in kbps; `None` unless both are
/// of the same running stream.
pub fn bitrate_kbps(earlier: &ObsStats, later: &ObsStats) -> Option<u32> {
    if !earlier.streaming || !later.streaming {
        return None;
    }
    let bytes = later.stream_bytes.checked_sub(earlier.stream_bytes)?;
    let ms = later
        .stream_duration_ms
        .checked_sub(earlier.stream_duration_ms)
        .filter(|ms| *ms > 0)?;
    // bytes * 8 bits / ms = kbit/s.
    u32::try_from(bytes * 8 / ms).ok()
}

impl Control {
    /// One reading of OBS's stats. Allowed while paused: it only reads.
    pub async fn obs_stats(&self, switcher_id: Uuid) -> Result<ObsStats> {
        let status = self.status(switcher_id).await.filter(|s| s.obs_connected);
        let Some(status) = status else {
            return Err(Error::ObsOffline);
        };
        let obs = obs_actions::ObsClient::new(self.nats.clone(), switcher_id);
        let stats = obs.stats().await?;
        let stream = obs.stream_status().await?;
        Ok(ObsStats::new(&stats, &stream, status.stream_frames))
    }

    /// OBS's stats with the stream's bitrate over [`BITRATE_WINDOW`] (so this
    /// takes that long), like NOALBS's `!serverinfo`.
    pub async fn obs_stats_with_bitrate(
        &self,
        switcher_id: Uuid,
    ) -> Result<(ObsStats, Option<u32>)> {
        let earlier = self.obs_stats(switcher_id).await?;
        if !earlier.streaming {
            return Ok((earlier, None));
        }
        tokio::time::sleep(BITRATE_WINDOW).await;
        let later = self.obs_stats(switcher_id).await?;
        let bitrate = bitrate_kbps(&earlier, &later);
        Ok((later, bitrate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(render: (u64, u64), output: (u64, u64)) -> GetStats {
        GetStats {
            cpu_usage: 12.5,
            memory_usage: 512.0,
            available_disk_space: 20480.0,
            active_fps: 60.0,
            average_frame_render_time: 1.2,
            render_skipped_frames: render.0,
            render_total_frames: render.1,
            output_skipped_frames: output.0,
            output_total_frames: output.1,
        }
    }

    fn stream(active: bool, bytes: u64, duration: u32) -> GetStreamStatus {
        serde_json::from_value(serde_json::json!({
            "outputActive": active,
            "outputReconnecting": false,
            "outputTimecode": "00:00:00.000",
            "outputDuration": duration,
            "outputCongestion": 0.0,
            "outputBytes": bytes,
            "outputSkippedFrames": 3,
            "outputTotalFrames": 600,
        }))
        .unwrap()
    }

    #[test]
    fn counts_this_streams_frames_when_the_start_is_known() {
        let start = ObsFrames {
            render_total: 1000,
            render_skipped: 10,
            output_total: 1000,
            output_skipped: 2,
        };
        let reading = ObsStats::new(
            &stats((12, 1600), (4, 1600)),
            &stream(true, 0, 10_000),
            Some(start),
        );
        assert!(reading.this_stream);
        assert_eq!(
            reading.rendering,
            Dropped {
                frames: 2,
                total: 600
            }
        );
        assert_eq!(
            reading.encoding,
            Dropped {
                frames: 2,
                total: 600
            }
        );
        assert_eq!(
            reading.network,
            Dropped {
                frames: 3,
                total: 600
            }
        );
        assert_eq!(reading.network.percent(), 0.5);

        // Not streaming: a leftover start doesn't count, OBS's totals do.
        let idle = ObsStats::new(
            &stats((12, 1600), (4, 1600)),
            &stream(false, 0, 0),
            Some(start),
        );
        assert!(!idle.this_stream);
        assert_eq!(
            idle.rendering,
            Dropped {
                frames: 12,
                total: 1600
            }
        );
    }

    #[test]
    fn bitrate_between_two_readings() {
        let reading = |active, bytes, ms| {
            ObsStats::new(&stats((0, 0), (0, 0)), &stream(active, bytes, ms), None)
        };
        // 1.5 MB over 2 s = 6000 kbps.
        let earlier = reading(true, 10_000_000, 60_000);
        let later = reading(true, 11_500_000, 62_000);
        assert_eq!(bitrate_kbps(&earlier, &later), Some(6000));
        // A new stream in between (bytes went back), or no time passed.
        assert_eq!(bitrate_kbps(&later, &earlier), None);
        assert_eq!(bitrate_kbps(&later, &later), None);
        assert_eq!(bitrate_kbps(&earlier, &reading(false, 0, 0)), None);
        assert_eq!(
            Dropped {
                frames: 0,
                total: 0
            }
            .percent(),
            0.0
        );
    }
}
