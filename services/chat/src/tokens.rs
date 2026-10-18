//! Stored user tokens (Kick and YouTube: bots post with theirs; Kick channels
//! may subscribe with theirs, YouTube channels find and read their live chat
//! with theirs), refreshed when close to expiring. Kick replaces the refresh
//! token on every refresh, so the row is locked while refreshing: another
//! instance waits and then uses the fresh token instead of spending the old one.

use chrono::{Duration, Utc};
use db::chat::{ChatPlatform, StoredTokens, TokenOwner};

use crate::{
    app::State,
    error::{Error, Result},
};

/// Refresh this long before a token expires.
const MARGIN: Duration = Duration::minutes(2);

/// A usable access token for `owner`. A refused refresh marks the grant revoked.
pub async fn access_token(state: &State, owner: TokenOwner) -> Result<String> {
    token_valid_for(state, owner, MARGIN).await
}

/// A newly refreshed access token, for when the platform refused the stored one
/// before its time.
pub async fn refreshed_token(state: &State, owner: TokenOwner) -> Result<String> {
    token_valid_for(state, owner, Duration::days(365)).await
}

/// New tokens from a platform's refresh endpoint.
struct Renewed {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

async fn renew(state: &State, platform: ChatPlatform, refresh: &str) -> Result<Renewed> {
    let not_configured = || Error::Other(format!("{} isn't configured", platform.label()));
    match platform {
        ChatPlatform::Kick => {
            let kick = state.kick.clone().ok_or_else(not_configured)?;
            let renewed = kick.refresh(refresh).await?;
            Ok(Renewed {
                access_token: renewed.access_token,
                refresh_token: renewed.refresh_token,
                expires_in: renewed.expires_in,
            })
        }
        ChatPlatform::YouTube => {
            let youtube = state.youtube.clone().ok_or_else(not_configured)?;
            let renewed = youtube.refresh(refresh).await?;
            Ok(Renewed {
                access_token: renewed.access_token,
                refresh_token: renewed.refresh_token,
                expires_in: renewed.expires_in,
            })
        }
        ChatPlatform::Twitch => Err(Error::Other("Twitch grants hold no tokens".into())),
    }
}

/// An access token valid for at least `margin`, refreshed if needed.
async fn token_valid_for(state: &State, owner: TokenOwner, margin: Duration) -> Result<String> {
    let mut tx = state.db.begin().await.map_err(db::Error::from)?;
    let stored = db::chat::lock_tokens(&mut *tx, owner)
        .await?
        .ok_or_else(|| Error::Other("no such grant".into()))?;
    let access = stored
        .access_token
        .as_deref()
        .ok_or_else(|| Error::Other("no token stored".into()))?;

    let fresh = stored
        .expires_at
        .is_none_or(|expires_at| expires_at - Utc::now() > margin);
    if fresh {
        return Ok(decrypt(state, access)?);
    }

    let refresh = stored
        .refresh_token
        .as_deref()
        .ok_or_else(|| Error::Other("the token expired and can't be refreshed".into()))?;
    let refresh = decrypt(state, refresh)?;
    let renewed = match renew(state, stored.platform, &refresh).await {
        Ok(renewed) => renewed,
        Err(error) => {
            if grant_refused(stored.platform, &error) {
                tracing::warn!(
                    ?owner,
                    platform = stored.platform.label(),
                    "A refresh was refused; the grant is revoked"
                );
                drop(tx);
                db::chat::revoke(&state.db, owner).await?;
            }
            return Err(error);
        }
    };
    let tokens = StoredTokens {
        platform: stored.platform,
        access_token: Some(state.data_key.encrypt(renewed.access_token.as_bytes())),
        refresh_token: Some(
            state.data_key.encrypt(
                renewed
                    .refresh_token
                    .as_deref()
                    .unwrap_or(&refresh)
                    .as_bytes(),
            ),
        ),
        expires_at: renewed
            .expires_in
            .map(|seconds| Utc::now() + Duration::seconds(seconds)),
    };
    db::chat::store_tokens(&mut *tx, owner, &tokens).await?;
    tx.commit().await.map_err(db::Error::from)?;
    Ok(renewed.access_token)
}

/// Whether a refused refresh means the grant is gone (rather than, say, our app
/// being misconfigured). Google says `invalid_grant` for revoked or expired grants.
fn grant_refused(platform: ChatPlatform, error: &Error) -> bool {
    match error {
        Error::Platform { status, body, .. } if status.is_client_error() => {
            platform != ChatPlatform::YouTube || body.contains("invalid_grant")
        }
        _ => false,
    }
}

fn decrypt(state: &State, stored: &[u8]) -> Result<String> {
    let plain = state.data_key.decrypt(stored)?;
    String::from_utf8(plain).map_err(|_| Error::Other("a stored token isn't text".into()))
}

/// Refreshes stored tokens that expire within this, ahead of use.
const KEEP_AHEAD: Duration = Duration::hours(1);
const KEEP_ALIVE_EVERY: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Every half hour one instance (a lease) refreshes the tokens close to
/// expiring, so a bot that hasn't spoken in a while can still speak, and a
/// revoked grant shows up on the dashboard before anyone needs it.
pub async fn keep_alive(state: State) {
    let mut tick = tokio::time::interval(KEEP_ALIVE_EVERY);
    loop {
        tick.tick().await;
        let lease = match crate::kv::Lease::acquire(
            &state.buckets.locks,
            &state.key(&["tokens"]),
            &state.instance_id,
        )
        .await
        {
            Ok(Some(lease)) => lease,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(%error, "Couldn't take the token lease");
                continue;
            }
        };
        match db::chat::tokens_expiring(&state.db, Utc::now() + KEEP_AHEAD).await {
            Ok(owners) => {
                for owner in owners {
                    if let Err(error) = token_valid_for(&state, owner, KEEP_AHEAD).await {
                        tracing::warn!(%error, ?owner, "Couldn't refresh a token");
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "Couldn't list tokens to refresh"),
        }
        lease.release().await;
    }
}
