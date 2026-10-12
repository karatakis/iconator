/// Server configuration, read from the environment at startup.
#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
}

impl Config {
    /// Read `HOST` / `PORT` from the process environment.
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Build a config from a lookup function. Empty values are treated as
    /// unset. Injected so unit tests can exercise defaults and overrides
    /// without touching the shared process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let lookup = |key: &str| lookup(key).filter(|v| !v.is_empty());
        let host = lookup("HOST").unwrap_or_else(|| "0.0.0.0".to_string());

        let port = match lookup("PORT") {
            Some(raw) => match raw.parse::<u16>() {
                Ok(port) => port,
                Err(err) => {
                    log::warn!("ignoring invalid PORT {raw:?}: {err}; using default 3000");
                    3000
                }
            },
            None => 3000,
        };

        Self { host, port }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let config = Config::from_lookup(|_| None);
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn env_overrides() {
        let config = Config::from_lookup(|key| match key {
            "HOST" => Some("127.0.0.1".to_string()),
            "PORT" => Some("8080".to_string()),
            _ => None,
        });
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn invalid_port_falls_back_to_default() {
        let config = Config::from_lookup(|key| (key == "PORT").then(|| "not-a-port".to_string()));
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn empty_values_are_ignored() {
        let config = Config::from_lookup(|_| Some(String::new()));
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3000);
    }
}
