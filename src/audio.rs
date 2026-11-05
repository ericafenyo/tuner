//! Audio capture (cpal) and the background analysis thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::pitch::PitchDetector;

/// Ignore input quieter than this (dBFS).
const GATE_DB: f32 = -55.0;
/// Detections less periodic than this are discarded.
const MIN_CLARITY: f32 = 0.85;
/// How often to run detection.
const HOP: Duration = Duration::from_millis(20);
/// Number of recent estimates the median filter looks at.
const MEDIAN_LEN: usize = 5;

/// Latest analysis result, shared with the UI.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    /// Smoothed frequency of the current note, if one is ringing.
    pub freq: Option<f32>,
    /// When `freq` was last updated.
    pub updated: Option<Instant>,
    /// Input level in dBFS.
    pub level_db: f32,
}

pub fn input_device_names() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| {
            devs.filter_map(|d| d.description().ok().map(|d| d.name().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|d| d.name().to_string())
}

/// A running capture. Dropping it stops audio and the analysis thread.
pub struct Engine {
    _stream: cpal::Stream,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub device_name: String,
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Engine {
    /// Start capturing from the named device (or the default one).
    /// `on_update` is called from the analysis thread after each new reading.
    pub fn start(
        device_name: Option<&str>,
        reading: Arc<Mutex<Reading>>,
        on_update: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match device_name {
            Some(name) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.description().is_ok_and(|desc| desc.name() == name))
                .ok_or_else(|| format!("Input device \"{name}\" not found"))?,
            None => host
                .default_input_device()
                .ok_or("No input device found. Is a microphone or audio interface connected?")?,
        };
        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "Unknown device".into());

        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let sample_rate = config.sample_rate;
        let channels = config.channels as usize;

        // ~1 s of mono audio between the callback and the analysis thread.
        let (producer, consumer) = HeapRb::<f32>::new(sample_rate as usize).split();

        use cpal::SampleFormat as F;
        let stream = match supported.sample_format() {
            F::F32 => build::<f32>(&device, config, channels, producer),
            F::I16 => build::<i16>(&device, config, channels, producer),
            F::I32 => build::<i32>(&device, config, channels, producer),
            F::U16 => build::<u16>(&device, config, channels, producer),
            F::U8 => build::<u8>(&device, config, channels, producer),
            F::I8 => build::<i8>(&device, config, channels, producer),
            F::F64 => build::<f64>(&device, config, channels, producer),
            other => return Err(format!("Unsupported sample format {other}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("pitch-analysis".into())
                .spawn(move || analysis_loop(consumer, sample_rate, reading, stop, on_update))
                .map_err(|e| e.to_string())?
        };

        Ok(Self {
            _stream: stream,
            stop,
            thread: Some(thread),
            device_name,
        })
    }
}

type Prod = HeapProd<f32>;
type Cons = HeapCons<f32>;

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    mut producer: Prod,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    // Fixed scratch buffer so the audio callback never allocates.
    let mut mono = vec![0.0f32; 8192];
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                for chunk in data.chunks(mono.len() * channels) {
                    let frames = chunk.len() / channels;
                    for (i, frame) in chunk.chunks_exact(channels).enumerate() {
                        let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
                        mono[i] = sum / channels as f32;
                    }
                    // If analysis falls behind, drop samples rather than block.
                    producer.push_slice(&mono[..frames]);
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn analysis_loop(
    mut consumer: Cons,
    sample_rate: u32,
    reading: Arc<Mutex<Reading>>,
    stop: Arc<AtomicBool>,
    on_update: impl Fn(),
) {
    let mut detector = PitchDetector::new(sample_rate);
    let mut buf = vec![0.0f32; 4096];
    let mut history: Vec<f32> = Vec::with_capacity(MEDIAN_LEN);
    let mut last_run = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        let n = consumer.pop_slice(&mut buf);
        if n > 0 {
            detector.push(&buf[..n]);
        }
        if last_run.elapsed() < HOP {
            if n == 0 {
                std::thread::sleep(Duration::from_millis(4));
            }
            continue;
        }
        last_run = Instant::now();

        let level_db = 20.0 * detector.rms().max(1e-9).log10();
        let detection = (level_db > GATE_DB)
            .then(|| detector.detect())
            .flatten()
            .filter(|d| d.clarity >= MIN_CLARITY);

        let smoothed = detection.map(|d| {
            // Start fresh when the pitch jumps by more than a semitone (new note).
            if let Some(&last) = history.last()
                && (crate::notes::cents(d.freq, last)).abs() > 100.0
            {
                history.clear();
            }
            if history.len() == MEDIAN_LEN {
                history.remove(0);
            }
            history.push(d.freq);
            let mut sorted = history.clone();
            sorted.sort_by(f32::total_cmp);
            sorted[sorted.len() / 2]
        });

        if let Ok(mut r) = reading.lock() {
            r.level_db = level_db;
            if let Some(f) = smoothed {
                r.freq = Some(f);
                r.updated = Some(Instant::now());
            }
        }
        on_update();
    }
}
