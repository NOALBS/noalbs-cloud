//! Loading a service's settings: `config.toml` in the working directory if
//! there is one, then environment variables, which win. Containers can run on
//! environment variables alone.
//!
//! Variables are `<PREFIX>_<KEY>`, e.g. `API_POSTGRES_URL`; nested keys use a
//! double underscore, e.g. `API_OAUTH_TWITCH__CLIENT_ID`.

use config::{Config, ConfigError, Environment, File, Map};
use serde::de::DeserializeOwned;

/// The example key in the defaults and docs (`0123…cdef`). Anyone can read it,
/// so release builds refuse to run with it.
pub const EXAMPLE_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub fn load<T: DeserializeOwned>(prefix: &str) -> Result<T, ConfigError> {
    load_with(prefix, None)
}

fn load_with<T: DeserializeOwned>(
    prefix: &str,
    env: Option<Map<String, String>>,
) -> Result<T, ConfigError> {
    Config::builder()
        .add_source(File::with_name("config.toml").required(false))
        .add_source(
            Environment::with_prefix(prefix)
                .prefix_separator("_")
                .separator("__")
                // Blank (e.g. `KEY=` in a compose file) means "not set".
                .ignore_empty(true)
                .source(env),
        )
        .build()?
        .try_deserialize()
}

/// Fails in release builds when a secret key is still the public example.
pub fn require_real_key(setting: &str, value: &str) -> Result<(), ConfigError> {
    if !cfg!(debug_assertions) && value == EXAMPLE_KEY {
        return Err(ConfigError::Message(format!(
            "`{setting}` is the example key from the docs. Generate one with `openssl rand -hex 32`."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use config::Map;
    use serde::Deserialize;

    use super::load_with;

    #[derive(Debug, Default, Deserialize)]
    #[serde(default)]
    struct Settings {
        port: u16,
        dev_login: bool,
        data_encryption_key: String,
        oauth_twitch: Option<OAuth>,
        oauth_kick: Option<OAuth>,
    }

    #[derive(Debug, Deserialize)]
    struct OAuth {
        client_id: String,
        client_secret: String,
    }

    #[test]
    fn reads_flat_and_nested_keys_from_the_environment() {
        let env: Map<String, String> = [
            ("API_PORT", "13000"),
            ("API_DEV_LOGIN", "true"),
            ("API_DATA_ENCRYPTION_KEY", "0123456789"),
            ("API_OAUTH_TWITCH__CLIENT_ID", "abc"),
            ("API_OAUTH_TWITCH__CLIENT_SECRET", "shh"),
            ("OTHER_PORT", "1"),
            ("API_OAUTH_KICK__CLIENT_ID", ""),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();

        let settings: Settings = load_with("API", Some(env)).unwrap();
        assert_eq!(settings.port, 13000);
        assert!(settings.dev_login);
        // Digits-only secrets stay strings.
        assert_eq!(settings.data_encryption_key, "0123456789");
        // Blank variables count as unset.
        assert!(settings.oauth_kick.is_none());
        let twitch = settings.oauth_twitch.unwrap();
        assert_eq!(
            (twitch.client_id.as_str(), twitch.client_secret.as_str()),
            ("abc", "shh")
        );
    }
}
