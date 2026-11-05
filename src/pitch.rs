//! Pitch detection tuned for bass guitar.
//!
//! Pipeline: raw mono samples → 4th-order low-pass → decimate to ~8 kHz →
//! sliding window → McLeod Pitch Method (NSDF + peak picking + parabolic
//! interpolation).

/// Lowest frequency we try to detect (below low B0 = 30.87 Hz).
pub const MIN_FREQ: f32 = 27.0;
/// Highest frequency we try to detect (above C3 = 130.81 Hz, with headroom
/// for harmonics/high frets).
pub const MAX_FREQ: f32 = 420.0;

/// Target rate after decimation. Bass fundamentals are all below ~400 Hz.
const TARGET_RATE: f32 = 8000.0;
/// Low-pass cutoff applied before decimation.
const LOWPASS_HZ: f32 = 900.0;
/// Analysis window length in decimated samples (~128 ms at 8 kHz, ≥ 3 periods of low B).
const WINDOW: usize = 1024;
/// MPM threshold relative to the highest NSDF peak.
const MPM_K: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    pub freq: f32,
    /// 0..1, how periodic the signal is. Noise is low, a clean note is > 0.9.
    pub clarity: f32,
}

/// RBJ biquad low-pass filter.
#[derive(Debug, Clone)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn lowpass(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        // Transposed direct form II.
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Streaming pitch detector. Feed it raw samples, ask it for a detection.
pub struct PitchDetector {
    factor: usize,
    rate: f32,
    filters: [Biquad; 2],
    phase: usize,
    /// Circular buffer of decimated samples.
    ring: Vec<f32>,
    pos: usize,
    filled: usize,
    /// Scratch buffers reused between calls.
    frame: Vec<f32>,
    nsdf: Vec<f32>,
}

impl PitchDetector {
    pub fn new(input_rate: u32) -> Self {
        let input_rate = input_rate as f32;
        let factor = ((input_rate / TARGET_RATE).floor() as usize).max(1);
        // Butterworth 4th order = two biquads with these Qs.
        let filters = [
            Biquad::lowpass(input_rate, LOWPASS_HZ, 0.541_196_1),
            Biquad::lowpass(input_rate, LOWPASS_HZ, 1.306_563),
        ];
        let rate = input_rate / factor as f32;
        let max_lag = (rate / MIN_FREQ).ceil() as usize + 2;
        Self {
            factor,
            rate,
            filters,
            phase: 0,
            ring: vec![0.0; WINDOW],
            pos: 0,
            filled: 0,
            frame: vec![0.0; WINDOW],
            nsdf: vec![0.0; max_lag + 1],
        }
    }

    /// Push raw mono samples (at the input rate). Returns how many decimated
    /// samples were added.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let mut added = 0;
        for &s in samples {
            let y = self.filters[0].process(s);
            let y = self.filters[1].process(y);
            self.phase += 1;
            if self.phase >= self.factor {
                self.phase = 0;
                self.ring[self.pos] = y;
                self.pos = (self.pos + 1) % WINDOW;
                self.filled = (self.filled + 1).min(WINDOW);
                added += 1;
            }
        }
        added
    }

    /// RMS level of the current window (linear, 0..1 for full-scale input).
    pub fn rms(&self) -> f32 {
        if self.filled == 0 {
            return 0.0;
        }
        let sum: f32 = self.ring.iter().map(|x| x * x).sum();
        (sum / self.filled as f32).sqrt()
    }

    /// Run MPM on the current window.
    pub fn detect(&mut self) -> Option<Detection> {
        if self.filled < WINDOW {
            return None;
        }
        // Unroll ring into chronological order and remove DC.
        let (a, b) = self.ring.split_at(self.pos);
        self.frame[..b.len()].copy_from_slice(b);
        self.frame[b.len()..].copy_from_slice(a);
        let mean = self.frame.iter().sum::<f32>() / WINDOW as f32;
        self.frame.iter_mut().for_each(|x| *x -= mean);

        mpm(&self.frame, self.rate, &mut self.nsdf)
    }
}

/// McLeod Pitch Method on one frame. `nsdf` is scratch space whose length
/// sets the maximum lag considered.
fn mpm(x: &[f32], rate: f32, nsdf: &mut [f32]) -> Option<Detection> {
    let n = x.len();
    let max_lag = (nsdf.len() - 1).min(n / 2);
    let min_lag = (rate / MAX_FREQ).floor() as usize;

    // Normalized square difference function.
    for tau in 0..=max_lag {
        let mut acf = 0.0f32;
        let mut m = 0.0f32;
        for j in 0..n - tau {
            acf += x[j] * x[j + tau];
            m += x[j] * x[j] + x[j + tau] * x[j + tau];
        }
        nsdf[tau] = if m > 0.0 { 2.0 * acf / m } else { 0.0 };
    }
    let nsdf = &nsdf[..=max_lag];

    // Key maxima: the highest point in each positive lobe after the first
    // negative-going zero crossing.
    let mut maxima: Vec<usize> = Vec::with_capacity(16);
    let mut tau = 1;
    while tau < max_lag && nsdf[tau] > 0.0 {
        tau += 1;
    }
    while tau < max_lag {
        while tau < max_lag && nsdf[tau] <= 0.0 {
            tau += 1;
        }
        let mut best: Option<usize> = None;
        while tau < max_lag && nsdf[tau] > 0.0 {
            if nsdf[tau] > nsdf[tau - 1]
                && nsdf[tau] >= nsdf[tau + 1]
                && best.is_none_or(|b| nsdf[tau] > nsdf[b])
            {
                best = Some(tau);
            }
            tau += 1;
        }
        if let Some(b) = best
            && b >= min_lag
        {
            maxima.push(b);
        }
    }

    let highest = maxima.iter().map(|&t| nsdf[t]).fold(0.0f32, f32::max);
    if highest <= 0.0 {
        return None;
    }
    let threshold = MPM_K * highest;
    let t = *maxima.iter().find(|&&t| nsdf[t] >= threshold)?;

    // Parabolic interpolation around the chosen peak.
    let (y0, y1, y2) = (nsdf[t - 1], nsdf[t], nsdf[t + 1]);
    let denom = y0 - 2.0 * y1 + y2;
    let (offset, peak) = if denom.abs() > f32::EPSILON {
        let d = 0.5 * (y0 - y2) / denom;
        (d, y1 - 0.25 * (y0 - y2) * d)
    } else {
        (0.0, y1)
    };
    let period = t as f32 + offset;
    let freq = rate / period;
    (MIN_FREQ..=MAX_FREQ).contains(&freq).then_some(Detection {
        freq,
        clarity: peak.clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn cents(a: f32, b: f32) -> f32 {
        1200.0 * (a / b).log2()
    }

    /// Bass-like tone: weak fundamental, strong 2nd/3rd harmonics.
    fn bass_tone(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let harmonics = [(1.0, 0.25), (2.0, 1.0), (3.0, 0.6), (4.0, 0.35), (5.0, 0.2)];
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                harmonics
                    .iter()
                    .map(|&(h, a)| a * (TAU * freq * h * t).sin())
                    .sum::<f32>()
                    * 0.2
            })
            .collect()
    }

    fn detect_tone(samples: &[f32], rate: u32) -> Option<Detection> {
        let mut d = PitchDetector::new(rate);
        d.push(samples);
        d.detect()
    }

    #[test]
    fn detects_pure_sines_on_all_bass_strings() {
        for rate in [44_100, 48_000, 96_000] {
            for f in [30.87, 41.20, 55.0, 73.42, 98.0, 130.81] {
                let s: Vec<f32> = (0..rate / 2)
                    .map(|i| 0.5 * (TAU * f * i as f32 / rate as f32).sin())
                    .collect();
                let d = detect_tone(&s, rate).expect("no detection");
                assert!(
                    cents(d.freq, f).abs() < 1.0,
                    "{rate} Hz, {f}: got {}",
                    d.freq
                );
                assert!(d.clarity > 0.95);
            }
        }
    }

    #[test]
    fn harmonic_rich_tone_does_not_jump_an_octave() {
        for f in [30.87, 41.20, 55.0, 73.42, 98.0] {
            let d = detect_tone(&bass_tone(f, 48_000, 0.5), 48_000).expect("no detection");
            assert!(cents(d.freq, f).abs() < 1.0, "{f}: got {}", d.freq);
        }
    }

    #[test]
    fn slightly_out_of_tune_is_measured() {
        let f = 41.20 * 2f32.powf(-15.0 / 1200.0); // 15 cents flat
        let d = detect_tone(&bass_tone(f, 48_000, 0.5), 48_000).unwrap();
        assert!((cents(d.freq, 41.20) + 15.0).abs() < 1.0, "got {}", d.freq);
    }

    #[test]
    fn noise_has_low_clarity_or_no_detection() {
        // Deterministic white noise (xorshift).
        let mut s = 0x1234_5678u32;
        let noise: Vec<f32> = (0..24_000)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s as f32 / u32::MAX as f32 - 0.5) * 0.5
            })
            .collect();
        if let Some(d) = detect_tone(&noise, 48_000) {
            assert!(d.clarity < 0.8, "clarity {}", d.clarity);
        }
    }

    #[test]
    fn silence_gives_nothing() {
        assert_eq!(detect_tone(&vec![0.0; 24_000], 48_000), None);
    }
}
