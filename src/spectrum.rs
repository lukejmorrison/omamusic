use crate::paths::which;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const BAND_EDGES: [f64; 11] = [
    20.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0, 12800.0, 16000.0, 20000.0,
];
pub const SAMPLE_RATE: usize = 44100;
pub const WINDOW: usize = 2048;
pub const NUM_BANDS: usize = 10;

fn hann_window(size: usize) -> Vec<f64> {
    if size <= 1 {
        return vec![1.0; size];
    }
    (0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (size as f64 - 1.0)).cos()))
        .collect()
}

fn hann() -> &'static [f64] {
    use std::sync::OnceLock;
    static HANN: OnceLock<Vec<f64>> = OnceLock::new();
    HANN.get_or_init(|| hann_window(WINDOW))
}

pub fn fft_magnitudes(samples: &[f64]) -> Vec<f64> {
    let n = samples.len();
    if n == 0 || n & (n - 1) != 0 {
        return vec![];
    }
    let mut real = samples.to_vec();
    let mut imag = vec![0.0; n];
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            real.swap(i, j);
            imag.swap(i, j);
        }
    }
    let mut length = 2usize;
    while length <= n {
        let angle = -2.0 * std::f64::consts::PI / length as f64;
        let wlen_r = angle.cos();
        let wlen_i = angle.sin();
        let half = length / 2;
        for start in (0..n).step_by(length) {
            let mut wr = 1.0;
            let mut wi = 0.0;
            for k in 0..half {
                let even = start + k;
                let odd = even + half;
                let vr = real[odd] * wr - imag[odd] * wi;
                let vi = real[odd] * wi + imag[odd] * wr;
                real[odd] = real[even] - vr;
                imag[odd] = imag[even] - vi;
                real[even] += vr;
                imag[even] += vi;
                let nwr = wr * wlen_r - wi * wlen_i;
                wi = wr * wlen_i + wi * wlen_r;
                wr = nwr;
            }
        }
        length <<= 1;
    }
    (0..n / 2).map(|i| real[i].hypot(imag[i])).collect()
}

pub fn analyze(samples: &[f64], prev: Option<&[f64]>, sample_rate: usize) -> Vec<f64> {
    let mut previous = prev
        .map(|p| p.to_vec())
        .unwrap_or_else(|| vec![0.0; NUM_BANDS]);
    if previous.len() != NUM_BANDS {
        previous = vec![0.0; NUM_BANDS];
    }
    if samples.is_empty() {
        return previous.iter().map(|level| level * 0.8).collect();
    }
    let mut windowed: Vec<f64> = samples.iter().copied().take(WINDOW).collect();
    if windowed.len() < WINDOW {
        windowed.resize(WINDOW, 0.0);
    }
    let hann = hann();
    for i in 0..WINDOW {
        windowed[i] *= hann[i];
    }
    let spectrum = fft_magnitudes(&windowed);
    if spectrum.is_empty() {
        return previous.iter().map(|level| level * 0.8).collect();
    }
    let bin_hz = sample_rate as f64 / WINDOW as f64;
    let half_len = spectrum.len();
    let mut bands = Vec::with_capacity(NUM_BANDS);
    for b in 0..NUM_BANDS {
        let mut lo_idx = (BAND_EDGES[b] / bin_hz) as usize;
        let mut hi_idx = (BAND_EDGES[b + 1] / bin_hz) as usize;
        if lo_idx < 1 {
            lo_idx = 1;
        }
        if hi_idx >= half_len {
            hi_idx = half_len - 1;
        }
        let mut total = 0.0;
        let mut count = 0;
        if hi_idx >= lo_idx {
            for item in spectrum.iter().take(hi_idx + 1).skip(lo_idx) {
                total += *item;
                count += 1;
            }
        }
        let average = if count > 0 { total / count as f64 } else { 0.0 };
        let mut level = if average > 0.0 {
            (20.0 * average.log10() + 10.0) / 50.0
        } else {
            0.0
        };
        level = level.clamp(0.0, 1.0);
        let last = previous[b];
        if level > last {
            level = level * 0.6 + last * 0.4;
        } else {
            level = level * 0.25 + last * 0.75;
        }
        bands.push(level);
    }
    bands
}

pub fn default_monitor_source() -> String {
    let Some(pactl) = which("pactl") else {
        return String::new();
    };
    let sink = Command::new(&pactl)
        .arg("get-default-sink")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if !sink.is_empty() {
        return format!("{sink}.monitor");
    }
    let listing = Command::new(&pactl)
        .args(["list", "short", "sources"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).into_owned())
            } else {
                None
            }
        })
        .unwrap_or_default();
    for line in listing.lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1].ends_with(".monitor") {
            return parts[1].to_string();
        }
    }
    String::new()
}

pub struct SpectrumTap {
    levels: Arc<Mutex<Vec<f64>>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Default for SpectrumTap {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumTap {
    pub fn new() -> Self {
        Self {
            levels: Arc::new(Mutex::new(vec![0.0; NUM_BANDS])),
            stop: Arc::new(AtomicBool::new(false)),
            thread: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> Vec<f64> {
        self.levels.lock().unwrap().clone()
    }

    pub fn start(&self) {
        let mut thread = self.thread.lock().unwrap();
        if thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        self.stop.store(false, Ordering::SeqCst);
        let levels = Arc::clone(&self.levels);
        let stop = Arc::clone(&self.stop);
        *thread = Some(thread::spawn(move || spectrum_loop(levels, stop)));
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn shutdown(&self) {
        self.stop();
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

fn spectrum_loop(levels: Arc<Mutex<Vec<f64>>>, stop: Arc<AtomicBool>) {
    let chunk = WINDOW * 4;
    while !stop.load(Ordering::SeqCst) {
        let monitor = default_monitor_source();
        let Some(parec) = which("parec") else {
            decay_levels(&levels);
            thread::sleep(Duration::from_millis(800));
            continue;
        };
        if monitor.is_empty() {
            decay_levels(&levels);
            thread::sleep(Duration::from_millis(800));
            continue;
        }
        let mut child = match Command::new(parec)
            .args([
                "--format=float32le",
                &format!("--rate={SAMPLE_RATE}"),
                "--channels=1",
                "-d",
                &monitor,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                decay_levels(&levels);
                thread::sleep(Duration::from_millis(800));
                continue;
            }
        };
        if let Some(mut stdout) = child.stdout.take() {
            let mut buf = vec![0u8; chunk];
            while !stop.load(Ordering::SeqCst) {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let count = n / 4;
                        if count == 0 {
                            continue;
                        }
                        let mut samples = Vec::with_capacity(count);
                        for i in 0..count {
                            let bytes =
                                [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
                            samples.push(f32::from_le_bytes(bytes) as f64);
                        }
                        let previous = levels.lock().unwrap().clone();
                        let next = analyze(&samples, Some(&previous), SAMPLE_RATE);
                        *levels.lock().unwrap() = next;
                    }
                    Err(_) => break,
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        if !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(400));
        }
    }
}

fn decay_levels(levels: &Arc<Mutex<Vec<f64>>>) {
    let mut guard = levels.lock().unwrap();
    for level in guard.iter_mut() {
        *level *= 0.8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, n: usize, sr: usize, amp: f64) -> Vec<f64> {
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / sr as f64).sin())
            .collect()
    }

    #[test]
    fn empty_samples_decay() {
        let previous = vec![1.0; NUM_BANDS];
        let decayed = analyze(&[], Some(&previous), SAMPLE_RATE);
        assert_eq!(decayed.len(), NUM_BANDS);
        assert!(decayed.iter().all(|value| (value - 0.8).abs() < 1e-9));
    }

    #[test]
    fn fft_peaks_on_tone() {
        let samples = sine(1000.0, WINDOW, SAMPLE_RATE, 0.6);
        let windowed: Vec<f64> = samples
            .iter()
            .enumerate()
            .map(|(i, s)| {
                s * 0.5
                    * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (WINDOW as f64 - 1.0)).cos())
            })
            .collect();
        let magnitudes = fft_magnitudes(&windowed);
        let bin_hz = SAMPLE_RATE as f64 / WINDOW as f64;
        let peak = (1..magnitudes.len())
            .max_by(|a, b| magnitudes[*a].partial_cmp(&magnitudes[*b]).unwrap())
            .unwrap();
        assert!((peak as f64 * bin_hz - 1000.0).abs() < bin_hz * 2.0);
    }

    #[test]
    fn thousand_hertz_dominates_its_band() {
        let mut levels = vec![0.0; NUM_BANDS];
        let samples = sine(1000.0, WINDOW, SAMPLE_RATE, 0.6);
        for _ in 0..8 {
            levels = analyze(&samples, Some(&levels), SAMPLE_RATE);
        }
        assert!(levels[4] > 0.15);
        assert!(levels[4] > levels[0]);
        assert!(levels[4] > levels[9]);
    }

    #[test]
    fn bass_tone_stays_in_low_bands() {
        let mut levels = vec![0.0; NUM_BANDS];
        let samples = sine(70.0, WINDOW, SAMPLE_RATE, 0.6);
        for _ in 0..8 {
            levels = analyze(&samples, Some(&levels), SAMPLE_RATE);
        }
        assert!(levels[0] > levels[8]);
        assert!(levels[0] > 0.05);
    }
}
