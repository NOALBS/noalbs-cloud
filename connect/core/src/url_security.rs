use std::net::IpAddr;

pub fn validate_api_url(value: &str) -> Result<(), String> {
    validate_url(value, "https", "http", "API")
}

pub fn validate_link_url(value: &str) -> Result<(), String> {
    validate_url(value, "wss", "ws", "link")
}

pub fn validate_verification_url(api_base_url: &str, value: &str) -> Result<(), String> {
    let api_url = reqwest::Url::parse(api_base_url)
        .map_err(|error| format!("Invalid configured API URL: {error}"))?;
    let verification_url =
        reqwest::Url::parse(value).map_err(|error| format!("Invalid verification URL: {error}"))?;
    let api_host = api_url
        .host_str()
        .ok_or_else(|| "Configured API URL has no host".to_owned())?;
    let verification_host = verification_url
        .host_str()
        .ok_or_else(|| "Verification URL has no host".to_owned())?;
    if !api_host.eq_ignore_ascii_case(verification_host) {
        return Err("Verification URL must use the same host as the configured API URL".into());
    }
    if !verification_url.username().is_empty() || verification_url.password().is_some() {
        return Err("Verification URL must not contain credentials".into());
    }
    let api_is_loopback_http = api_url.scheme() == "http" && is_loopback_host(api_host);
    let valid_scheme = verification_url.scheme() == "https"
        || (api_is_loopback_http
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
    use super::{validate_api_url, validate_link_url, validate_verification_url};

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
                "https://cloud.example/connect/verify?code=abc"
            )
            .is_ok()
        );
        assert!(
            validate_verification_url(
                "https://cloud.example/api",
                "https://attacker.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "https://cloud.example/api",
                "http://cloud.example/connect/verify?code=abc"
            )
            .is_err()
        );
        assert!(
            validate_verification_url(
                "http://127.0.0.1:12000/api",
                "http://127.0.0.1:12001/connect/verify?code=abc"
            )
            .is_ok()
        );
        assert!(
            validate_verification_url(
                "http://127.0.0.1:12000/api",
                "http://localhost/connect/verify?code=abc"
            )
            .is_err()
        );
    }
}
