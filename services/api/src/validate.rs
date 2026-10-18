//! Input validation shared by route handlers.

use crate::routes::ApiError;

/// Dashboard routes and paths the proxy sends elsewhere (`/api`, `/link`,
/// `/source`, `/assets`, `/chat/webhooks`) that live next to `/{workspace_slug}`,
/// so no workspace may use them as its slug.
const RESERVED_SLUGS: &[&str] = &[
    "add-server",
    "admin",
    "api",
    "app",
    "assets",
    "auth",
    "chat",
    "dashboard",
    "help",
    "integrate-stream-server",
    "invite",
    "invites",
    "licenses",
    "link",
    "login",
    "logout",
    "new",
    "org-invite",
    "orgs",
    "organizations",
    "privacy",
    "settings",
    "signup",
    "source",
    "static",
    "support",
    "terms",
    "welcome",
    "workspaces",
];

pub const SLUG_MIN_LEN: usize = 3;
pub const SLUG_MAX_LEN: usize = 50;
pub const NAME_MAX_LEN: usize = 60;

/// Validates a workspace URL slug: lowercase letters, digits and single dashes,
/// not starting or ending with a dash, and not a reserved route name.
pub fn url_slug(slug: &str) -> Result<(), ApiError> {
    let invalid = |message: &str| ApiError::bad_request("invalid_slug", message);

    if slug.len() < SLUG_MIN_LEN || slug.len() > SLUG_MAX_LEN {
        return Err(invalid("URL must be between 3 and 50 characters long"));
    }

    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(invalid(
            "URL can only contain lowercase letters, numbers, and dashes",
        ));
    }

    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(invalid("URL cannot start or end with a dash"));
    }

    if slug.contains("--") {
        return Err(invalid("URL cannot contain consecutive dashes"));
    }

    if RESERVED_SLUGS.contains(&slug) {
        return Err(ApiError::conflict(
            "slug_taken",
            "This URL is not available",
        ));
    }

    Ok(())
}

/// Validates and trims a workspace (or switcher) display name.
pub fn name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();

    if name.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_name",
            "Name cannot be empty",
        ));
    }

    if name.chars().count() > NAME_MAX_LEN {
        return Err(ApiError::bad_request(
            "invalid_name",
            "Name must be at most 60 characters long",
        ));
    }

    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::{name, url_slug};

    #[test]
    fn accepts_simple_slugs() {
        for slug in ["nomad-live", "abc", "team-42", "a1b2c3"] {
            assert!(url_slug(slug).is_ok(), "{slug} should be valid");
        }
    }

    #[test]
    fn rejects_malformed_slugs() {
        for slug in [
            "ab",
            "Nomad",
            "no_underscore",
            "-lead",
            "trail-",
            "dou--ble",
            "spa ce",
        ] {
            assert!(url_slug(slug).is_err(), "{slug} should be invalid");
        }
        assert!(url_slug(&"a".repeat(51)).is_err());
    }

    #[test]
    fn rejects_reserved_slugs() {
        let error = url_slug("login").unwrap_err();
        assert_eq!(error.code(), "slug_taken");
        assert!(url_slug("welcome").is_err());
        assert!(url_slug("invite").is_err());
        assert!(url_slug("add-server").is_err());
        assert!(url_slug("integrate-stream-server").is_err());
        assert!(url_slug("privacy").is_err());
        assert!(url_slug("terms").is_err());
        assert!(url_slug("licenses").is_err());
    }

    #[test]
    fn trims_and_limits_names() {
        assert_eq!(name("  Nomad Live  ").unwrap(), "Nomad Live");
        assert!(name("   ").is_err());
        assert!(name(&"x".repeat(61)).is_err());
    }
}
