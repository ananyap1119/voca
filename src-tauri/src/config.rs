use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub provider_name: Option<String>,
    pub endpoint: Option<String>,
    pub api_key_env_var: Option<String>,
    pub model: Option<String>,
    pub language: Option<String>,
    pub codemix: Option<bool>,
    pub hotkey: Option<String>,
    pub polish_mode: Option<String>,
    pub polish_endpoint: Option<String>,
    pub polish_model: Option<String>,
    pub polish_api_key_env_var: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TomlConfig {
    provider: Option<Config>,
}

impl Config {
    fn default_hotkey() -> String {
        if cfg!(target_os = "windows") {
            "Alt".into()
        } else {
            "Alt".into()
        }
    }

    pub fn load(app_name: &str) -> Self {
        let prefix = app_name.to_uppercase().replace("-", "_");
        let mut merged = Self::default();

        if let Some(path) = Self::path(app_name) {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(toml) = toml::from_str::<TomlConfig>(&content) {
                    if let Some(p) = toml.provider {
                        merged.provider_name = p.provider_name.or(merged.provider_name);
                        merged.endpoint = p.endpoint.or(merged.endpoint);
                        merged.model = p.model.or(merged.model);
                        merged.api_key_env_var = p.api_key_env_var.or(merged.api_key_env_var);
                        merged.language = p.language.or(merged.language);
                        merged.codemix = p.codemix.or(merged.codemix);
                        merged.hotkey = p.hotkey.or(merged.hotkey);
                        merged.polish_mode = p.polish_mode.or(merged.polish_mode);
                        merged.polish_endpoint = p.polish_endpoint.or(merged.polish_endpoint);
                        merged.polish_model = p.polish_model.or(merged.polish_model);
                        merged.polish_api_key_env_var =
                            p.polish_api_key_env_var.or(merged.polish_api_key_env_var);
                    }
                }
            }
        }

        let env = |k: &str| env::var(format!("{}_{}", prefix, k)).ok();
        merged.provider_name = env("NAME").or(merged.provider_name);
        merged.endpoint = env("ENDPOINT").or(merged.endpoint);
        merged.model = env("MODEL").or(merged.model);
        merged.api_key_env_var = env("API_KEY_ENV").or(merged.api_key_env_var);
        if merged.api_key_env_var.is_none() {
            merged.api_key_env_var = Some("SAARAS_API_KEY".into());
        }
        merged.language = env("LANGUAGE").or(merged.language);
        if let Ok(v) = env::var(format!("{}_CODEMIX", prefix)) {
            merged.codemix = Some(v == "true" || v == "1");
        }
        merged.hotkey = env("HOTKEY").or(merged.hotkey);
        if cfg!(target_os = "windows")
            && merged.hotkey.as_deref() == Some("Ctrl+Alt+Shift+S")
        {
            merged.hotkey = Some(Self::default_hotkey());
        }
        merged.polish_mode = env("POLISH_MODE").or(merged.polish_mode);
        merged.polish_endpoint = env("POLISH_ENDPOINT").or(merged.polish_endpoint);
        merged.polish_model = env("POLISH_MODEL").or(merged.polish_model);
        merged.polish_api_key_env_var =
            env("POLISH_API_KEY_ENV").or(merged.polish_api_key_env_var);

        // Defaults
        if merged.endpoint.is_none() {
            merged.endpoint = Some("https://api.sarvam.ai/speech-to-text".into());
        }
        if merged.language.is_none() {
            merged.language = Some("hi-IN".into());
        }
        if merged.codemix.is_none() {
            merged.codemix = Some(true);
        }
        if merged.hotkey.is_none() {
            merged.hotkey = Some(Self::default_hotkey());
        }
        if merged.polish_mode.is_none() {
            merged.polish_mode = Some("light".into());
        }

        merged
    }

    fn config_dir() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            env::var("APPDATA").map(PathBuf::from).ok()
        }

        #[cfg(not(target_os = "windows"))]
        {
            env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .ok()
                .or_else(|| env::var("HOME").map(|home| PathBuf::from(home).join(".config")).ok())
        }
    }

    pub fn path(app_name: &str) -> Option<PathBuf> {
        Self::config_dir().map(|base| base.join(app_name).join("config.toml"))
    }

    pub fn api_key(&self) -> Option<String> {
        if let Some(raw) = self.api_key_env_var.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty()) {
            if let Ok(value) = env::var(raw) {
                return Some(value);
            }

            if raw.len() > 20 && !raw.chars().any(|c| c.is_whitespace()) {
                return Some(raw.to_string());
            }
        }

        env::var("SAARAS_API_KEY")
            .ok()
            .or_else(|| env::var("SAARAS_TRAY_API_KEY").ok())
    }

    pub fn polish_api_key(&self) -> Option<String> {
        if let Some(raw) = self
            .polish_api_key_env_var
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            if let Ok(value) = env::var(raw) {
                return Some(value);
            }

            if raw.len() > 20 && !raw.chars().any(|c| c.is_whitespace()) {
                return Some(raw.to_string());
            }
        }

        None
    }

    pub fn save(&self, app_name: &str) -> Result<PathBuf, String> {
        let path = Self::path(app_name).ok_or_else(|| "Unable to resolve config path".to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        let content = toml::to_string_pretty(&TomlConfig {
            provider: Some(self.clone()),
        })
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&path, content).map_err(|e| format!("Failed to write config: {}", e))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::env;

    #[test]
    fn api_key_uses_env_var_name_when_present() {
        env::set_var("SAARAS_TRAY_TEST_KEY", "env-secret");
        let config = Config {
            api_key_env_var: Some("SAARAS_TRAY_TEST_KEY".into()),
            ..Default::default()
        };

        assert_eq!(config.api_key().as_deref(), Some("env-secret"));
        env::remove_var("SAARAS_TRAY_TEST_KEY");
    }

    #[test]
    fn api_key_accepts_pasted_secret() {
        let config = Config {
            api_key_env_var: Some("sk_test_1234567890abcdef".into()),
            ..Default::default()
        };

        assert_eq!(
            config.api_key().as_deref(),
            Some("sk_test_1234567890abcdef")
        );
    }

    #[test]
    fn api_key_falls_back_to_direct_env_secret() {
        env::set_var("SAARAS_API_KEY", "direct-env-secret");
        let config = Config::default();

        assert_eq!(config.api_key().as_deref(), Some("direct-env-secret"));
        env::remove_var("SAARAS_API_KEY");
    }
}
