use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

const CREDENTIAL_TARGET: &str = "Voca/SarvamApiKey";
const DEFAULT_CONFIG: &str = include_str!("../defaults.toml");

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
        let mut merged = toml::from_str::<TomlConfig>(DEFAULT_CONFIG)
            .ok()
            .and_then(|config| config.provider)
            .unwrap_or_default();

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

        if merged.hotkey.is_none() {
            merged.hotkey = Some(Self::default_hotkey());
        }

        let pasted_key = merged
            .api_key_env_var
            .as_deref()
            .map(str::trim)
            .filter(|value| value.len() >= 20 && !value.chars().any(char::is_whitespace))
            .map(str::to_owned);
        if let Some(secret) = pasted_key {
            if store_api_key(&secret).is_ok() {
                merged.api_key_env_var = toml::from_str::<TomlConfig>(DEFAULT_CONFIG)
                    .ok()
                    .and_then(|config| config.provider)
                    .and_then(|provider| provider.api_key_env_var);
                let _ = merged.save(app_name);
            }
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
        if let Some(raw) = self
            .api_key_env_var
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            if let Ok(value) = env::var(raw) {
                return Some(value);
            }
        }

        env::var("SAARAS_API_KEY")
            .ok()
            .or_else(|| stored_api_key().ok().flatten())
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

#[cfg(target_os = "windows")]
pub fn store_api_key(secret: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let secret = secret.trim();
    if secret.len() < 20 || secret.chars().any(char::is_whitespace) {
        return Err("Enter a valid Sarvam API key".into());
    }

    let mut target = CREDENTIAL_TARGET.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut username = "voca-user".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut blob = secret.as_bytes().to_vec();
    let mut credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(username.as_mut_ptr()),
        ..Default::default()
    };

    unsafe { CredWriteW(&mut credential, 0) }
        .map_err(|e| format!("Unable to save API key in Windows Credential Manager: {}", e))
}

#[cfg(not(target_os = "windows"))]
pub fn store_api_key(_secret: &str) -> Result<(), String> {
    Err("Secure API-key storage is currently available on Windows only".into())
}

#[cfg(target_os = "windows")]
pub fn stored_api_key() -> Result<Option<String>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target = CREDENTIAL_TARGET.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    if let Err(error) = unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    } {
        if error.code() == ERROR_NOT_FOUND.to_hresult() {
            return Ok(None);
        }
        return Err(format!("Unable to read API key from Windows Credential Manager: {}", error));
    }

    if credential.is_null() {
        return Ok(None);
    }

    let secret = unsafe {
        let value = &*credential;
        let bytes = std::slice::from_raw_parts(
            value.CredentialBlob,
            value.CredentialBlobSize as usize,
        );
        let decoded = String::from_utf8(bytes.to_vec())
            .map_err(|_| "Stored API key is not valid UTF-8".to_string());
        CredFree(credential.cast());
        decoded?
    };
    Ok(Some(secret))
}

#[cfg(not(target_os = "windows"))]
pub fn stored_api_key() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(target_os = "windows")]
pub fn delete_stored_api_key() -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = CREDENTIAL_TARGET.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(error) => Err(format!("Unable to remove API key: {}", error)),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn delete_stored_api_key() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::env;

    #[test]
    fn api_key_uses_env_var_name_when_present() {
        env::set_var("VOCA_TEST_KEY", "env-secret");
        let config = Config {
            api_key_env_var: Some("VOCA_TEST_KEY".into()),
            ..Default::default()
        };

        assert_eq!(config.api_key().as_deref(), Some("env-secret"));
        env::remove_var("VOCA_TEST_KEY");
    }

    #[test]
    fn api_key_falls_back_to_direct_env_secret() {
        env::set_var("SAARAS_API_KEY", "direct-env-secret");
        let config = Config::default();

        assert_eq!(config.api_key().as_deref(), Some("direct-env-secret"));
        env::remove_var("SAARAS_API_KEY");
    }

    #[test]
    fn shipped_defaults_are_complete() {
        let config = Config::load("voca-defaults-test");

        assert_eq!(config.provider_name.as_deref(), Some("sarvam"));
        assert!(config.endpoint.as_deref().is_some_and(|value| value.starts_with("https://")));
        assert!(config.model.as_deref().is_some_and(|value| !value.is_empty()));
    }
}
