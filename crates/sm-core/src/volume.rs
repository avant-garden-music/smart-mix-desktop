//! Volumen/LUFS/waveform — port de `harmonic-engine/engine/mik_volume.py`.
//!
//! [VERIF] histograma |x| 1000 bins con CALIB_GAIN ×1.2388 (+1.801 dB),
//! clipping crudo ≥ 0.997 sobre TODOS los canales; LUFS BS.1770-4 con
//! biquads time-domain (la vía FFT quedó deprecada en el prototipo).
//! Waveform: 6 floats/1024 muestras (layout propio del prototipo).

pub const BINS: usize = 1000;
pub const CLIP: f64 = 0.997;

pub fn calib_gain() -> f64 {
    10f64.powf(1.801 / 20.0)
}

/// Histograma de |x·CALIB| en 1000 bins sobre el stream interleaved completo.
pub fn volume_histogram(interleaved: &[f32]) -> Vec<u64> {
    let g = calib_gain();
    let mut hist = vec![0u64; BINS];
    for &s in interleaved {
        let v = ((s as f64) * g).clamp(-1.0, 1.0).abs();
        let idx = ((v * BINS as f64) as usize).min(BINS - 1);
        hist[idx] += 1;
    }
    hist
}

fn range_volume_db(hist: &[u64; BINS], lo: usize, hi: usize) -> f64 {
    let s: u64 = hist[lo..=hi].iter().sum();
    if s == 0 {
        return 1.0;
    }
    let num: f64 = hist[lo..=hi]
        .iter()
        .enumerate()
        .map(|(i, &h)| h as f64 * (((i + lo) as f64 + 0.5) / BINS as f64).powi(2))
        .sum();
    20.0 * (num / s as f64).sqrt().log10()
}

pub fn volume_db(hist: &[u64; BINS]) -> f64 {
    range_volume_db(hist, 0, BINS - 1)
}

pub fn clipped_peak_count(interleaved: &[f32]) -> u64 {
    interleaved.iter().filter(|&&s| (s as f64).abs() >= CLIP).count() as u64
}

// ------------------------------------------------ LUFS BS.1770-4 ----------

#[derive(Clone)]
struct Biquad {
    b0: f64, b1: f64, b2: f64,
    a1: f64, a2: f64,
    x1: f64, x2: f64, y1: f64, y2: f64,
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 3]) -> Self {
        Biquad { b0: b[0], b1: b[1], b2: b[2], a1: a[1], a2: a[2], x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

fn k_filters(sr: f64) -> (Biquad, Biquad) {
    // Stage 1: high-shelf +4 dB
    let f0 = 1681.974_450_955_533;
    let g = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (std::f64::consts::PI * f0 / sr).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let b = [
        (vh + vb * k / q + k * k) / a0,
        2.0 * (k * k - vh) / a0,
        (vh - vb * k / q + k * k) / a0,
    ];
    let a = [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0];
    // Stage 2: RLB high-pass
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (std::f64::consts::PI * f0 / sr).tan();
    let d = 1.0 + k / q + k * k;
    let b2 = [1.0 / d, -2.0 / d, 1.0 / d];
    let a2 = [1.0, 2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d];
    (Biquad::new(b, a), Biquad::new(b2, a2))
}

/// LUFS integrada BS.1770-4 (hasta 2 canales, como el prototipo validado).
pub fn lufs_integrated(interleaved: &[f32], channels: usize, sr: u32) -> f64 {
    let nch = channels.min(2).max(1);
    let frames = interleaved.len() / channels.max(1);
    let mut chans: Vec<Vec<f64>> = vec![Vec::with_capacity(frames); nch];
    for c in 0..nch {
        for i in 0..frames {
            chans[c].push(interleaved[i * channels + c] as f64);
        }
    }
    let (s1, s2) = k_filters(sr as f64);
    let filtered: Vec<Vec<f64>> = chans
        .iter()
        .map(|c| {
            let (mut a, mut b) = (s1.clone(), s2.clone());
            c.iter().map(|&x| b.process(a.process(x))).collect()
        })
        .collect();
    let weights = [1.0f64, 1.0f64];
    let blk = (0.4 * sr as f64) as usize;
    let hop = (0.1 * sr as f64) as usize;
    let mut powers: Vec<f64> = Vec::new();
    let n = frames;
    if n < blk {
        return -70.0;
    }
    let mut s = 0usize;
    while s + blk <= n {
        let mut p = 0.0;
        for (c, ch) in filtered.iter().enumerate() {
            p += weights[c] * ch[s..s + blk].iter().map(|v| v * v).sum::<f64>() / blk as f64;
        }
        powers.push(p);
        s += hop;
    }
    let sel: Vec<f64> = powers.iter().copied().filter(|&p| p > 10f64.powf(-70.0 / 10.0)).collect();
    if sel.is_empty() {
        return -70.0;
    }
    let mean = sel.iter().sum::<f64>() / sel.len() as f64;
    let gamma = 10f64.powf((-0.691 + 10.0 * mean.log10()) / 10.0);
    let sel2: Vec<f64> = sel.iter().copied().filter(|&p| p > gamma).collect();
    let pool = if sel2.is_empty() { &sel } else { &sel2 };
    let m = pool.iter().sum::<f64>() / pool.len() as f64;
    -0.691 + 10.0 * m.log10()
}

// ------------------------------------------------------------- waveform ----

/// Registro por bloque de 1024 muestras interleaved:
/// {meanL, peakL, meanR, peakR, rmsL, rmsR}.
pub fn waveform_records(interleaved: &[f32], channels: usize, block: usize) -> Vec<[f32; 6]> {
    let n = interleaved.len() / block * block;
    let mut recs = Vec::with_capacity(n / block);
    for s in (0..n).step_by(block) {
        let blk = &interleaved[s..s + block];
        let (l, r): (Vec<f32>, Vec<f32>) = if channels >= 2 {
            (blk.iter().step_by(2).copied().collect(), blk.iter().skip(1).step_by(2).copied().collect())
        } else {
            (blk.to_vec(), blk.to_vec())
        };
        let mean = |v: &[f32]| v.iter().map(|x| x.abs()).sum::<f32>() / v.len() as f32;
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        recs.push([
            mean(&l), peak(&l), mean(&r), peak(&r), rms(&l), rms(&r),
        ]);
    }
    recs
}

/// Waveform de visualización: N buckets × (min, max, rms) sobre el canal L.
pub fn display_waveform(recs: &[[f32; 6]], target_buckets: usize) -> Vec<f32> {
    if recs.is_empty() {
        return Vec::new();
    }
    let n = target_buckets.min(recs.len());
    let mut out = Vec::with_capacity(n * 3);
    let per = recs.len() as f64 / n as f64;
    for i in 0..n {
        let a = (i as f64 * per).floor() as usize;
        let b = (((i as f64 + 1.0) * per).ceil() as usize).clamp(a + 1, recs.len());
        let mut peak = 0.0f32;
        let mut rms = 0.0f32;
        for r in &recs[a..b] {
            peak = peak.max(r[1]);
            rms += r[4];
        }
        rms /= (b - a) as f32;
        out.push(-peak);
        out.push(peak);
        out.push(rms);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lufs_sine_minus6() {
        // senoide −6 dBFS estéreo → ≈ −6.06 LU (validación del prototipo)
        let sr = 44_100u32;
        let secs = 20.0;
        let amp = 10f64.powf(-6.0 / 20.0);
        let mut inter = Vec::new();
        for i in 0..(secs as usize * sr as usize) {
            let v = (2.0 * std::f64::consts::PI * 997.0 * i as f64 / sr as f64).sin() * amp;
            inter.push(v as f32);
            inter.push(v as f32);
        }
        let lufs = lufs_integrated(&inter, 2, sr);
        assert!((lufs - (-6.06)).abs() < 0.2, "LUFS = {lufs}");
    }

    #[test]
    fn volume_histogram_tone() {
        // seno puro a 0.5 de amplitud: RMS = 0.5/√2 · calib → dB esperado
        let sr = 44_100u32;
        let mut inter = Vec::new();
        for i in 0..sr as usize {
            inter.push((2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin() as f32 * 0.5);
        }
        let hist_v = volume_histogram(&inter);
        let mut hist = [0u64; BINS];
        hist.copy_from_slice(&hist_v);
        let db = volume_db(&hist);
        let expected = 20.0 * (0.5 / 2f64.sqrt() * calib_gain()).log10();
        assert!((db - expected).abs() < 0.35, "vol {db} vs esperado {expected}");
    }

    #[test]
    fn clipping_counted() {
        let inter = vec![0.999f32; 100];
        assert_eq!(clipped_peak_count(&inter), 100);
        let inter2 = vec![0.5f32; 100];
        assert_eq!(clipped_peak_count(&inter2), 0);
    }
}
