use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{Local, NaiveDateTime};
use flate2::{Compression, write::GzEncoder};
use serde::{Deserialize, Serialize};

pub const MAX_GZIP_BASE64_BYTES: usize = 3 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsLogFile {
    pub path: PathBuf,
    pub modified_at: Option<SystemTime>,
}

pub fn select_obs_log_file(
    files: &[ObsLogFile],
    obs_start_time: Option<SystemTime>,
) -> Option<&ObsLogFile> {
    if let Some(start_time) = obs_start_time {
        let earliest = start_time
            .checked_sub(Duration::from_secs(10))
            .unwrap_or(UNIX_EPOCH);
        return files
            .iter()
            .filter_map(|file| Some((file, obs_log_filename_time(&file.path)?)))
            .filter(|(_, time)| *time >= earliest)
            .min_by_key(|(_, time)| *time)
            .map(|(file, _)| file);
    }
    files
        .iter()
        .filter_map(|file| Some((file, file.modified_at?)))
        .max_by_key(|(_, modified)| *modified)
        .map(|(file, _)| file)
}

fn obs_log_filename_time(path: &Path) -> Option<SystemTime> {
    let name = path.file_stem()?.to_str()?;
    let local = NaiveDateTime::parse_from_str(name, "%Y-%m-%d %H-%M-%S")
        .ok()?
        .and_local_timezone(Local)
        .single()?;
    Some(local.with_timezone(&chrono::Utc).into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectLogUpload {
    pub kind: String,
    pub file_name: String,
    pub modified_at: Option<String>,
    pub truncated: bool,
    pub content_gzip_base64: String,
}

pub fn encode_log_upload(
    kind: &str,
    file_name: &str,
    modified_at: Option<String>,
    content: &[u8],
) -> std::io::Result<ConnectLogUpload> {
    let compressed = gzip_base64(content)?;
    if compressed.len() <= MAX_GZIP_BASE64_BYTES {
        return Ok(ConnectLogUpload {
            kind: kind.to_owned(),
            file_name: file_name.to_owned(),
            modified_at,
            truncated: false,
            content_gzip_base64: compressed,
        });
    }
    let mut starts = content
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1))
        .filter(|start| *start < content.len())
        .collect::<Vec<_>>();
    starts.push(content.len());
    let (mut low, mut high) = (0, starts.len());
    while low < high {
        let middle = low + (high - low) / 2;
        if gzip_base64(&content[starts[middle]..])?.len() <= MAX_GZIP_BASE64_BYTES {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let mut start = starts.get(low).copied().unwrap_or(content.len());
    let mut tail = gzip_base64(&content[start..])?;
    while tail.len() > MAX_GZIP_BASE64_BYTES && start < content.len() {
        start = content[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(content.len(), |offset| start + offset + 1);
        tail = gzip_base64(&content[start..])?;
    }
    if tail.len() > MAX_GZIP_BASE64_BYTES || (start == content.len() && !content.is_empty()) {
        let (mut low, mut high) = (0, content.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if gzip_base64(&content[middle..])?.len() <= MAX_GZIP_BASE64_BYTES {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        tail = gzip_base64(&content[low..])?;
    }
    Ok(ConnectLogUpload {
        kind: kind.to_owned(),
        file_name: file_name.to_owned(),
        modified_at,
        truncated: true,
        content_gzip_base64: tail,
    })
}

fn gzip_base64(content: &[u8]) -> std::io::Result<String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(content)?;
    Ok(STANDARD.encode(encoder.finish()?))
}

#[cfg(test)]
mod tests {
    use std::{
        io::Read,
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use base64::{Engine, engine::general_purpose::STANDARD};
    use flate2::read::GzDecoder;

    use super::{MAX_GZIP_BASE64_BYTES, ObsLogFile, encode_log_upload, select_obs_log_file};

    fn file(name: &str, modified: u64) -> ObsLogFile {
        ObsLogFile {
            path: PathBuf::from(name),
            modified_at: Some(UNIX_EPOCH + Duration::from_secs(modified)),
        }
    }

    fn local_time(name: &str) -> SystemTime {
        chrono::NaiveDateTime::parse_from_str(name, "%Y-%m-%d %H-%M-%S")
            .unwrap()
            .and_local_timezone(chrono::Local)
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc)
            .into()
    }

    #[test]
    fn selects_first_log_at_or_after_process_start_minus_ten_seconds() {
        let files = [
            file("2026-11-04 12-00-00.txt", 30),
            file("2026-11-04 12-00-10.txt", 10),
            file("2026-11-04 12-00-20.txt", 20),
        ];
        let selected =
            select_obs_log_file(&files, Some(local_time("2026-11-04 12-00-15"))).unwrap();
        assert_eq!(
            selected.path.file_name().unwrap(),
            "2026-11-04 12-00-10.txt"
        );
    }

    #[test]
    fn selects_most_recently_modified_log_without_process_start_time() {
        let files = [
            file("old.txt", 1),
            file("latest.txt", 3),
            file("middle.txt", 2),
        ];
        assert_eq!(
            select_obs_log_file(&files, None)
                .unwrap()
                .path
                .file_name()
                .unwrap(),
            "latest.txt"
        );
    }

    #[test]
    fn capped_upload_preserves_tail_and_fits_wire_limit() {
        let mut input = String::new();
        let mut value = 0x1234_5678_9abc_def0_u64;
        for index in 0..400_000 {
            value = value
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            input.push_str(&format!("line-{index:06}-{value:016x}\n"));
        }
        let upload = encode_log_upload("obs_log", "latest.txt", None, input.as_bytes()).unwrap();
        assert!(upload.truncated);
        assert!(upload.content_gzip_base64.len() <= MAX_GZIP_BASE64_BYTES);
        let bytes = STANDARD.decode(upload.content_gzip_base64).unwrap();
        let mut tail = String::new();
        GzDecoder::new(bytes.as_slice())
            .read_to_string(&mut tail)
            .unwrap();
        assert!(tail.contains("line-399999-"));
        assert!(!tail.starts_with("line-000000-"));
    }

    #[test]
    fn capped_upload_preserves_a_long_single_line_tail() {
        let mut input = Vec::with_capacity(MAX_GZIP_BASE64_BYTES * 2);
        let mut value = 0x1234_5678_9abc_def0_u64;
        for _ in 0..MAX_GZIP_BASE64_BYTES * 2 {
            value = value
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let byte = ((value >> 32) % 255) as u8;
            input.push(if byte >= b'\n' { byte + 1 } else { byte });
        }
        let upload = encode_log_upload("obs_log", "latest.txt", None, &input).unwrap();
        assert!(upload.truncated);
        assert!(!upload.content_gzip_base64.is_empty());
        assert!(upload.content_gzip_base64.len() <= MAX_GZIP_BASE64_BYTES);
        let bytes = STANDARD.decode(upload.content_gzip_base64).unwrap();
        let mut tail = Vec::new();
        GzDecoder::new(bytes.as_slice())
            .read_to_end(&mut tail)
            .unwrap();
        assert!(!tail.is_empty());
        assert!(tail.len() < input.len());
        assert!(input.ends_with(&tail));
    }
}
