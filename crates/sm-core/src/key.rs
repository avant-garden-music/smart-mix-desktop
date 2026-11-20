//! Detector de tonalidad — port fiel de `harmonic-engine/engine/ref_clone.py`
//! (spec `ref_detector_spec.md` v94, algoritmo 0x5E, constantes [VERIF]).
//!
//! FFT 4096 rectangular, hop 2048, 44100 Hz; picos sinusoidales refinados por
//! fase; filtro anti-lóbulo 0.96; estabilización 3 bloques; espectros A/B
//! max-hold; sharpening con A-weighting; acumulado 88 teclas y
//! StaticProcessFeatures ((f/27.5)^0.55·x)^0.62.

use rayon::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};
use std::cell::RefCell;
use std::sync::LazyLock;

pub const SR: u32 = 44_100;
pub const FFT: usize = 4096;
pub const HOP: usize = 2048;
pub const NBINS: usize = 512;
pub const BW: f64 = SR as f64 / FFT as f64; // 10.7666 Hz
const A_CO: f64 = 1.07;
const B_CO: f64 = -0.68;
const P_CO: f64 = 0.41;
const Q_CO: f64 = -0.85;
const T_CLAMP: f64 = 1.254_404;
pub const GAMMA: f64 = 1.0;

const EQ_EDGES: [f64; 25] = [
    0.0, 100.0, 200.0, 300.0, 400.0, 510.0, 630.0, 770.0, 920.0, 1080.0, 1270.0, 1480.0,
    1720.0, 2000.0, 2320.0, 2700.0, 3150.0, 3700.0, 4400.0, 5300.0, 6400.0, 7700.0,
    9500.0, 12000.0, 15500.0,
];

fn a_weight(f: f64) -> f64 {
    let f2 = f * f;
    (f.powi(4) * 148_840_000.0)
        / ((f2 + 148_840_000.0)
            * (f2 + 424.36)
            * ((f2 + 11_599.29) * (f2 + 544_496.409_999_999_9)).sqrt())
}

static WIDTHS: LazyLock<Vec<f64>> = LazyLock::new(|| {
    let n = NBINS + 64;
    let mut wt = vec![0.0; n];
    let mut covered = vec![false; n];
    for w in EQ_EDGES.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let b0 = (lo / BW).round() as usize;
        let b1 = ((hi / BW).round() as usize).min(FFT);
        let mut width = (hi - lo) / BW;
        if lo > 440.0 {
            width /= 1.4;
        }
        for b in b0..b1.min(n) {
            wt[b] = width;
            covered[b] = true;
        }
    }
    for b in 0..n {
        if !covered[b] {
            wt[b] = (15_500.0 - 12_000.0) / BW / 1.4;
        }
    }
    wt
});

static AW_BIN: LazyLock<Vec<f64>> =
    LazyLock::new(|| (0..NBINS + 64).map(|i| a_weight(i as f64 * BW)).collect());

/// Tecla de piano fraccional (A0=0, A4=48, C8=87).
#[inline]
fn piano_key(f: f64) -> f64 {
    ((f / 440.0).log2() + 4.0) * 12.0
}

// ---------------------------------------------------------------- picos ----

type Spectrum = Vec<Complex<f64>>;

fn extract_peaks(x: &Spectrum, mag: &mut [f64], tol_factor: f64, guard07: bool) -> Vec<(usize, f64)> {
    let mut peaks = Vec::new();
    let mut i = 1usize;
    while i <= NBINS - 2 {
        let (m0, m1, m2) = (mag[i - 1], mag[i], mag[i + 1]);
        if m0 <= m1 && m2 < m1 && m0 > 0.0 && m2 > 0.0 {
            let c0 = x[i];
            let d = 0.45 * (c0.im * (x[i - 1].im - x[i + 1].im)
                + c0.re * (x[i - 1].re - x[i + 1].re))
                / (m1 * m1);
            if !guard07 || d.abs() < 0.7 {
                let c = B_CO * d.powi(3) + A_CO * d;
                let d_l = if i == 1 {
                    c + 1.0
                } else {
                    let cl = x[i - 1];
                    let dl = 0.45 * (cl.im * (x[i - 2].im - x[i].im)
                        + cl.re * (x[i - 2].re - x[i].re))
                        / (m0 * m0);
                    dl.clamp(-T_CLAMP, T_CLAMP)
                };
                let d_l = d_l.clamp(-T_CLAMP, T_CLAMP);
                let amp = m1 * 2.0_f64.powf((4.0 * P_CO * d + Q_CO * d.powi(3)).powi(2) as f64);
                let t1 = tol_factor * amp / (amp * 0.001).max(m0);
                let mut ok =
                    (B_CO * d_l.powi(3) + A_CO * d_l + 1.0 - c).abs() < t1;
                if !ok {
                    let cr = x[i + 1];
                    let dr = 0.45 * (cr.im * (x[i].im - x[i + 2].im)
                        + cr.re * (x[i].re - x[i + 2].re))
                        / (m2 * m2);
                    let dr = dr.clamp(-T_CLAMP, T_CLAMP);
                    let t2 = tol_factor * amp / (amp * 0.001).max(m2);
                    ok = (B_CO * dr.powi(3) + A_CO * dr - 1.0 - c).abs() < t2;
                }
                if ok {
                    mag[i] = amp;
                    peaks.push((i, c));
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    peaks
}

fn dominance_filter(peaks: Vec<(usize, f64)>, mag: &[f64]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (b, c) in peaks {
        let hw = (0.4 * WIDTHS[b] - 1.0) as i64;
        if hw <= 0 {
            out.push((b as f64 + c, mag[b]));
            continue;
        }
        let lo = (b as i64 - hw).max(0) as usize;
        let hi = ((b as i64 + hw + 1) as usize).min(NBINS);
        let k = b - lo;
        let seg = &mag[lo..hi];
        let center = seg[k];
        if seg.iter().enumerate().any(|(j, &v)| j != k && v > 0.96 * center) {
            continue;
        }
        let pos = b as f64 + c;
        if pos >= 1.0 {
            out.push((pos, mag[b]));
        }
    }
    out
}

// ------------------------------------------------------- estabilización ----

fn stabilize(hist: &[Vec<(f64, f64)>]) -> Vec<(f64, f64)> {
    let mut stable = Vec::new();
    if hist.len() < 3 {
        return stable;
    }
    let t2 = &hist[hist.len() - 3];
    let t1 = &hist[hist.len() - 2];
    let t0 = &hist[hist.len() - 1];
    let mut tr = [0usize, 0usize];
    for &(fb, ab) in t1 {
        let mut matches: [(f64, f64); 2] = [(0.0, 0.0); 2];
        for (j, prev) in [t2, t0].iter().enumerate() {
            if tr[j] >= prev.len() {
                matches[j] = (-1e9, 0.0);
                continue;
            }
            let mut k = tr[j];
            let mut best = k;
            while k + 1 < prev.len()
                && (prev[k + 1].0 - fb).abs() < (prev[best].0 - fb).abs()
            {
                k += 1;
                best = k;
            }
            tr[j] = best;
            matches[j] = prev[best];
        }
        let k0 = piano_key(fb * BW);
        let d_old = if matches[0].0 > 0.0 {
            (piano_key(matches[0].0 * BW) - k0).abs()
        } else {
            f64::INFINITY
        };
        let d_new = if matches[1].0 > 0.0 {
            (piano_key(matches[1].0 * BW) - k0).abs()
        } else {
            f64::INFINITY
        };
        if d_old.min(d_new) < 4.0 {
            let (mut f, mut a) = (fb, ab);
            if d_old < 0.4 && d_new < 0.4 {
                f = (matches[0].0 + matches[1].0) * 0.3 + f * 0.4;
                a = (matches[0].1 + matches[1].1) * 0.3 + a * 0.4;
            }
            stable.push((f, a));
        }
    }
    stable
}

// ------------------------------------------------------- espectros A/B ----

fn key_spectra(stable: &[(f64, f64)]) -> [f64; 88] {
    let mut a = [0.0f64; 93];
    let mut b = [0.0f64; 91];
    for &(fbin, amp) in stable {
        let k = piano_key(fbin * BW);
        let p = amp * amp;
        let k1 = (k + 0.5) as i64; // trunc hacia cero
        if (0..89).contains(&k1) {
            let x = (k - k1 as f64) / 0.8 + 0.5;
            if (0.0..1.0).contains(&x) {
                let g = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * x).cos();
                let idx = (k1 + 2) as usize;
                a[idx] = a[idx].max(p * g);
            }
        }
        let k2 = k as i64;
        if (0..89).contains(&k2) {
            let x = (k - 0.5 - k2 as f64) / 0.8 + 0.5;
            if (0.0..1.0).contains(&x) {
                let g = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * x).cos();
                let idx = k2 as usize;
                b[idx] = b[idx].max(p * g);
            }
        }
    }
    for k in 0..89usize {
        let fk = 440.0 * 2f64.powf(k as f64 / 12.0 - 4.0);
        let tk = AW_BIN[(fk / BW + 0.5) as usize];
        a[k + 2] = ((a[k + 2] * tk.powf(3.003_002_9)).powf(0.333)).sqrt() * 6e6;
        let fkh = 440.0 * 2f64.powf((k as f64 + 0.5) / 12.0 - 4.0);
        let tkh = AW_BIN[(fkh / BW + 0.5) as usize];
        b[k] = ((b[k] * tkh.powf(3.003_002_9)).powf(0.333)).sqrt() * 6e6;
    }
    let mut out = [0.0f64; 88];
    for kk in 2..88usize {
        let peak = a[kk + 2];
        let floor =
            b[kk - 2] + a[kk - 1 + 2] + b[kk - 1] + b[kk] + a[kk + 1 + 2] + b[kk + 1];
        let mut v = if floor * 0.4 <= peak { peak } else { 0.0 };
        if v != 0.0 {
            v /= (440.0 * 2f64.powf(kk as f64 / 12.0 - 4.0)) / BW;
        }
        out[kk] = v;
    }
    out
}

/// StaticProcessFeatures (spec §4.9): ((f/27.5)^0.55·x)^0.62, teclas 0-4 y 87
/// → 0, 5-6 ×0.5, Σ = 0.9.
pub fn static_process_features(acc: &[f64; 88]) -> [f64; 88] {
    let mut out = [0.0f64; 88];
    for i in 0..88 {
        let mut v = (2f64.powf(i as f64 * 0.045_833_333_333_333_34) * acc[i]).powf(0.62);
        if i <= 4 || i == 87 {
            v = 0.0;
        }
        if i == 5 || i == 6 {
            v *= 0.5;
        }
        out[i] = v;
    }
    let s: f64 = out.iter().sum();
    if s == 0.0 {
        return [0.9 / 88.0; 88];
    }
    for o in out.iter_mut() {
        *o *= 0.9 / s;
    }
    out
}

// ------------------------------------------------------------ pipeline ----

pub struct KeyFrames {
    pub frames: Vec<[f64; 88]>,
    pub n_frames: usize,
}

thread_local! {
    static PLANNER: RefCell<FftPlanner<f64>> = RefCell::new(FftPlanner::new());
}

fn frame_peaks(x: &[f32], start: usize, fft: &dyn rustfft::Fft<f64>) -> Vec<(f64, f64)> {
    let mut buf = vec![Complex { re: 0.0, im: 0.0 }; FFT];
    for (j, s) in x[start..start + FFT].iter().enumerate() {
        buf[j].re = *s as f64;
    }
    fft.process(&mut buf);
    let spec: Spectrum = buf[..NBINS + 2].to_vec();
    let mut mag: Vec<f64> = spec.iter().map(|c| (c.re * c.re + c.im * c.im).sqrt()).collect();
    let peaks = extract_peaks(&spec, &mut mag, 0.4, true);
    dominance_filter(peaks, &mag)
}

/// Analiza la pista completa mono y devuelve los 88 features por frame.
pub fn analyze_frames(x_mono: &[f32]) -> KeyFrames {
    let n_frames = if x_mono.len() < FFT { 0 } else { (x_mono.len() - FFT) / HOP + 1 };
    if n_frames <= 2 {
        return KeyFrames { frames: Vec::new(), n_frames };
    }
    // Fase 1 paralela: FFT + picos + anti-lóbulo por frame.
    let peaks_per_frame: Vec<Vec<(f64, f64)>> = (0..n_frames)
        .into_par_iter()
        .map(|n| {
            PLANNER.with(|p| {
                let fft = p.borrow_mut().plan_fft_forward(FFT);
                frame_peaks(x_mono, n * HOP, fft.as_ref())
            })
        })
        .collect();
    // Fase 2 secuencial: estabilización + espectros + acumulado.
    let mut frames = Vec::with_capacity(n_frames);
    let mut hist: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut acc = [0.0f64; 88];
    for pf in peaks_per_frame {
        hist.push(pf);
        if hist.len() > 4 {
            hist.remove(0);
        }
        let stable = stabilize(&hist);
        let frame = if stable.is_empty() { [0.0; 88] } else { key_spectra(&stable) };
        for k in 0..88 {
            acc[k] += frame[k].powf(GAMMA);
        }
        frames.push(frame);
    }
    KeyFrames { frames, n_frames }
}

fn median3(a: f64, b: f64, c: f64) -> f64 {
    let mut v = [a, b, c];
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    v[1]
}

fn idx_frame(t: f64) -> f64 {
    (SR as f64 * t - HOP as f64) / HOP as f64 - 1.0
}

/// Agregador temporal §10.4: mediana de 3 frames ponderada por el rango.
/// Devuelve (features A, acumulado B).
pub fn results_for_time_range(frames: &[[f64; 88]], start: f64, end: f64) -> Option<(Vec<f64>, Vec<f64>)> {
    let n = frames.len();
    let (mut lo, mut hi) = (idx_frame(start), idx_frame(end));
    let width;
    if hi - lo < 1.0 {
        let mid = (lo + hi) * 0.5;
        lo = mid - 0.5;
        hi = mid + 0.5;
        width = 1.0;
    } else {
        width = hi - lo;
    }
    let (lo_i, hi_i) = (lo as i64, hi as i64);
    if hi_i < lo_i {
        return None;
    }
    let f0 = if lo_i >= 1 { lo_i as usize } else { 0 };
    let f1 = ((hi_i + 1) as usize).min(n);
    if f0 >= f1 {
        return None;
    }
    let mut b_acc = [0.0f64; 88];
    for f in f0..f1 {
        let w = if f as i64 == hi_i {
            hi - hi_i as f64
        } else if f as i64 == lo_i && lo_i >= 0 {
            (lo_i as f64 - lo) + 1.0
        } else {
            1.0
        };
        let prev: &[f64; 88] = if f > 0 { &frames[f - 1] } else { &[0.0; 88] };
        let cur = &frames[f];
        let nxt: &[f64; 88] = if f + 1 < n { &frames[f + 1] } else { &[0.0; 88] };
        for k in 0..88 {
            b_acc[k] += median3(prev[k], cur[k], nxt[k]) * w / width;
        }
    }
    let a = static_process_features(&b_acc);
    Some((a.to_vec(), b_acc.to_vec()))
}

// ------------------------------------------------- features del modelo ----

pub fn chroma12(x: &[f64; 88]) -> [f64; 12] {
    let mut c = [0.0f64; 12];
    for (i, &v) in x.iter().enumerate() {
        c[(i + 9) % 12] += v;
    }
    c
}

/// 112 features: [chroma/max (12), log1p(chroma/Σ) (12), x/|x| (88)].
pub fn key_feats(a: &[f64; 88]) -> Vec<f64> {
    let c = chroma12(a);
    let m = if c.iter().all(|&v| v == 0.0) { 1.0 } else { c.iter().cloned().fold(0.0, f64::max) };
    let sum: f64 = c.iter().sum();
    let norm: f64 = a.iter().map(|v| v * v).sum::<f64>().sqrt();
    let norm = if norm == 0.0 { 1.0 } else { norm };
    let mut out = Vec::with_capacity(112);
    out.extend(c.iter().map(|&v| v / m));
    out.extend(c.iter().map(|&v| (v / sum).ln_1p()));
    out.extend(a.iter().map(|&v| v / norm));
    out
}

// ------------------------------------- clasificador Krumhansl (fallback) ----

const KK_MAJ: [f64; 12] =
    [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const KK_MIN: [f64; 12] =
    [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
const PITCH_NAMES: [&str; 12] =
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Fallback sin modelo: Krumhansl-Schmuckler sobre la chroma de 88 teclas.
/// Devuelve (camelot, "Tonic(m)", score).
pub fn classify_fallback(a: &[f64; 88]) -> (String, String, f64) {
    let c = chroma12(a);
    let m = if c.iter().all(|&v| v == 0.0) { 1.0 } else { c.iter().cloned().fold(0.0, f64::max) };
    let mut best = (-1.0f64, 0usize, false);
    for root in 0..12usize {
        for (minor, prof) in [(false, &KK_MAJ), (true, &KK_MIN)] {
            let mut s = 0.0;
            for j in 0..12 {
                s += c[(root + j) % 12] / m * prof[j];
            }
            if s > best.0 {
                best = (s, root, minor);
            }
        }
    }
    let tonic = PITCH_NAMES[best.1];
    let num = {
        let n = (8 + 7 * (best.1 as i32 + if best.2 { 3 } else { 0 })) % 12;
        if n == 0 { 12 } else { n }
    };
    (
        format!("{}{}", num, if best.2 { 'A' } else { 'B' }),
        format!("{}{}", tonic, if best.2 { "m" } else { "" }),
        best.0,
    )
}

/// Análisis completo de tonalidad sobre PCM mono 44100.
/// Devuelve los features agregados A (88) y las feats del modelo (112).
pub struct KeyResult {
    pub features88: Vec<f64>,
    pub feats112: Vec<f64>,
}

pub fn detect_key_frames(x_mono: &[f32]) -> Option<KeyResult> {
    let kf = analyze_frames(x_mono);
    if kf.frames.is_empty() {
        return None;
    }
    let end = kf.n_frames as f64 * HOP as f64 / SR as f64;
    let (a, _b) = results_for_time_range(&kf.frames, 0.0, end)?;
    let mut a88 = [0.0f64; 88];
    a88.copy_from_slice(&a);
    Some(KeyResult { feats112: key_feats(&a88), features88: a })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_chord(freqs: &[f64], secs: f64) -> Vec<f32> {
        let n = (secs * SR as f64) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / SR as f64;
                (freqs.iter().map(|&f| (2.0 * std::f64::consts::PI * f * t).sin()).sum::<f64>()
                    / freqs.len() as f64) as f32
            })
            .collect()
    }

    #[test]
    fn c_major_detected() {
        // C4 E4 G4 + bajo C3 (armónicos naturales vía senos)
        let x = synth_chord(&[130.81, 261.63, 329.63, 392.0, 523.25], 60.0);
        let kf = analyze_frames(&x);
        assert!(kf.frames.len() > 1000, "frames {}", kf.frames.len());
        let end = kf.n_frames as f64 * HOP as f64 / SR as f64;
        let (a, _) = results_for_time_range(&kf.frames, 0.0, end).unwrap();
        let mut a88 = [0.0f64; 88];
        a88.copy_from_slice(&a);
        let (camelot, name, _) = classify_fallback(&a88);
        assert_eq!(camelot, "8B", "esperaba C major, salió {camelot} ({name})");
        assert_eq!(name, "C");
    }

    #[test]
    fn a_minor_detected() {
        let x = synth_chord(&[220.0, 261.63, 329.63, 440.0, 110.0], 60.0);
        let kf = analyze_frames(&x);
        let end = kf.n_frames as f64 * HOP as f64 / SR as f64;
        let (a, _) = results_for_time_range(&kf.frames, 0.0, end).unwrap();
        let mut a88 = [0.0f64; 88];
        a88.copy_from_slice(&a);
        let (camelot, name, _) = classify_fallback(&a88);
        assert_eq!(camelot, "8A", "esperaba A minor, salió {camelot} ({name})");
    }

    #[test]
    fn feats_dimension() {
        let x = synth_chord(&[261.63, 329.63, 392.0], 30.0);
        let kf = analyze_frames(&x);
        let end = kf.n_frames as f64 * HOP as f64 / SR as f64;
        let (a, _) = results_for_time_range(&kf.frames, 0.0, end).unwrap();
        let mut a88 = [0.0f64; 88];
        a88.copy_from_slice(&a);
        assert_eq!(key_feats(&a88).len(), 112);
        let sum: f64 = a.iter().sum();
        assert!((sum - 0.9).abs() < 1e-6, "Σ features = {sum}");
    }
}
