use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use hound::{SampleFormat as WavSampleFormat, WavReader, WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MIN_RECORDING_SECONDS: u64 = 1;
const MAX_RECORDING_SECONDS: u64 = 300;
const STOP_AFTER_SILENCE_MS: u64 = 1600;
const VOICE_RMS_THRESHOLD: f32 = 0.018;
const LEVEL_REPORT_INTERVAL_MS: u64 = 33;

pub type LevelCallback = Arc<dyn Fn(f32) + Send + Sync>;

pub struct AudioRecorder {}

#[derive(Debug, Clone)]
pub struct RecordingSummary {
    pub duration_ms: u64,
    pub peak_level: f32,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn split_wav_for_api(path: &Path, max_seconds: u32) -> Result<Vec<std::path::PathBuf>, String> {
    if max_seconds == 0 {
        return Err("Audio chunk duration must be greater than zero".into());
    }

    let mut reader = WavReader::open(path).map_err(|e| format!("Failed to open recording: {}", e))?;
    let spec = reader.spec();
    if spec.bits_per_sample != 16 || spec.sample_format != WavSampleFormat::Int {
        return Err("Recorded WAV must use 16-bit PCM audio".into());
    }

    let samples_per_chunk = spec.sample_rate as usize
        * spec.channels as usize
        * max_seconds as usize;
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read recording: {}", e))?;

    if samples.len() <= samples_per_chunk {
        return Ok(vec![path.to_path_buf()]);
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("voca-recording");
    let mut paths = Vec::new();

    for (index, chunk) in samples.chunks(samples_per_chunk).enumerate() {
        let chunk_path = parent.join(format!("{}-part-{:03}.wav", stem, index + 1));
        let mut writer = WavWriter::create(&chunk_path, spec)
            .map_err(|e| format!("Failed to create audio chunk: {}", e))?;
        for sample in chunk {
            writer
                .write_sample(*sample)
                .map_err(|e| format!("Failed to write audio chunk: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize audio chunk: {}", e))?;
        paths.push(chunk_path);
    }

    Ok(paths)
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {}
    }

    #[allow(dead_code)]
    pub fn record_to_file(&self, path: &Path, duration_secs: u64) -> Result<(), String> {
        self.record_with_stop_policy(
            path,
            Duration::from_secs(duration_secs),
            Duration::from_secs(duration_secs),
            Duration::from_secs(duration_secs),
            Arc::new(AtomicBool::new(false)),
            None,
        )
        .map(|_| ())
    }

    #[allow(dead_code)]
    pub fn record_until_silence(&self, path: &Path) -> Result<RecordingSummary, String> {
        self.record_until_silence_or_stop(path, Arc::new(AtomicBool::new(false)))
    }

    pub fn record_until_silence_or_stop(
        &self,
        path: &Path,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<RecordingSummary, String> {
        self.record_with_stop_policy(
            path,
            Duration::from_secs(MIN_RECORDING_SECONDS),
            Duration::from_millis(STOP_AFTER_SILENCE_MS),
            Duration::from_secs(MAX_RECORDING_SECONDS),
            stop_signal,
            None,
        )
    }

    pub fn record_until_silence_or_stop_with_levels(
        &self,
        path: &Path,
        stop_signal: Arc<AtomicBool>,
        on_level: LevelCallback,
    ) -> Result<RecordingSummary, String> {
        self.record_with_stop_policy(
            path,
            Duration::from_secs(MIN_RECORDING_SECONDS),
            Duration::from_millis(STOP_AFTER_SILENCE_MS),
            Duration::from_secs(MAX_RECORDING_SECONDS),
            stop_signal,
            Some(on_level),
        )
    }

    fn record_with_stop_policy(
        &self,
        path: &Path,
        min_duration: Duration,
        silence_duration: Duration,
        max_duration: Duration,
        stop_signal: Arc<AtomicBool>,
        on_level: Option<LevelCallback>,
    ) -> Result<RecordingSummary, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No input device available")?;
        let config = device.default_input_config().map_err(|e| e.to_string())?;

        let spec = WavSpec {
            channels: config.channels(),
            sample_rate: config.sample_rate().0,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let writer = Arc::new(Mutex::new(Some(
            WavWriter::new(
                BufWriter::new(File::create(path).map_err(|e| e.to_string())?),
                spec,
            )
            .map_err(|e| e.to_string())?,
        )));
        let last_voice_at = Arc::new(Mutex::new(Instant::now()));
        let peak_level = Arc::new(Mutex::new(0.0_f32));
        let last_level_report_at = Arc::new(Mutex::new(
            Instant::now() - Duration::from_millis(LEVEL_REPORT_INTERVAL_MS),
        ));

        let err_fn = |err| eprintln!("audio error: {}", err);
        let stream_config = config.config();
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                let writer_clone = writer.clone();
                let last_voice_at = last_voice_at.clone();
                let peak_level = peak_level.clone();
                let on_level = on_level.clone();
                let last_level_report_at = last_level_report_at.clone();
                device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let level = write_f32_samples(&writer_clone, data);
                            update_voice_activity(level, &last_voice_at, &peak_level);
                            report_level(level, &on_level, &last_level_report_at);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| e.to_string())?
            }
            SampleFormat::I16 => {
                let writer_clone = writer.clone();
                let last_voice_at = last_voice_at.clone();
                let peak_level = peak_level.clone();
                let on_level = on_level.clone();
                let last_level_report_at = last_level_report_at.clone();
                device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let level = write_i16_samples(&writer_clone, data);
                            update_voice_activity(level, &last_voice_at, &peak_level);
                            report_level(level, &on_level, &last_level_report_at);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| e.to_string())?
            }
            SampleFormat::U16 => {
                let writer_clone = writer.clone();
                let last_voice_at = last_voice_at.clone();
                let peak_level = peak_level.clone();
                let on_level = on_level.clone();
                let last_level_report_at = last_level_report_at.clone();
                device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let level = write_u16_samples(&writer_clone, data);
                            update_voice_activity(level, &last_voice_at, &peak_level);
                            report_level(level, &on_level, &last_level_report_at);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| e.to_string())?
            }
            other => return Err(format!("Unsupported input sample format: {:?}", other)),
        };

        stream.play().map_err(|e| e.to_string())?;
        let started_at = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let elapsed = started_at.elapsed();
            if elapsed >= min_duration && stop_signal.load(Ordering::Relaxed) {
                break;
            }
            if elapsed >= max_duration {
                break;
            }

            let silent_for = last_voice_at
                .lock()
                .map(|last| last.elapsed())
                .unwrap_or_default();
            if elapsed >= min_duration && silent_for >= silence_duration {
                break;
            }
        }
        drop(stream);

        if let Some(callback) = &on_level {
            callback(0.0);
        }

        if let Ok(mut guard) = writer.lock() {
            if let Some(w) = guard.take() {
                w.finalize().map_err(|e| e.to_string())?;
            }
        }

        Ok(RecordingSummary {
            duration_ms: started_at.elapsed().as_millis() as u64,
            peak_level: peak_level.lock().map(|level| *level).unwrap_or_default(),
            sample_rate: spec.sample_rate,
            channels: spec.channels,
        })
    }

    pub fn probe_microphone(&self) -> Result<String, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No input device available")?;
        let name = device.name().map_err(|e| e.to_string())?;
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        Ok(format!(
            "Mic opened: {} ({} ch, {} Hz)",
            name,
            config.channels(),
            config.sample_rate().0
        ))
    }
}

fn report_level(
    level: f32,
    callback: &Option<LevelCallback>,
    last_report_at: &Arc<Mutex<Instant>>,
) {
    let Some(callback) = callback else {
        return;
    };

    if let Ok(mut last) = last_report_at.lock() {
        if last.elapsed() >= Duration::from_millis(LEVEL_REPORT_INTERVAL_MS) {
            callback(level);
            *last = Instant::now();
        }
    }
}

type SharedWriter = Arc<Mutex<Option<WavWriter<BufWriter<File>>>>>;

fn update_voice_activity(
    level: f32,
    last_voice_at: &Arc<Mutex<Instant>>,
    peak_level: &Arc<Mutex<f32>>,
) {
    if let Ok(mut peak) = peak_level.lock() {
        *peak = peak.max(level);
    }
    if level >= VOICE_RMS_THRESHOLD {
        if let Ok(mut last) = last_voice_at.lock() {
            *last = Instant::now();
        }
    }
}

fn write_i16_samples(writer: &SharedWriter, data: &[i16]) -> f32 {
    let mut sum = 0.0_f32;
    if let Ok(mut guard) = writer.lock() {
        if let Some(ref mut w) = *guard {
            for &sample in data {
                let _ = w.write_sample(sample);
                let normalized = sample as f32 / i16::MAX as f32;
                sum += normalized * normalized;
            }
        }
    }
    rms(sum, data.len())
}

fn write_f32_samples(writer: &SharedWriter, data: &[f32]) -> f32 {
    let mut sum = 0.0_f32;
    if let Ok(mut guard) = writer.lock() {
        if let Some(ref mut w) = *guard {
            for &sample in data {
                let clipped = sample.clamp(-1.0, 1.0);
                let sample_i16 = (clipped * i16::MAX as f32) as i16;
                let _ = w.write_sample(sample_i16);
                sum += clipped * clipped;
            }
        }
    }
    rms(sum, data.len())
}

fn write_u16_samples(writer: &SharedWriter, data: &[u16]) -> f32 {
    let mut sum = 0.0_f32;
    if let Ok(mut guard) = writer.lock() {
        if let Some(ref mut w) = *guard {
            for &sample in data {
                let centered = sample as i32 - 32768;
                let _ = w.write_sample(centered as i16);
                let normalized = centered as f32 / i16::MAX as f32;
                sum += normalized * normalized;
            }
        }
    }
    rms(sum, data.len())
}

fn rms(sum: f32, len: usize) -> f32 {
    if len == 0 {
        0.0
    } else {
        (sum / len as f32).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::{report_level, split_wav_for_api, AudioRecorder, LevelCallback, LEVEL_REPORT_INTERVAL_MS};
    use hound::{WavSpec, WavWriter};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    #[ignore]
    fn smoke_records_microphone_wav() {
        let path = std::env::temp_dir().join("voca-mic-smoke.wav");
        let recorder = AudioRecorder::new();
        let summary = recorder.record_until_silence(&path).unwrap();
        assert!(summary.duration_ms >= 1_000);

        let metadata = fs::metadata(&path).unwrap();
        assert!(metadata.len() > 44);

        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert!(spec.sample_rate > 0);
        assert!(spec.channels > 0);
        assert_eq!(spec.bits_per_sample, 16);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rms_detects_signal_level() {
        assert!(super::rms(4.0, 4) > 0.9);
        assert_eq!(super::rms(0.0, 0), 0.0);
    }

    #[test]
    fn level_callback_is_throttled() {
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = calls.clone();
        let callback: LevelCallback = Arc::new(move |_| {
            callback_calls.fetch_add(1, Ordering::Relaxed);
        });
        let last_report_at = Arc::new(Mutex::new(
            Instant::now() - Duration::from_millis(LEVEL_REPORT_INTERVAL_MS),
        ));

        report_level(0.2, &Some(callback.clone()), &last_report_at);
        report_level(0.3, &Some(callback), &last_report_at);

        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn long_wav_is_split_below_api_limit() {
        let path = std::env::temp_dir().join("voca-split-test.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..550 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();

        let chunks = split_wav_for_api(&path, 2).unwrap();
        assert_eq!(chunks.len(), 3);
        for chunk in &chunks {
            let reader = hound::WavReader::open(chunk).unwrap();
            assert!(reader.len() <= 200);
            let _ = fs::remove_file(chunk);
        }
        let _ = fs::remove_file(path);
    }
}
