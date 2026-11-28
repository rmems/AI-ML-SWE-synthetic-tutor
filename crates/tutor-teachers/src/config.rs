use serde::{Deserialize, Serialize};

use crate::Error;

/// Providers understood by the readiness command, not executable live clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Built-in first-party wording; no model or network.
    Mock,
    /// Future teacher selection; execution is always blocked in this slice.
    Deepseek,
}

/// Non-secret provider selection. Unknown fields (including API keys) fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeacherConfig {
    /// Local mock or future DeepSeek teacher.
    pub provider: Provider,
    /// Exact intended model ID; not evidence that the model is approved.
    pub model: String,
    /// Base URL for a future client; never contacted.
    pub endpoint: Option<String>,
    /// Intended route, separately from the serving vendor.
    pub route: String,
    /// Intended serving vendor.
    pub serving_vendor: String,
    /// Environment variable NAME only; its value is never read.
    pub api_key_env: Option<String>,
}

impl Default for TeacherConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Mock,
            model: "derivative-surface-v1".into(),
            endpoint: None,
            route: "offline".into(),
            serving_vendor: "first-party-fixture".into(),
            api_key_env: None,
        }
    }
}

impl TeacherConfig {
    /// Parse without echoing user-supplied values in error messages.
    pub fn from_json(text: &str) -> Result<Self, Error> {
        let config: Self = serde_json::from_str(text)
            .map_err(|_| "invalid teacher config: expected only documented non-secret fields")?;
        config.validate()?;
        Ok(config)
    }

    /// Check shape only. This does not establish rights or permit live calls.
    pub fn validate(&self) -> Result<(), Error> {
        if self.provider == Provider::Mock {
            if self != &Self::default() {
                return Err("mock config must match the built-in offline provider".into());
            }
            return Ok(());
        }
        for value in [&self.model, &self.route, &self.serving_vendor] {
            if value.is_empty()
                || value.len() > 200
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._/:".contains(&b))
            {
                return Err(
                    "model, route and serving_vendor must be non-secret identifiers".into(),
                );
            }
        }
        let name = self
            .api_key_env
            .as_deref()
            .ok_or("api_key_env NAME is required")?;
        if name.is_empty()
            || name.len() > 128
            || name.as_bytes()[0].is_ascii_digit()
            || !name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("api_key_env must be an uppercase environment variable NAME".into());
        }
        let endpoint = url::Url::parse(self.endpoint.as_deref().ok_or("endpoint is required")?)
            .map_err(|_| "endpoint must be an HTTPS base URL without credentials")?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("endpoint must be HTTPS with no credentials, query or fragment".into());
        }
        Ok(())
    }
}
