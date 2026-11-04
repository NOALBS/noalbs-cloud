use std::net::IpAddr;

pub fn validate_api_url(value: &str) -> Result<(), String> {
    validate_url(value, "https", "http", "API")
}

pub fn validate_link_url(value: &str) -> Result<(), String> {
    validate_url(value, "wss", "ws", "link")
}

pub fn validate_dashboard_url(value: &str) -> Result<(), String> {
    validate_url(value, "https", "http", "dashboard")?;
    let url =
        reqwest::Url::parse(value).map_err(|error| format!("Invalid dashboard URL: {error}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Dashboard URL must not contain credentials".into());
    }
    Ok(())
}

pub fn validate_verification_url(
    api_base_url: &str,
    dashboard_url: Option<&str>,
    value: &str,
) -> Result<(), String> {
    validate_api_url(api_base_url)?;
    let api_url = reqwest::Url::parse(api_base_url)
        .map_err(|error| format!("Invalid configured API URL: {error}"))?;
    let verification_url =
        reqwest::Url::parse(value).map_err(|error| format!("Invalid verification URL: {error}"))?;
    let verification_host = verification_url
        .host_str()
        .ok_or_else(|| "Verification URL has no host".to_owned())?;
    if !verification_url.username().is_empty() || verification_url.password().is_some() {
        return Err("Verification URL must not contain credentials".into());
    }

    let dashboard_url = dashboard_url
        .map(|url| {
            validate_dashboard_url(url)?;
            reqwest::Url::parse(url).map_err(|error| format!("Invalid dashboard URL: {error}"))
        })
        .transpose()?;
    let origin_url = dashboard_url.as_ref().unwrap_or(&api_url);
    let origin_host = origin_url
        .host_str()
        .ok_or_else(|| "Configured URL has no host".to_owned())?;
    let origin_matches = if dashboard_url.is_some() {
        origin_url.scheme() == verification_url.scheme()
            && origin_host.eq_ignore_ascii_case(verification_host)
            && origin_url.port_or_known_default() == verification_url.port_or_known_default()
    } else {
        origin_host.eq_ignore_ascii_case(verification_host)
    };
    if !origin_matches {
        let url_kind = if dashboard_url.is_some() {
            "configured dashboard URL origin"
        } else {
            "configured API URL host"
        };
        return Err(format!("Verification URL must use the same {url_kind}"));
    }

    let loopback_http_allowed = origin_url.scheme() == "http" && is_loopback_host(origin_host);
    let valid_scheme = verification_url.scheme() == "https"
        || (loopback_http_allowed
            && verification_url.scheme() == "http"
            && is_loopback_host(verification_host));
    if !valid_scheme {
        return Err(
            "Verification URL must use HTTPS; HTTP is allowed only for a loopback API host".into(),
        );
    }
    Ok(())
}

fn validate_url(
    value: &str,
    secure_scheme: &str,
    loopback_scheme: &str,
    label: &str,
) -> Result<(), String> {
    let url =
        reqwest::Url::parse(value).map_err(|error| format!("Invalid {label} URL: {error}"))?;
    let loopback = url.host_str().is_some_and(is_loopback_host);
    if url.scheme() == secure_scheme || (url.scheme() == loopback_scheme && loopback) {
        Ok(())
    } else {
        Err(format!(
            "{label} URL must use {secure_scheme}; {loopback_scheme} is allowed only for loopback hosts"
        ))
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::{
        validate_api_url, validate_dashboard_url, validate_link_url, validate_verification_url,
    };

    #[test]
    fn insecure_urls_are_allowed_only_for_loopback_hosts() {
        assert!(validate_api_url("https://example.com/api").is_ok());
        assert!(validate_api_url("http://127.0.0.5/api").is_ok());
        assert!(validate_api_url("http://[::1]/api").is_ok());
        assert!(validate_api_url("http://example.com/api").is_err());
        assert!(validate_link_url("wss://example.com/link").is_ok());
        assert!(validate_link_url("ws://localhost/link").is_ok());
        assert!(validate_link_url("ws://[::1]/link").is_ok());
        assert!(validate_link_url("ws://example.com/link").is_err());
    }

    #[test]
    fn verification_urls_must_use_the_api_host_and_a_secure_scheme() {
        assert!(
            validate_verification_url(
                "https://cloud.example/api",
                None,
                "https://cloud.example/connect/verify?code=abc"
            )
            .is_ok()
        );
        assert!(
            validate_verification_url(
                "https://cloud.example/api",
                None,
                "https://attacker.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "https://cloud.example/api",
                None,
                "http://cloud.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "http://127.0.0.1:12000/api",
                None,
                "http://127.0.0.1:12001/connect/verify?code=abc"
            )
            .is_ok()
        );
        assert!(
            validate_verification_url(
                "http://127.0.0.1:12000/api",
                None,
                "http://localhost/connect/verify?code=abc"
            )
            .is_err()
        );
    }

    #[test]
    fn verification_url_uses_the_dashboard_origin_when_configured() {
        assert!(validate_dashboard_url("https://dashboard.example").is_ok());
        assert!(validate_dashboard_url("http://127.0.0.1:3000").is_ok());
        assert!(validate_dashboard_url("http://dashboard.example").is_err());
        assert!(
            validate_verification_url(
                "https://api.example/api",
                Some("https://dashboard.example"),
                "https://dashboard.example/connect/verify?code=abc"
            )
            .is_ok()
        );
        assert!(
            validate_verification_url(
                "https://api.example/api",
                None,
                "https://dashboard.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "https://api.example/api",
                Some("https://dashboard.example"),
                "https://user@dashboard.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "https://api.example/api",
                Some("https://dashboard.example:8443"),
                "https://dashboard.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "https://api.example/api",
                Some("http://127.0.0.1:3000"),
                "http://127.0.0.1:3000/connect/verify?code=abc"
            )
            .is_ok()
        );
    }
}
