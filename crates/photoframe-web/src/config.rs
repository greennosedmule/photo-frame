//! Environment-only configuration, parsed once into a validated struct.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use ipnet::IpNet;

#[derive(Debug, Clone)]
#[allow(dead_code)] // some fields are only read by the PWA routes, which do not exist yet
pub struct Config {
    pub database_url: String,
    pub library_root: PathBuf,
    pub bind_addr: SocketAddr,
    pub public_hostname: Option<String>,
    pub admin_username: String,
    /// `None` when management is granted by `admin_allowed_cidrs` alone.
    pub admin_password: Option<String>,
    /// Source ranges granted management access without credentials.
    pub admin_allowed_cidrs: Vec<IpNet>,
    /// Peers whose `X-Forwarded-For` is believed (the Ingress controller).
    pub trusted_proxy_cidrs: Vec<IpNet>,
    pub max_upload_bytes: u64,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_map(&std::env::vars().collect())
    }

    /// Separate from `from_env` so tests never touch process-global state.
    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, String> {
        let required = |k: &str| {
            env.get(k)
                .filter(|v| !v.is_empty())
                .cloned()
                .ok_or_else(|| format!("{k} is required"))
        };
        fn parsed<T: std::str::FromStr>(
            env: &HashMap<String, String>,
            k: &str,
            default: &str,
        ) -> Result<T, String> {
            let raw = env.get(k).map(String::as_str).unwrap_or(default);
            raw.parse()
                .map_err(|_| format!("{k}: invalid value {raw:?}"))
        }

        fn cidrs(env: &HashMap<String, String>, k: &str) -> Result<Vec<IpNet>, String> {
            env.get(k)
                .map(String::as_str)
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    // A bare address is a host route; anything else must be a CIDR.
                    s.parse::<IpNet>()
                        .or_else(|_| s.parse::<std::net::IpAddr>().map(IpNet::from))
                        .map_err(|_| format!("{k}: invalid CIDR {s:?}"))
                })
                .collect()
        }

        let admin_password = env.get("ADMIN_PASSWORD").filter(|v| !v.is_empty()).cloned();
        let admin_allowed_cidrs = cidrs(env, "ADMIN_ALLOWED_CIDRS")?;
        if admin_password.is_none() && admin_allowed_cidrs.is_empty() {
            return Err("ADMIN_PASSWORD or ADMIN_ALLOWED_CIDRS is required".into());
        }
        let trusted_proxy_cidrs = cidrs(env, "TRUSTED_PROXY_CIDRS")?;

        let database_url = required("DATABASE_URL")?;
        if !(database_url.starts_with("sqlite:") || database_url.starts_with("postgres")) {
            return Err("DATABASE_URL must start with sqlite:// or postgres://".into());
        }

        Ok(Self {
            database_url,
            library_root: required("LIBRARY_ROOT")?.into(),
            bind_addr: parsed(env, "BIND_ADDR", "0.0.0.0:8080")?,
            public_hostname: env
                .get("PUBLIC_HOSTNAME")
                .filter(|v| !v.is_empty())
                .cloned(),
            admin_username: env
                .get("ADMIN_USERNAME")
                .cloned()
                .unwrap_or_else(|| "admin".into()),
            admin_password,
            admin_allowed_cidrs,
            trusted_proxy_cidrs,
            max_upload_bytes: parsed(env, "MAX_UPLOAD_BYTES", "104857600")?,
            log_level: env
                .get("LOG_LEVEL")
                .cloned()
                .unwrap_or_else(|| "info".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    const OK: &[(&str, &str)] = &[
        ("DATABASE_URL", "sqlite://./state/photoframe.db"),
        ("LIBRARY_ROOT", "./library"),
        ("ADMIN_PASSWORD", "dev"),
    ];

    #[test]
    fn defaults_apply() {
        let c = Config::from_map(&env(OK)).unwrap();
        assert_eq!(c.bind_addr.to_string(), "0.0.0.0:8080");
        assert_eq!(c.admin_username, "admin");
        assert_eq!(c.max_upload_bytes, 104_857_600);
    }

    #[test]
    fn missing_required_fails() {
        for missing in ["DATABASE_URL", "LIBRARY_ROOT"] {
            let pairs: Vec<_> = OK.iter().copied().filter(|(k, _)| *k != missing).collect();
            let err = Config::from_map(&env(&pairs)).unwrap_err();
            assert!(err.contains(missing), "{err}");
        }
    }

    #[test]
    fn invalid_value_fails_rather_than_defaulting() {
        let mut pairs = OK.to_vec();
        pairs.push(("MAX_UPLOAD_BYTES", "lots"));
        assert!(Config::from_map(&env(&pairs)).is_err());
    }

    #[test]
    fn needs_a_password_or_a_cidr_list() {
        let pairs: Vec<_> = OK
            .iter()
            .copied()
            .filter(|(k, _)| *k != "ADMIN_PASSWORD")
            .collect();
        let err = Config::from_map(&env(&pairs)).unwrap_err();
        assert!(err.contains("ADMIN_ALLOWED_CIDRS"), "{err}");

        let mut with_cidr = pairs.clone();
        with_cidr.push(("ADMIN_ALLOWED_CIDRS", "192.168.1.0/24, 10.0.0.7 ,fd00::/8"));
        with_cidr.push(("TRUSTED_PROXY_CIDRS", "10.42.0.0/16"));
        let c = Config::from_map(&env(&with_cidr)).unwrap();
        assert!(c.admin_password.is_none());
        assert_eq!(c.admin_allowed_cidrs.len(), 3);
        assert_eq!(c.admin_allowed_cidrs[1].to_string(), "10.0.0.7/32");
        assert_eq!(c.trusted_proxy_cidrs.len(), 1);
    }

    #[test]
    fn invalid_cidr_fails_rather_than_being_skipped() {
        let mut pairs = OK.to_vec();
        pairs.push(("ADMIN_ALLOWED_CIDRS", "192.168.1.0/24,nonsense"));
        assert!(
            Config::from_map(&env(&pairs))
                .unwrap_err()
                .contains("nonsense")
        );
    }
}
