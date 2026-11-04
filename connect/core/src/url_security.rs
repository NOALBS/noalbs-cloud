use std::net::IpAddr;

pub fn validate_api_url(value: &str) -> Result<(), String> {
    validate_url(value, "https", "http", "API")
}

pub fn validate_link_url(value: &str) -> Result<(), String> {
    validate_url(value, "wss", "ws", "link")
}

fn validate_url(
    value: &str,
    secure_scheme: &str,
    loopback_scheme: &str,
    label: &str,
) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|error| format!("Invalid {label} URL: {error}"))?;
    let loopback = url.host_str().is_some_and(|host| {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if url.scheme() == secure_scheme || (url.scheme() == loopback_scheme && loopback) {
        Ok(())
    } else {
        Err(format!(
            "{label} URL must use {secure_scheme}; {loopback_scheme} is allowed only for loopback hosts"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_api_url, validate_link_url};

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
}
