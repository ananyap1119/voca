use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use hound::{WavSpec, WavWriter};
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

pub struct AudioRecorder {}

#[derive(Debug, Clone)]
pub struct RecordingSummary {
    pub duration_ms: u64,
    pub peak_level: f32,
    pub sample_rate: u32,
    pub channels: u16,
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
        )
    }

    fn record_with_stop_policy(
        &self,
        path: &Path,
        min_duration: Duration,
        silence_duration: Duration,
        max_duration: Duration,
        stop_signal: Arc<AtomicBool>,
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

        let err_fn = |err| eprintln!("audio error: {}", err);
        let stream_config = config.config();
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                let writer_clone = writer.clone();
                let last_voice_at = last_voice_at.clone();
                let peak_level = peak_level.clone();
                device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let level = write_f32_samples(&writer_clone, data);
                            update_voice_activity(level, &last_voice_at, &peak_level);
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
                device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let level = write_i16_samples(&writer_clone, data);
                            update_voice_activity(level, &last_voice_at, &peak_level);
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
                device
                    .build_input_stream(
                        &stream_config,
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let level = write_u16_samples(&writer_clone, data);
                            update_voice_activity(level, &last_voice_at, &peak_level);
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
    use super::AudioRecorder;
    use std::fs;

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
}
