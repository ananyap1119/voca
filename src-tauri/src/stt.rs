use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttResult {
    pub text: String,
    pub confidence: Option<f32>,
    pub language: Option<String>,
    pub language_probability: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct SaarasResponse {
    transcript: String,
    #[serde(rename = "language_code")]
    language_code: Option<String>,
    language_probability: Option<f32>,
}

#[async_trait::async_trait]
pub trait SttProvider: Send + Sync {
    async fn transcribe(&self, audio_path: &Path) -> Result<SttResult, String>;
    async fn transcribe_with_model(
        &self,
        audio_path: &Path,
        model: &str,
    ) -> Result<SttResult, String>;
    fn name(&self) -> &str;
}

pub struct SaarasProvider {
    endpoint: String,
    api_key: Option<String>,
    model: String,
    language: String,
    codemix: bool,
}

impl SaarasProvider {
    pub fn new(
        endpoint: String,
        api_key: Option<String>,
        model: String,
        language: String,
        codemix: bool,
    ) -> Self {
        Self {
            endpoint,
            api_key,
            model,
            language,
            codemix,
        }
    }

    fn mode(&self) -> &'static str {
        if self.codemix {
            "codemix"
        } else {
            "transcribe"
        }
    }

    fn client() -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .user_agent("voca/0.1")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))
    }

    async fn request(&self, audio_path: &Path, model: &str) -> Result<SttResult, String> {
        let api_key = self.api_key.as_ref().ok_or("SAARAS_API_KEY not set")?;

        let file_bytes = tokio::fs::read(audio_path)
            .await
            .map_err(|e| format!("Failed to read audio file: {}", e))?;

        let file_name = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("audio/wav")
            .map_err(|e| e.to_string())?;

        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", model.to_owned())
            .text("mode", self.mode())
            .text("language_code", self.language.clone());

        let client = Self::client()?;
        let response = client
            .post(&self.endpoint)
            .header("api-subscription-key", api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", format_reqwest_error(e)))?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("Saaras API error ({}): {}", status, body));
        }

        parse_saaras_response(&body)
    }
}

fn parse_saaras_response(body: &str) -> Result<SttResult, String> {
    let parsed: SaarasResponse =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(SttResult {
        text: parsed.transcript,
        confidence: None,
        language: parsed.language_code,
        language_probability: parsed.language_probability,
    })
}

fn format_reqwest_error(error: reqwest::Error) -> String {
    let mut details = Vec::new();
    if error.is_timeout() {
        details.push("timeout".to_string());
    }
    if error.is_connect() {
        details.push("connect".to_string());
    }
    if error.is_request() {
        details.push("request".to_string());
    }

    let mut source = error.source();
    while let Some(inner) = source {
        details.push(inner.to_string());
        source = inner.source();
    }

    if details.is_empty() {
        error.to_string()
    } else {
        format!("{} ({})", error, details.join("; "))
    }
}

#[async_trait::async_trait]
impl SttProvider for SaarasProvider {
    fn name(&self) -> &str {
        &self.model
    }

    async fn transcribe(&self, audio_path: &Path) -> Result<SttResult, String> {
        self.request(audio_path, &self.model).await
    }

    async fn transcribe_with_model(
        &self,
        audio_path: &Path,
        model: &str,
    ) -> Result<SttResult, String> {
        self.request(audio_path, model).await
    }
}

pub type SharedProvider = Arc<Mutex<Box<dyn SttProvider>>>;

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_saaras_provider_name() {
        let provider = SaarasProvider::new(
            "https://example.invalid/speech-to-text".into(),
            Some("dummy".into()),
            "test-model".into(),
            "hi-IN".into(),
            true,
        );
        assert_eq!(provider.name(), "test-model");
    }

    #[test]
    fn test_saaras_provider_modes() {
        let codemix_provider = SaarasProvider::new(
            "https://example.invalid/speech-to-text".into(),
            Some("dummy".into()),
            "test-model".into(),
            "hi-IN".into(),
            true,
        );
        let transcribe_provider = SaarasProvider::new(
            "https://example.invalid/speech-to-text".into(),
            Some("dummy".into()),
            "test-model".into(),
            "hi-IN".into(),
            false,
        );

        assert_eq!(codemix_provider.mode(), "codemix");
        assert_eq!(transcribe_provider.mode(), "transcribe");
    }

    #[test]
    fn parses_optional_language_probability() {
        let with_probability = parse_saaras_response(
            r#"{"transcript":"namaste","language_code":"hi-IN","language_probability":0.91}"#,
        )
        .unwrap();
        assert_eq!(with_probability.language_probability, Some(0.91));

        let without_probability =
            parse_saaras_response(r#"{"transcript":"namaste","language_code":"hi-IN"}"#).unwrap();
        assert_eq!(without_probability.language_probability, None);
    }

    fn silent_wav_path() -> PathBuf {
        let path = std::env::temp_dir().join("voca-sarvam-smoke.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..16_000 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    #[tokio::test]
    #[ignore]
    async fn smoke_sarvam_transcribes_silent_wav() {
        let api_key =
            std::env::var("SAARAS_API_KEY").expect("Set SAARAS_API_KEY to run Sarvam smoke test");
        let audio = silent_wav_path();
        let provider = SaarasProvider::new(
            crate::config::Config::load("voca").endpoint.unwrap(),
            Some(api_key),
            crate::config::Config::load("voca").model.unwrap(),
            "hi-IN".into(),
            false,
        );

        let result = provider.transcribe(&audio).await.unwrap();
        assert_eq!(result.language.as_deref(), Some("hi-IN"));
        assert!(result.text.is_empty());
        let _ = fs::remove_file(audio);
    }
}
