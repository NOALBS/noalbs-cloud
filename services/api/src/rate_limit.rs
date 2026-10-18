//! Rate limits, in memory per API replica (a guard against floods and abuse, not
//! an exact quota, so several replicas each allowing the full rate is fine).
//!
//! | Limit | Key | Where |
//! |---|---|---|
//! | [`RateLimits::requests`] | client IP | every request ([`limit_requests`]), except Stripe's webhook |
//! | [`RateLimits::sign_in`] | client IP | OAuth sign-in and linking, dev login |
//! | [`RateLimits::outbound`] | user | handlers that call someone else: stats probe, chatter lookup, Stripe Checkout/portal |
//!
//! The client IP is the proxy's header when `client_ip_header` is configured
//! (`cf-connecting-ip` behind Cloudflare; `x-forwarded-for` behind Caddy alone,
//! which replaces what the client sent), else the connection's address.

use std::{
    collections::HashMap,
    hash::Hash,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName},
    middleware::Next,
    response::{IntoResponse, Response},
};
use common::shutdown::ShutdownHandler;
use uuid::Uuid;

use crate::{app::ApiState, routes::ApiError};

/// How often buckets that filled up again are forgotten.
const PRUNE_EVERY: Duration = Duration::from_secs(60);

pub struct RateLimits {
    /// Any request: generous, so a team on one address (or a carrier's NAT)
    /// never notices; it only stops floods.
    pub requests: Limiter<IpAddr>,
    /// Starting or finishing an OAuth sign-in (each calls the platform).
    pub sign_in: Limiter<IpAddr>,
    /// Requests that make us call another service for the user.
    pub outbound: Limiter<Uuid>,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            requests: Limiter::new(600, Duration::from_millis(50)),
            sign_in: Limiter::new(20, Duration::from_secs(6)),
            outbound: Limiter::new(20, Duration::from_secs(3)),
        }
    }
}

impl RateLimits {
    fn prune(&self, now: Instant) {
        self.requests.prune(now);
        self.sign_in.prune(now);
        self.outbound.prune(now);
    }

    /// Takes one of the user's outbound requests, or 429.
    pub fn outbound(&self, user_id: Uuid) -> Result<(), ApiError> {
        self.outbound
            .check(user_id, Instant::now())
            .map_err(ApiError::rate_limited)
    }
}

/// Forgets idle buckets now and then, so memory follows recent clients only.
pub fn spawn_upkeep(limits: std::sync::Arc<RateLimits>, mut shutdown: ShutdownHandler) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(PRUNE_EVERY);
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = every.tick() => limits.prune(Instant::now()),
            }
        }
    });
}

/// Token buckets per key: `capacity` requests at once, one more every `refill`.
pub struct Limiter<K> {
    capacity: f64,
    refill: Duration,
    buckets: Mutex<HashMap<K, Bucket>>,
}

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: Instant,
}

impl<K: Hash + Eq> Limiter<K> {
    pub fn new(capacity: u32, refill: Duration) -> Self {
        Self {
            capacity: f64::from(capacity),
            refill,
            buckets: Mutex::default(),
        }
    }

    /// Takes a token for `key`; `Err` says how long until one is back.
    pub fn check(&self, key: K, now: Instant) -> Result<(), Duration> {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: self.capacity,
            at: now,
        });
        bucket.tokens = self.tokens_at(*bucket, now);
        bucket.at = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            Err(self.refill.mul_f64(1.0 - bucket.tokens))
        }
    }

    fn tokens_at(&self, bucket: Bucket, now: Instant) -> f64 {
        let refilled =
            now.saturating_duration_since(bucket.at).as_secs_f64() / self.refill.as_secs_f64();
        (bucket.tokens + refilled).min(self.capacity)
    }

    /// Drops buckets that are full again: a new one would be the same.
    fn prune(&self, now: Instant) {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buckets.retain(|_, bucket| self.tokens_at(*bucket, now) < self.capacity);
    }
}

/// The per-IP limits, as middleware in front of every route.
pub async fn limit_requests(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // Stripe retries on its own and may send bursts; the signature guards it.
    if path == "/billing/webhook" {
        return next.run(request).await;
    }

    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    if let Some(ip) = client_ip(request.headers(), state.client_ip_header.as_ref(), peer) {
        let now = Instant::now();
        let mut result = state.limits.requests.check(ip, now);
        if result.is_ok() && is_sign_in(path) {
            result = state.limits.sign_in.check(ip, now);
        }
        if let Err(wait) = result {
            return ApiError::rate_limited(wait).into_response();
        }
    }

    next.run(request).await
}

/// The visitor's address: the configured proxy header if it holds one, else the
/// connection's. With several `x-forwarded-for` entries, the last is the one
/// our proxy added.
fn client_ip(
    headers: &HeaderMap,
    header: Option<&HeaderName>,
    peer: Option<IpAddr>,
) -> Option<IpAddr> {
    header
        .and_then(|name| headers.get(name))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .and_then(|value| value.trim().parse().ok())
        .or(peer)
}

/// Routes that start or finish a sign-in or account link (not `/auth/me` etc.).
fn is_sign_in(path: &str) -> bool {
    path.strip_prefix("/auth/")
        .is_some_and(|rest| !matches!(rest, "me" | "providers" | "logout"))
}

#[cfg(test)]
mod tests {
    use std::{
        net::IpAddr,
        time::{Duration, Instant},
    };

    use axum::http::{HeaderMap, HeaderName, HeaderValue};

    use super::{Limiter, client_ip, is_sign_in};

    #[test]
    fn a_bucket_empties_refills_and_says_how_long_to_wait() {
        let limiter = Limiter::new(3, Duration::from_secs(2));
        let start = Instant::now();
        for _ in 0..3 {
            assert!(limiter.check("a", start).is_ok());
        }
        assert_eq!(limiter.check("a", start), Err(Duration::from_secs(2)));
        // Other keys have buckets of their own.
        assert!(limiter.check("b", start).is_ok());

        let later = start + Duration::from_secs(1);
        assert_eq!(limiter.check("a", later), Err(Duration::from_secs(1)));
        assert!(limiter.check("a", start + Duration::from_secs(2)).is_ok());
        // Never more than the capacity, however long it waited.
        let much_later = start + Duration::from_secs(3600);
        for _ in 0..3 {
            assert!(limiter.check("a", much_later).is_ok());
        }
        assert!(limiter.check("a", much_later).is_err());
    }

    #[test]
    fn pruning_forgets_only_full_buckets() {
        let limiter = Limiter::new(2, Duration::from_secs(1));
        let start = Instant::now();
        limiter.check("used", start).unwrap();
        limiter.prune(start);
        assert_eq!(limiter.buckets.lock().unwrap().len(), 1);
        limiter.prune(start + Duration::from_secs(1));
        assert!(limiter.buckets.lock().unwrap().is_empty());
    }

    #[test]
    fn the_client_ip_comes_from_the_configured_header_or_the_connection() {
        let peer: IpAddr = "10.0.0.2".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.7"));
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.1, 2001:db8::1"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("nonsense"));

        let ip = |header: Option<&'static str>| {
            let header = header.map(HeaderName::from_static);
            client_ip(&headers, header.as_ref(), Some(peer))
                .unwrap()
                .to_string()
        };
        assert_eq!(ip(Some("cf-connecting-ip")), "203.0.113.7");
        assert_eq!(ip(Some("x-forwarded-for")), "2001:db8::1");
        // Not configured: headers are the client's to make up.
        assert_eq!(ip(None), "10.0.0.2");
        // Configured but missing or garbage: the connection.
        assert_eq!(ip(Some("x-client-ip")), "10.0.0.2");
        assert_eq!(ip(Some("x-real-ip")), "10.0.0.2");
    }

    #[test]
    fn sign_in_routes_are_the_oauth_and_dev_login_ones() {
        assert!(is_sign_in("/auth/twitch"));
        assert!(is_sign_in("/auth/kick/callback"));
        assert!(is_sign_in("/auth/link/twitch"));
        assert!(is_sign_in("/auth/dev-login"));
        assert!(!is_sign_in("/auth/me"));
        assert!(!is_sign_in("/auth/providers"));
        assert!(!is_sign_in("/auth/logout"));
        assert!(!is_sign_in("/workspaces"));
    }
}
