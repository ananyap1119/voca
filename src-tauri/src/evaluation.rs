use crate::stt::{SttProvider, SttResult};
use serde::Serialize;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const SAARAS_V3: &str = "saaras:v3";
pub const SAARAS_V4: &str = "saaras:v4";

#[derive(Debug, Clone, Serialize)]
pub struct ModelEvaluation {
    pub model: String,
    pub raw_transcript: Option<String>,
    pub returned_language_code: Option<String>,
    pub language_probability: Option<f32>,
    pub request_latency_ms: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationRun {
    pub run_number: usize,
    pub timestamp_ms: u64,
    pub expected_text: Option<String>,
    pub selected_language_code: String,
    pub mode: String,
    pub audio_duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub request_order: Vec<String>,
    pub v3: ModelEvaluation,
    pub v4: ModelEvaluation,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct EvaluationSession {
    pub enabled: bool,
    pub expected_text: Option<String>,
    pub runs: Vec<EvaluationRun>,
}

impl EvaluationSession {
    pub fn set_settings(&mut self, enabled: bool, expected_text: Option<String>) {
        self.enabled = enabled;
        self.expected_text = expected_text.and_then(|text| {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        });
    }

    pub fn clear(&mut self) {
        self.expected_text = None;
        self.runs.clear();
    }

    pub fn next_run_number(&self) -> usize {
        self.runs.len() + 1
    }
}

#[derive(Debug, Clone)]
pub struct EvaluationMetadata {
    pub run_number: usize,
    pub expected_text: Option<String>,
    pub selected_language_code: String,
    pub mode: String,
    pub audio_duration_ms: u64,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn request_order_for_run(run_number: usize) -> [&'static str; 2] {
    if run_number % 2 == 1 {
        [SAARAS_V3, SAARAS_V4]
    } else {
        [SAARAS_V4, SAARAS_V3]
    }
}

async fn evaluate_one(
    provider: &dyn SttProvider,
    audio_path: &Path,
    model: &str,
) -> ModelEvaluation {
    let started = Instant::now();
    let result = provider.transcribe_with_model(audio_path, model).await;
    let request_latency_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok(SttResult {
            text,
            language,
            language_probability,
            ..
        }) => ModelEvaluation {
            model: model.to_owned(),
            raw_transcript: Some(text),
            returned_language_code: language,
            language_probability,
            request_latency_ms,
            success: true,
            error: None,
        },
        Err(error) => ModelEvaluation {
            model: model.to_owned(),
            raw_transcript: None,
            returned_language_code: None,
            language_probability: None,
            request_latency_ms,
            success: false,
            error: Some(error),
        },
    }
}

pub async fn evaluate_models(
    provider: &dyn SttProvider,
    audio_path: &Path,
    metadata: EvaluationMetadata,
) -> EvaluationRun {
    let order = request_order_for_run(metadata.run_number);
    let first = evaluate_one(provider, audio_path, order[0]).await;
    let second = evaluate_one(provider, audio_path, order[1]).await;
    let (v3, v4) = if order[0] == SAARAS_V3 {
        (first, second)
    } else {
        (second, first)
    };

    EvaluationRun {
        run_number: metadata.run_number,
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        expected_text: metadata.expected_text,
        selected_language_code: metadata.selected_language_code,
        mode: metadata.mode,
        audio_duration_ms: metadata.audio_duration_ms,
        sample_rate: metadata.sample_rate,
        channels: metadata.channels,
        request_order: order.iter().map(|model| (*model).to_owned()).collect(),
        v3,
        v4,
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

pub fn session_to_csv(session: &EvaluationSession) -> String {
    let headers = [
        "run_number",
        "timestamp_ms",
        "expected_text",
        "selected_language_code",
        "mode",
        "audio_duration_ms",
        "sample_rate",
        "channels",
        "request_order",
        "v3_model",
        "v3_success",
        "v3_transcript",
        "v3_language",
        "v3_language_probability",
        "v3_latency_ms",
        "v3_error",
        "v4_model",
        "v4_success",
        "v4_transcript",
        "v4_language",
        "v4_language_probability",
        "v4_latency_ms",
        "v4_error",
    ];
    let mut csv = format!("{}\n", headers.join(","));

    for run in &session.runs {
        let values = vec![
            run.run_number.to_string(),
            run.timestamp_ms.to_string(),
            run.expected_text.clone().unwrap_or_default(),
            run.selected_language_code.clone(),
            run.mode.clone(),
            run.audio_duration_ms.to_string(),
            run.sample_rate.to_string(),
            run.channels.to_string(),
            run.request_order.join(" -> "),
            run.v3.model.clone(),
            run.v3.success.to_string(),
            run.v3.raw_transcript.clone().unwrap_or_default(),
            run.v3.returned_language_code.clone().unwrap_or_default(),
            run.v3
                .language_probability
                .map(|value| value.to_string())
                .unwrap_or_default(),
            run.v3.request_latency_ms.to_string(),
            run.v3.error.clone().unwrap_or_default(),
            run.v4.model.clone(),
            run.v4.success.to_string(),
            run.v4.raw_transcript.clone().unwrap_or_default(),
            run.v4.returned_language_code.clone().unwrap_or_default(),
            run.v4
                .language_probability
                .map(|value| value.to_string())
                .unwrap_or_default(),
            run.v4.request_latency_ms.to_string(),
            run.v4.error.clone().unwrap_or_default(),
        ];
        csv.push_str(
            &values
                .iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }

    csv
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    struct MockProvider {
        calls: Mutex<Vec<(PathBuf, Vec<u8>, String)>>,
        outcomes: HashMap<String, Result<SttResult, String>>,
    }

    #[async_trait]
    impl SttProvider for MockProvider {
        async fn transcribe(&self, audio_path: &Path) -> Result<SttResult, String> {
            self.transcribe_with_model(audio_path, SAARAS_V3).await
        }

        async fn transcribe_with_model(
            &self,
            audio_path: &Path,
            model: &str,
        ) -> Result<SttResult, String> {
            self.calls.lock().unwrap().push((
                audio_path.to_path_buf(),
                std::fs::read(audio_path).unwrap(),
                model.to_owned(),
            ));
            self.outcomes.get(model).cloned().unwrap()
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    fn success(text: &str) -> Result<SttResult, String> {
        Ok(SttResult {
            text: text.to_owned(),
            confidence: None,
            language: Some("hi-IN".into()),
            language_probability: Some(0.9),
        })
    }

    fn metadata(run_number: usize) -> EvaluationMetadata {
        EvaluationMetadata {
            run_number,
            expected_text: Some("expected".into()),
            selected_language_code: "hi-IN".into(),
            mode: "codemix".into(),
            audio_duration_ms: 1_500,
            sample_rate: 16_000,
            channels: 1,
        }
    }

    fn provider(v3: Result<SttResult, String>, v4: Result<SttResult, String>) -> MockProvider {
        MockProvider {
            calls: Mutex::new(Vec::new()),
            outcomes: HashMap::from([(SAARAS_V3.into(), v3), (SAARAS_V4.into(), v4)]),
        }
    }

    #[tokio::test]
    async fn identical_audio_and_correct_model_ids_are_used() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("one-recording.wav");
        std::fs::write(&audio, b"same audio bytes").unwrap();
        let provider = provider(success("v3"), success("v4"));

        evaluate_models(&provider, &audio, metadata(1)).await;
        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, audio);
        assert_eq!(calls[1].0, audio);
        assert_eq!(calls[0].1, calls[1].1);
        assert_eq!(calls[0].2, SAARAS_V3);
        assert_eq!(calls[1].2, SAARAS_V4);
    }

    #[tokio::test]
    async fn assembles_two_successful_results() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("audio.wav");
        std::fs::write(&audio, b"audio").unwrap();
        let run = evaluate_models(
            &provider(success("three"), success("four")),
            &audio,
            metadata(1),
        )
        .await;

        assert!(run.v3.success);
        assert!(run.v4.success);
        assert_eq!(run.v3.raw_transcript.as_deref(), Some("three"));
        assert_eq!(run.v4.raw_transcript.as_deref(), Some("four"));
    }

    #[tokio::test]
    async fn isolates_v4_failure_from_v3_success() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("audio.wav");
        std::fs::write(&audio, b"audio").unwrap();
        let run = evaluate_models(
            &provider(success("three"), Err("v4 rejected mode".into())),
            &audio,
            metadata(1),
        )
        .await;

        assert!(run.v3.success);
        assert!(!run.v4.success);
        assert_eq!(run.v4.error.as_deref(), Some("v4 rejected mode"));
    }

    #[tokio::test]
    async fn isolates_v3_failure_from_v4_success() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("audio.wav");
        std::fs::write(&audio, b"audio").unwrap();
        let run = evaluate_models(
            &provider(Err("v3 failed".into()), success("four")),
            &audio,
            metadata(2),
        )
        .await;

        assert!(!run.v3.success);
        assert!(run.v4.success);
        assert_eq!(run.request_order, vec![SAARAS_V4, SAARAS_V3]);
    }

    #[tokio::test]
    async fn preserves_both_errors_when_both_models_fail() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("audio.wav");
        std::fs::write(&audio, b"audio").unwrap();
        let run = evaluate_models(
            &provider(Err("v3 status 400".into()), Err("v4 status 422".into())),
            &audio,
            metadata(1),
        )
        .await;

        assert_eq!(run.v3.error.as_deref(), Some("v3 status 400"));
        assert_eq!(run.v4.error.as_deref(), Some("v4 status 422"));
    }

    #[test]
    fn request_order_alternates() {
        assert_eq!(request_order_for_run(1), [SAARAS_V3, SAARAS_V4]);
        assert_eq!(request_order_for_run(2), [SAARAS_V4, SAARAS_V3]);
        assert_eq!(request_order_for_run(3), [SAARAS_V3, SAARAS_V4]);
    }

    #[test]
    fn clear_removes_session_data_but_not_mode() {
        let mut session = EvaluationSession {
            enabled: true,
            expected_text: Some("private text".into()),
            runs: Vec::new(),
        };
        session.clear();
        assert!(session.enabled);
        assert!(session.expected_text.is_none());
        assert!(session.runs.is_empty());
    }

    #[test]
    fn csv_escapes_commas_quotes_and_newlines() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn evaluation_is_off_by_default_for_normal_dictation() {
        assert!(!EvaluationSession::default().enabled);
    }
}
