//! Kind-specific stream server settings, kept in `stream_servers.details`:
//! which fields each kind uses, validation, and the write-only secrets
//! (encrypted with the data key; the API only says whether one is set).

use common::crypto::DataKey;
use db::stream_servers::{Details, StreamServerKind};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::routes::{ApiError, ApiResult};

const MAX_LEN: usize = 256;

/// A stream server's settings as the dashboard sees them. Only the fields its
/// kind uses are set.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct DetailsDto {
    pub publisher: Option<String>,
    pub application: Option<String>,
    pub key: Option<String>,
    pub listener_id: Option<String>,
    pub username: Option<String>,
    /// A basic-auth password is stored (it is never returned).
    pub has_password: bool,
    /// An API key (SLS) or token (IRLHosting WebSocket) is stored (it is never
    /// returned).
    pub has_api_key: bool,
}

impl From<Details> for DetailsDto {
    fn from(details: Details) -> Self {
        Self {
            publisher: details.publisher,
            application: details.application,
            key: details.key,
            listener_id: details.listener_id,
            username: details.username,
            has_password: details.encrypted_password.is_some(),
            has_api_key: details.encrypted_api_key.is_some(),
        }
    }
}

/// Settings to save. Fields the kind doesn't use are ignored. Leave `password`
/// or `api_key` out to keep the stored one; `clear_*` removes it.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct DetailsInput {
    pub publisher: Option<String>,
    pub application: Option<String>,
    pub key: Option<String>,
    pub listener_id: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub clear_password: bool,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    No,
    Optional,
    Required,
}

/// Which settings a kind reads (mirrors obs-link's fetchers).
struct Fields {
    publisher: Need,
    application: Need,
    key: Need,
    listener_id: Need,
    basic_auth: bool,
    api_key: bool,
}

impl Fields {
    const NONE: Self = Self {
        publisher: Need::No,
        application: Need::No,
        key: Need::No,
        listener_id: Need::No,
        basic_auth: false,
        api_key: false,
    };
}

fn fields(kind: StreamServerKind) -> Fields {
    use Need::{Optional, Required};
    use StreamServerKind as K;

    match kind {
        K::Mediamtx => Fields {
            basic_auth: true,
            ..Fields::NONE
        },
        K::Sls => Fields {
            publisher: Required,
            api_key: true,
            ..Fields::NONE
        },
        K::Nginx | K::Xiu => Fields {
            application: Required,
            key: Required,
            ..Fields::NONE
        },
        K::Nms => Fields {
            application: Required,
            key: Required,
            basic_auth: true,
            ..Fields::NONE
        },
        K::Nimble => Fields {
            application: Required,
            key: Required,
            listener_id: Required,
            ..Fields::NONE
        },
        // SRT (publisher) or RTMP (application + key); checked below.
        K::Irlhosting => Fields {
            publisher: Optional,
            application: Optional,
            key: Optional,
            ..Fields::NONE
        },
        // The feed (else the first active stream) and a token, kept as the API key.
        K::IrlhostingWebsocket => Fields {
            publisher: Optional,
            api_key: true,
            ..Fields::NONE
        },
        K::Belabox | K::Openirl | K::Openrtmp | K::Rist => Fields::NONE,
    }
}

/// A trimmed value, `None` when empty or not used by the kind.
fn field(
    need: Need,
    value: Option<String>,
    code: &'static str,
    missing: &str,
) -> ApiResult<Option<String>> {
    if need == Need::No {
        return Ok(None);
    }
    let value = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    match value {
        Some(value) if value.chars().count() > MAX_LEN => Err(ApiError::bad_request(
            code,
            format!("That's too long (at most {MAX_LEN} characters)"),
        )),
        None if need == Need::Required => Err(ApiError::bad_request(code, missing)),
        value => Ok(value),
    }
}

/// A secret to store: kept, cleared or replaced (encrypted).
fn secret(
    used: bool,
    new: Option<String>,
    clear: bool,
    stored: Option<&String>,
    key: &DataKey,
) -> Option<String> {
    if !used || clear {
        return None;
    }
    match new.filter(|value| !value.is_empty()) {
        Some(value) => Some(key.encrypt_to_string(&value)),
        None => stored.cloned(),
    }
}

/// Validates `input` for `kind` and merges it with what's `stored` (secrets
/// left out are kept).
pub fn build(
    kind: StreamServerKind,
    input: DetailsInput,
    stored: &Details,
    key: &DataKey,
) -> ApiResult<Details> {
    let fields = fields(kind);
    let details = Details {
        publisher: field(
            fields.publisher,
            input.publisher,
            "invalid_publisher",
            "Enter the publisher (stream ID)",
        )?,
        application: field(
            fields.application,
            input.application,
            "invalid_application",
            "Enter the application name",
        )?,
        key: field(fields.key, input.key, "invalid_key", "Enter the stream key")?,
        listener_id: field(
            fields.listener_id,
            input.listener_id,
            "invalid_listener_id",
            "Enter the SRT listener ID (usually ip:port)",
        )?,
        username: field(
            if fields.basic_auth {
                Need::Optional
            } else {
                Need::No
            },
            input.username,
            "invalid_username",
            "",
        )?,
        encrypted_password: secret(
            fields.basic_auth,
            input.password,
            input.clear_password,
            stored.encrypted_password.as_ref(),
            key,
        ),
        encrypted_api_key: secret(
            fields.api_key,
            input.api_key,
            input.clear_api_key,
            stored.encrypted_api_key.as_ref(),
            key,
        ),
    };

    if details.encrypted_password.is_some() && details.username.is_none() {
        return Err(ApiError::bad_request(
            "invalid_username",
            "Enter the username that goes with the password",
        ));
    }
    if kind == StreamServerKind::Irlhosting
        && details.publisher.is_none()
        && (details.application.is_none() || details.key.is_none())
    {
        return Err(ApiError::bad_request(
            "invalid_publisher",
            "Enter the publisher for SRT, or the application and stream key for RTMP",
        ));
    }

    Ok(details)
}

/// The secrets, decrypted, for testing a connection. Undecryptable ones are
/// left out (the test then reports what the server says).
pub struct Secrets {
    pub password: Option<String>,
    pub api_key: Option<String>,
}

pub fn decrypt(details: &Details, key: &DataKey) -> Secrets {
    let decrypt = |stored: &Option<String>| {
        stored
            .as_deref()
            .and_then(|stored| key.decrypt_string(stored).ok())
    };
    Secrets {
        password: decrypt(&details.encrypted_password),
        api_key: decrypt(&details.encrypted_api_key),
    }
}

#[cfg(test)]
mod tests {
    use common::crypto::DataKey;
    use db::stream_servers::{Details, StreamServerKind};

    use super::{DetailsInput, build, decrypt};

    fn key() -> DataKey {
        DataKey::from_hex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .unwrap()
    }

    #[test]
    fn keeps_only_the_fields_a_kind_uses() {
        let input = DetailsInput {
            publisher: Some("  publish/live/feed1 ".into()),
            application: Some("live".into()),
            key: Some("feed1".into()),
            ..DetailsInput::default()
        };
        let sls = build(StreamServerKind::Sls, input, &Details::default(), &key()).unwrap();
        assert_eq!(sls.publisher.as_deref(), Some("publish/live/feed1"));
        assert_eq!(sls.application, None);
        assert_eq!(sls.key, None);
    }

    #[test]
    fn requires_what_a_kind_needs() {
        let error = build(
            StreamServerKind::Nginx,
            DetailsInput {
                application: Some("live".into()),
                ..DetailsInput::default()
            },
            &Details::default(),
            &key(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalid_key");

        let error = build(
            StreamServerKind::Irlhosting,
            DetailsInput::default(),
            &Details::default(),
            &key(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalid_publisher");
        assert!(
            build(
                StreamServerKind::Openirl,
                DetailsInput::default(),
                &Details::default(),
                &key()
            )
            .is_ok()
        );
    }

    #[test]
    fn secrets_are_encrypted_kept_and_cleared() {
        let key = key();
        let set = build(
            StreamServerKind::Mediamtx,
            DetailsInput {
                username: Some("admin".into()),
                password: Some("hunter2".into()),
                ..DetailsInput::default()
            },
            &Details::default(),
            &key,
        )
        .unwrap();
        let stored = set.encrypted_password.clone().unwrap();
        assert!(!stored.contains("hunter2"));
        assert_eq!(decrypt(&set, &key).password.as_deref(), Some("hunter2"));

        // Left out: kept.
        let kept = build(
            StreamServerKind::Mediamtx,
            DetailsInput {
                username: Some("admin".into()),
                ..DetailsInput::default()
            },
            &set,
            &key,
        )
        .unwrap();
        assert_eq!(kept.encrypted_password, Some(stored));

        let cleared = build(
            StreamServerKind::Mediamtx,
            DetailsInput {
                username: Some("admin".into()),
                clear_password: true,
                ..DetailsInput::default()
            },
            &set,
            &key,
        )
        .unwrap();
        assert_eq!(cleared.encrypted_password, None);

        // A password needs a username.
        let error = build(
            StreamServerKind::Mediamtx,
            DetailsInput {
                password: Some("hunter2".into()),
                ..DetailsInput::default()
            },
            &Details::default(),
            &key,
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalid_username");
    }
}
