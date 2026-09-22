//! Environment-only configuration, parsed once into a validated struct.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]

pub struct Config {
    pub database_url: String,
    pub library_root: PathBuf,
    pub scan_interval: Duration,
    pub ingest_require_scan: bool,
    pub ingest_quiet: Duration,
    pub derivative_workers: usize,
    pub max_upload_bytes: u64,
    pub limits: imagepipe::Limits,
    pub decode_timeout: Duration,
    /// Parsed and validated, but AVIF encoding is not implemented yet: derivatives are JPEG only.
    #[allow(dead_code)]
    pub enable_avif: bool,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_map(&std::env::vars().collect())
    }

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

        let database_url = required("DATABASE_URL")?;
        if !(database_url.starts_with("sqlite:") || database_url.starts_with("postgres")) {
            return Err("DATABASE_URL must start with sqlite:// or postgres://".into());
        }
        let derivative_workers: usize = parsed(env, "DERIVATIVE_WORKERS", "2")?;
        if derivative_workers == 0 {
            return Err("DERIVATIVE_WORKERS must be at least 1".into());
        }

        Ok(Self {
            database_url,
            library_root: required("LIBRARY_ROOT")?.into(),
            scan_interval: Duration::from_secs(parsed(env, "SCAN_INTERVAL_SECS", "60")?),
            ingest_require_scan: parsed(env, "INGEST_REQUIRE_SCAN", "false")?,
            ingest_quiet: Duration::from_secs(parsed(env, "INGEST_QUIET_SECS", "5")?),
            derivative_workers,
            // Shared with web: the indexer enforces it again on promotion.
            max_upload_bytes: parsed(env, "MAX_UPLOAD_BYTES", "104857600")?,
            limits: imagepipe::Limits {
                max_pixels: parsed(env, "MAX_DECODE_PIXELS", "80000000")?,
                max_bytes: parsed(env, "MAX_DECODE_BYTES", "134217728")?,
            },
            decode_timeout: Duration::from_secs(parsed(env, "DECODE_TIMEOUT_SECS", "30")?),
            enable_avif: parsed(env, "ENABLE_AVIF", "true")?,
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

    #[test]
    fn defaults_match_spec() {
        let c = Config::from_map(&env(&[
            ("DATABASE_URL", "sqlite://x.db"),
            ("LIBRARY_ROOT", "l"),
        ]))
        .unwrap();
        assert_eq!(c.scan_interval, Duration::from_secs(60));
        assert!(!c.ingest_require_scan);
        assert_eq!(c.derivative_workers, 2);
        assert_eq!(c.limits, imagepipe::Limits::default());
        assert!(c.enable_avif);
    }

    #[test]
    fn invalid_values_fail() {
        let base = [("DATABASE_URL", "sqlite://x.db"), ("LIBRARY_ROOT", "l")];
        for bad in [
            ("INGEST_REQUIRE_SCAN", "maybe"),
            ("DERIVATIVE_WORKERS", "0"),
            ("SCAN_INTERVAL_SECS", "-1"),
        ] {
            let mut p = base.to_vec();
            p.push(bad);
            assert!(Config::from_map(&env(&p)).is_err(), "{bad:?}");
        }
    }
}
