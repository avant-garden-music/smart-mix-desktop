//! Energía — port de `harmonic-engine/engine/ref_energy.py`
//! (spec ref_energy_spec.md; MAE 0.479 global, ±1 = 98.2% vs herramienta de referencia).
//!
//! FFT 2048/hop 682 @44100 mono, A-weighting ×0.7071, 14 bandas 0–13 kHz,
//! P40 deslizante ±7 frames, normalización 1/P95(banda 0), tempo local por
//! comb de ACF con prior log-normal @128 BPM, segmentos de 32 beats
//! re-anclados a rejilla de 16, CountBeats √(a·b) ≥ ref·q.

use rayon::prelude::*;
#[cfg(test)]
use crate::beats;
use rustfft::{num_complex::Complex, FftPlanner};
use std::cell::RefCell;
use std::sync::LazyLock;

pub const FS: u32 = 44_100;
pub const N: usize = 2048;
pub const HOP: usize = 682;
pub const FPS: f64 = FS as f64 / HOP as f64; // 64.66 frames/s
pub const NBANDS: usize = 14;

/// (start, end) de bins por banda — volcados de spec §3.3.
const BAND_BINS: [(usize, usize); NBANDS] = [
    (0, 3), (3, 5), (5, 8), (8, 11), (11, 17), (17, 23), (23, 37), (37, 55),
    (55, 83), (83, 116), (116, 171), (171, 232), (232, 325), (325, 604),
];
const BAND_EDGES: [f64; 15] =
    [0.0, 75.0, 125.0, 180.0, 250.0, 375.0, 500.0, 800.0, 1200.0, 1800.0, 2500.0, 3700.0, 5000.0, 7000.0, 13000.0];
const BAND_SCALES_A: [f64; NBANDS] = [
    3.31e-4, 2.42e-4, 4.59e-4, 3.09e-4, 3.22e-4, 4.64e-4, 4.41e-4, 4.78e-4,
    3.77e-4, 3.41e-4, 2.79e-4, 1.95e-4, 3.21e-4, 2.45e-4,
];
const D_GLOBAL: f64 = 0.01332;
const SAL_WIN: usize = 32;
const BPM_MIN_SWEEP: f64 = 71.0;
const BPM_MAX_SWEEP: f64 = 180.0;

fn a_weight(f: f64) -> f64 {
    let f2 = f * f;
    let num = f2 * f2 * 1.4884e8;
    let den = (f2 + 1.4884e8) * (f2 + 424.36) * ((f2 + 11599.29) * (f2 + 544496.41)).sqrt();
    if f > 0.0 {
        (num / den.max(1e-30)) * std::f64::consts::FRAC_1_SQRT_2
    } else {
        0.0
    }
}

static AW_TABLE: LazyLock<Vec<f64>> = LazyLock::new(|| {
    (0..=N / 2).map(|i| a_weight(i as f64 * FS as f64 / N as f64)).collect()
});
static HANN: LazyLock<Vec<f64>> = LazyLock::new(|| {
    (0..N)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N as f64).cos())
        .collect()
});

thread_local! {
    static PLANNER: RefCell<FftPlanner<f64>> = RefCell::new(FftPlanner::new());
}

// ------------------------------------------------------------- cuantiles ----

pub fn quantile_sorted(sorted: &[f64], q: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted[0];
    }
    let pos = q * (n as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

// ------------------------------------------------ front-end espectral [VERIF] ----

/// Devuelve (B[n,14] energies, rms[n], n_frames).
pub fn band_energies(x: &[f32]) -> (Vec<Vec<f64>>, Vec<f64>, usize) {
    let n_frames = if x.len() < N { 0 } else { (x.len() - N) / HOP + 1 };
    assert!(n_frames >= 8, "audio insuficiente");
    // RMS por frame vía prefix sums
    let mut prefix = vec![0.0f64; x.len() + 1];
    for i in 0..x.len() {
        prefix[i + 1] = prefix[i] + (x[i] as f64) * (x[i] as f64);
    }
    let rms: Vec<f64> = (0..n_frames)
        .map(|f| ((prefix[f * HOP + N] - prefix[f * HOP]) / N as f64).sqrt())
        .collect();
    let b: Vec<Vec<f64>> = (0..n_frames)
        .into_par_iter()
        .map(|f| {
            PLANNER.with(|p| {
                let fft = p.borrow_mut().plan_fft_forward(N);
                let mut buf = vec![Complex::<f64> { re: 0.0, im: 0.0 }; N];
                for j in 0..N {
                    buf[j].re = x[f * HOP + j] as f64 * HANN[j];
                }
                fft.process(&mut buf);
                let power: Vec<f64> = (0..=N / 2)
                    .map(|k| (buf[k].re * buf[k].re + buf[k].im * buf[k].im) * AW_TABLE[k])
                    .collect();
                BAND_BINS
                    .iter()
                    .map(|&(lo, hi)| power[lo..hi].iter().sum())
                    .collect()
            })
        })
        .collect();
    (b, rms, n_frames)
}

/// NormalizeBands [VERIF §4.1]: gain = 1/P95(banda 0).
pub fn normalize_bands(b: &[Vec<f64>]) -> (Vec<Vec<f64>>, f64) {
    let mut col: Vec<f64> = b.iter().map(|r| r[0]).collect();
    col.sort_by(|a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
    let p95 = quantile_sorted(&col, 0.95).max(1e-4);
    let norm: Vec<Vec<f64>> = b.iter().map(|r| r.iter().map(|v| v / p95).collect()).collect();
    (norm, p95)
}

/// P40 deslizante ±7 frames por banda [VERIF §4.2].
pub fn robust_levels(bn: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = bn.len();
    let mut out = vec![vec![0.0f64; NBANDS]; n];
    for band in 0..NBANDS {
        for i in 0..n {
            let a = i.saturating_sub(7);
            let b_ = (i + 7 + 1).min(n);
            let mut w: Vec<f64> = bn[a..b_].iter().map(|r| r[band]).collect();
            w.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
            let pos: f64 = 0.4 * 14.0;
            let lo = pos.floor() as usize;
            out[i][band] = w[lo] + (w[lo + 1] - w[lo]) * (pos - lo as f64);
        }
    }
    out
}

/// Store diferencial (ReLU del diff) — alimenta saliencia/ACF.
pub fn diff_store(l: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = l.len();
    let mut d = vec![vec![0.0f64; NBANDS]; n];
    for i in 0..n {
        for b in 0..NBANDS {
            d[i][b] = if i == 0 {
                0.0
            } else {
                (l[i][b] - l[i - 1][b]).max(0.0)
            };
        }
    }
    d
}

/// Saliencia [VERIF §4.3]: flag(≥1.2·P40 local ±32) dilatado ±2 · max(0, v−P90).
pub fn salience_band(v: &[f64]) -> Vec<f64> {
    let n = v.len();
    if n == 0 {
        return Vec::new();
    }
    let mut p40 = vec![0.0f64; n];
    let mut p90 = vec![0.0f64; n];
    for i in 0..n {
        let a = i.saturating_sub(SAL_WIN);
        let b = (i + SAL_WIN + 1).min(n);
        let mut w: Vec<f64> = v[a..b].to_vec();
        w.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        p40[i] = quantile_sorted(&w, 0.4);
        p90[i] = quantile_sorted(&w, 0.9);
    }
    let mut flag = vec![0.0f64; n];
    for i in 0..n {
        if v[i] >= 1.2 * p40[i] {
            flag[i] = 1.0;
        }
    }
    // dilatación ±2
    let mut fm = flag.clone();
    for k in 1..=2 {
        for i in k..n {
            fm[i] = fm[i].max(flag[i - k]);
        }
        for i in 0..n - k {
            fm[i] = fm[i].max(flag[i + k]);
        }
    }
    (0..n).map(|i| fm[i] * (v[i] - p90[i]).max(0.0)).collect()
}

// -------------------------------------------------- tempo por comb [VERIF §5.1] ----

/// ACF por banda (FFT insesgada) sumada sobre las 14 bandas. lag en frames.
pub fn band_acfs(d: &[Vec<f64>]) -> Vec<f64> {
    let n = d.len();
    if n == 0 {
        return Vec::new();
    }
    let size = (2 * n).max(16).next_power_of_two();
    let mut out = vec![0.0f64; n];
    PLANNER.with(|p| {
        let fft = p.borrow_mut().plan_fft_forward(size);
        let ifft = p.borrow_mut().plan_fft_inverse(size);
        let mut buf = vec![Complex::<f64> { re: 0.0, im: 0.0 }; size];
        let mut power = vec![Complex::<f64> { re: 0.0, im: 0.0 }; size];
        for band in 0..NBANDS {
            let mean: f64 = d.iter().map(|r| r[band]).sum::<f64>() / n as f64;
            for c in buf.iter_mut() {
                *c = Complex { re: 0.0, im: 0.0 };
            }
            for (i, r) in d.iter().enumerate() {
                buf[i].re = r[band] - mean;
            }
            fft.process(&mut buf);
            for i in 0..size {
                power[i].re = buf[i].re * buf[i].re + buf[i].im * buf[i].im;
                power[i].im = 0.0;
            }
            ifft.process(&mut power);
            for (i, o) in out.iter_mut().enumerate() {
                *o += power[i].re / size as f64 / (n - i).max(1) as f64;
            }
        }
    });
    out[0] = 0.0;
    out
}

fn interp_val(x: &[f64], pos: f64) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    if pos <= 0.0 {
        return x[0];
    }
    if pos >= x.len() as f64 - 1.0 {
        return *x.last().unwrap();
    }
    let i = pos.floor() as usize;
    let f = pos - i as f64;
    x[i] * (1.0 - f) + x[i + 1] * f
}

/// Comb duple-meter + prior log-normal @128. Devuelve (bpm, lag_frames, score).
pub fn comb_tempo(acf: &[f64], n_frames: usize) -> (f64, f64, f64) {
    let p_min = 60.0 / BPM_MAX_SWEEP;
    let p_max = 60.0 / BPM_MIN_SWEEP;
    let score_period = |period: f64| -> f64 {
        let lag = period * FPS;
        let kmax = ((n_frames as f64 - 1.0) / lag) as i64;
        if kmax < 2 {
            return f64::NEG_INFINITY;
        }
        let mut v = 0.0;
        for k in 1..=kmax {
            let w = 1.0
                + if k % 2 == 0 { 1.4 } else { 0.0 }
                + if k % 4 == 0 { 2.5 } else { 0.0 }
                - if k % 8 == 0 { 3.5 } else { 0.0 }
                - if k % 3 == 0 { 1.3 } else { 0.0 };
            v += interp_val(acf, k as f64 * lag) * w;
        }
        v
    };
    let prior = |period: f64| -> f64 {
        let bpm = 60.0 / period;
        2f64.powf(-0.7 * (bpm.log2() - 7.0).powi(2))
    };
    // barrido grueso 1 ms
    let mut best = (f64::NEG_INFINITY, 0.0f64);
    let mut steps = 0;
    let mut p = p_min;
    while p <= p_max + 1e-9 {
        let s = score_period(p) * prior(p);
        if s > best.0 {
            best = (s, p);
        }
        p += 0.001;
        steps += 1;
    }
    let _ = steps;
    // fino: ±1.5 ms paso 20 µs alrededor de los 5 mejores del grueso
    let mut cands: Vec<(f64, f64)> = Vec::new();
    let mut p = p_min;
    let mut scores: Vec<(f64, f64)> = Vec::new();
    while p <= p_max + 1e-9 {
        scores.push((score_period(p) * prior(p), p));
        p += 0.001;
    }
    scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, pc) in scores.iter().take(5) {
        let mut pf = (pc - 0.0015).max(p_min);
        let top = (pc + 0.0015 + 1e-12).min(p_max);
        while pf <= top {
            cands.push((score_period(pf) * prior(pf), pf));
            pf += 2e-5;
        }
    }
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let (_, bp) = cands[0];
    let (bs, _) = cands[0];
    let _ = best;
    (60.0 / bp, bp * FPS, bs)
}

pub fn local_tempo(d: &[Vec<f64>], f0: usize, f1: usize) -> (f64, f64, f64) {
    let n = d.len();
    let margin = (8.0 * 0.5 * FPS) as usize;
    let mut a = f0.saturating_sub(margin);
    let mut b = (f1 + margin).min(n);
    if b.saturating_sub(a) < 64 {
        a = 0;
        b = n;
    }
    let acf = band_acfs(&d[a..b]);
    comb_tempo(&acf, b - a)
}

// ------------------------------------------------ CountBeats [VERIF §5.3] ----

pub fn count_beats(vals: &[f64], q: f64) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    let mut s = vals.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let r = quantile_sorted(&s, q);
    s.iter().filter(|&&v| v >= r * q).count() as f64
}

fn beat_frame_idxs(tempo: f64, f0: usize, f1: usize) -> Vec<usize> {
    let step = 60.0 / tempo * FPS;
    let n = ((((f1 - f0) as f64 - 1.0) / step).floor() as i64).max(1) as usize;
    (0..n).map(|i| f0 + (i as f64 * step).round() as usize).collect()
}

// ------------------------------------------------ features por segmento ----

/// 14×12 = 168 features por segmento (estructura §2 + escalas §6).
pub fn segment_features(
    l: &[Vec<f64>],
    d: &[Vec<f64>],
    bn: &[Vec<f64>],
    rms: &[f64],
    tempo: f64,
    f0: usize,
    f1: usize,
) -> (Vec<f64>, f64) {
    let f0 = f0.min(l.len());
    let f1 = f1.min(l.len());
    let (f0, f1) = if f1 < 8 { (0, f1.max(8).min(l.len())) } else { (f0, f1) };
    let (f0, f1) = if f1 - f0 < 8 { (f0.saturating_sub(8 - (f1 - f0)), f1) } else { (f0, f1) };
    let len = f1 - f0;
    let lag = 60.0 / tempo * FPS;
    // ACF por banda en la ventana
    let size = (2 * len).max(16).next_power_of_two();
    let mut acf_b = vec![vec![0.0f64; len]; NBANDS];
    PLANNER.with(|p| {
        let fft = p.borrow_mut().plan_fft_forward(size);
        let ifft = p.borrow_mut().plan_fft_inverse(size);
        let mut buf = vec![Complex::<f64> { re: 0.0, im: 0.0 }; size];
        let mut power = vec![Complex::<f64> { re: 0.0, im: 0.0 }; size];
        for band in 0..NBANDS {
            let mean: f64 = d[f0..f1].iter().map(|r| r[band]).sum::<f64>() / len as f64;
            for c in buf.iter_mut() {
                *c = Complex { re: 0.0, im: 0.0 };
            }
            for (i, r) in d[f0..f1].iter().enumerate() {
                buf[i].re = r[band] - mean;
            }
            fft.process(&mut buf);
            for i in 0..size {
                power[i].re = buf[i].re * buf[i].re + buf[i].im * buf[i].im;
                power[i].im = 0.0;
            }
            ifft.process(&mut power);
            for (i, o) in acf_b[band].iter_mut().enumerate() {
                *o = power[i].re / size as f64 / (len - i).max(1) as f64;
            }
        }
    });
    let idxs = beat_frame_idxs(tempo, 0, len);
    let mut feats = vec![0.0f64; NBANDS * 12];
    for band in 0..NBANDS {
        let col: Vec<f64> = l[f0..f1].iter().map(|r| r[band]).collect();
        let sal = salience_band(&bn[f0..f1].iter().map(|r| r[band]).collect::<Vec<_>>());
        let mut col_sorted = col.clone();
        col_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p40 = quantile_sorted(&col_sorted, 0.4);
        let p90 = quantile_sorted(&col_sorted, 0.9);
        let mut sal_sorted = sal.clone();
        sal_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let s90 = if !sal_sorted.is_empty() { quantile_sorted(&sal_sorted, 0.9) } else { 0.0 };
        let vals: Vec<f64> = idxs
            .iter()
            .filter(|&&i| i + 1 < col.len())
            .map(|&i| (col[i] * col[i + 1]).abs().sqrt())
            .collect();
        let mean: f64 = col.iter().sum::<f64>() / col.len() as f64;
        let a = &acf_b[band];
        let o = band * 12;
        feats[o] = p40 * BAND_SCALES_A[band];
        feats[o + 1] = (s90 * BAND_SCALES_A[band] * 1.9).powf(0.8);
        feats[o + 2] = mean * 1.41 * 0.3;
        feats[o + 3] = mean * BAND_SCALES_A[band] * 0.5;
        feats[o + 4] = count_beats(&vals, 0.9);
        feats[o + 5] = count_beats(&vals, 0.5);
        feats[o + 6] = (interp_val(&a, lag)
            + interp_val(&a, (lag - 1.0).max(0.0))
            + interp_val(&a, (lag + 1.0).min(a.len() as f64 - 1.0))
            + 0.5 * interp_val(&a, (lag - 2.0).max(0.0))
            + 0.5 * interp_val(&a, (lag + 2.0).min(a.len() as f64 - 1.0)))
            / 4.0;
        feats[o + 7] = interp_val(&a, (lag - 1.0).max(0.0));
        feats[o + 8] = interp_val(&a, (lag + 1.0).min(a.len() as f64 - 1.0));
        feats[o + 9] = interp_val(&a, (lag - 2.0).max(0.0));
        feats[o + 10] = interp_val(&a, (lag + 2.0).min(a.len() as f64 - 1.0));
        feats[o + 11] = p90;
    }
    for f in feats.iter_mut() {
        *f *= D_GLOBAL;
    }
    let ms: f64 = rms[f0..f1].iter().map(|r| r * r).sum::<f64>() / (f1 - f0) as f64;
    let vol_db = 20.0 * ms.sqrt().max(1e-50).log10();
    (feats, vol_db)
}

// --------------------------------------------------- segmentación [VERIF §5.2] ----

pub fn raw_segments_32(d: &[Vec<f64>], duration: f64) -> Vec<(f64, f64, f64)> {
    let mut segs = Vec::new();
    let mut t = 0.0f64;
    let mut guard = 0;
    while t < duration - 1.0 && guard < 200 {
        guard += 1;
        let f0 = (t * FPS) as usize;
        let f1 = ((t + 40.0) * FPS) as usize;
        let f1 = f1.min(d.len());
        let (tempo, _, _) = local_tempo(d, f0, f1);
        let dur_seg = 60.0 / tempo * 32.0;
        segs.push((t, (t + dur_seg).min(duration), tempo));
        t += dur_seg;
    }
    segs
}

pub fn quantize_to_16(beats: &[f64], raw: &[(f64, f64, f64)], duration: f64) -> Vec<(f64, f64, f64)> {
    let nb = beats.len();
    if nb == 0 {
        return Vec::new();
    }
    let idx16 = |t: f64| -> usize {
        let mut i = match beats.binary_search_by(|b| b.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(v) => v as i64,
            Err(v) => v as i64 - 1,
        };
        i = i.clamp(0, nb as i64 - 1);
        i as usize
    };
    let mut out = Vec::new();
    for &(t0, t1, tempo) in raw {
        let a = (idx16(t0) / 16) * 16;
        let b = (idx16(t1) / 16) * 16;
        let (a, b) = (a.min(nb - 1), b.min(nb - 1));
        if a < b {
            out.push((beats[a], beats[b], tempo));
        }
    }
    if let Some(last) = out.last_mut() {
        last.1 = last.1.max(duration);
    }
    out
}

/// Fusión de niveles contiguos iguales (regla DB §9.1).
pub fn merge_same_level(mut segs: Vec<(f64, f64, i32)>) -> Vec<(f64, f64, i32)> {
    let mut out: Vec<(f64, f64, i32)> = Vec::new();
    for s in segs.drain(..) {
        if let Some(last) = out.last_mut() {
            if last.2 == s.2 {
                last.1 = s.1;
                continue;
            }
        }
        out.push(s);
    }
    out
}

// ------------------------------------------------ análisis de pista ----

pub struct EnergyAnalysis {
    pub bn: Vec<Vec<f64>>,
    pub l: Vec<Vec<f64>>,
    pub d: Vec<Vec<f64>>,
    pub rms: Vec<f64>,
    pub n_frames: usize,
    pub duration: f64,
    pub tempo_global: f64,
    pub p95: f64,
}

fn linear_slope(t: &[f64], y: &[f64]) -> f64 {
    // polyfit deg 1: slope = cov(t,y)/var(t)
    let n = t.len() as f64;
    let mt = t.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var = 0.0;
    for i in 0..t.len() {
        cov += (t[i] - mt) * (y[i] - my);
        var += (t[i] - mt) * (t[i] - mt);
    }
    if var == 0.0 { 0.0 } else { cov / var }
}

impl EnergyAnalysis {
    pub fn new(x_mono: &[f32]) -> anyhow::Result<EnergyAnalysis> {
        let (b, rms, n_frames) = band_energies(x_mono);
        if n_frames < 8 {
            anyhow::bail!("audio insuficiente para energía");
        }
        let (bn, p95) = normalize_bands(&b);
        let l = robust_levels(&bn);
        let d = diff_store(&l);
        let duration = x_mono.len() as f64 / FS as f64;
        let acf = band_acfs(&d);
        let (tempo_global, _, _) = comb_tempo(&acf, n_frames);
        Ok(EnergyAnalysis { bn, l, d, rms, n_frames, duration, tempo_global, p95 })
    }

    pub fn raw(&self) -> Vec<(f64, f64, f64)> {
        raw_segments_32(&self.d, self.duration)
    }

    /// Segmentos re-anclados a beats externos (la vía validada).
    pub fn beats_anchored_segments(&self, beats: &[f64]) -> Vec<(f64, f64, f64, Vec<f64>, f64)> {
        let raw = self.raw();
        let q = quantize_to_16(beats, &raw, self.duration);
        q.into_iter()
            .map(|(t0, t1, tempo)| {
                let (feats, vol) = segment_features(
                    &self.l, &self.d, &self.bn, &self.rms, tempo,
                    (t0 * FPS) as usize, (t1 * FPS) as usize,
                );
                (t0, t1, tempo, feats, vol)
            })
            .collect()
    }

    /// Segmentos con rejilla propia (sin beats externos).
    pub fn own_segments(&self) -> Vec<(f64, f64, f64, Vec<f64>, f64)> {
        self.raw()
            .into_iter()
            .map(|(t0, t1, tempo)| {
                let (feats, vol) = segment_features(
                    &self.l, &self.d, &self.bn, &self.rms, tempo,
                    (t0 * FPS) as usize, (t1 * FPS) as usize,
                );
                (t0, t1, tempo, feats, vol)
            })
            .collect()
    }

    /// Features globales (122 dims): 14×8 + 10.
    pub fn global_features(&self) -> Vec<f64> {
        let n = self.n_frames;
        let t: Vec<f64> = (0..n).map(|i| i as f64 / (n as f64 - 1.0)).collect();
        let mut per_band: Vec<f64> = Vec::with_capacity(NBANDS * 8);
        for band in 0..NBANDS {
            let col: Vec<f64> = self.l.iter().map(|r| r[band]).collect();
            let mut s = col.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mu = col.iter().sum::<f64>() / n as f64 + 1e-12;
            let slope = linear_slope(&t, &col) / mu;
            let sal = salience_band(&self.bn.iter().map(|r| r[band]).collect::<Vec<_>>());
            let mut sal_s = sal.clone();
            sal_s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            per_band.extend([
                mu,
                quantile_sorted(&s, 0.40),
                quantile_sorted(&s, 0.50),
                quantile_sorted(&s, 0.90),
                quantile_sorted(&s, 0.95),
                slope,
                sal.iter().sum::<f64>() / n as f64,
                quantile_sorted(&sal_s, 0.90),
            ]);
        }
        let agg: Vec<f64> = self.d.iter().map(|r| r.iter().sum::<f64>()).collect();
        let idxs = beat_frame_idxs(self.tempo_global, 0, n);
        let vals: Vec<f64> = idxs
            .iter()
            .filter(|&&i| i + 1 < agg.len())
            .map(|&i| (agg[i] * agg[i + 1]).abs().sqrt())
            .collect();
        let cb9 = count_beats(&vals, 0.9);
        let cb5 = count_beats(&vals, 0.5);
        let centers: Vec<f64> = (0..NBANDS)
            .map(|i| {
                let (lo, hi) = (BAND_EDGES[i], BAND_EDGES[i + 1]);
                if lo > 0.0 { (lo * hi).sqrt() } else { hi / 2.0 }
            })
            .collect();
        let lm: Vec<f64> = (0..NBANDS).map(|b| self.l.iter().map(|r| r[b]).sum::<f64>() / n as f64 + 1e-12).collect();
        let logm: Vec<f64> = lm.iter().map(|v| v.ln()).collect();
        let logc: Vec<f64> = centers.iter().map(|c| c.ln()).collect();
        let tilt = linear_slope(&logc, &logm);
        let tot: f64 = lm.iter().sum();
        let rms_mean = self.rms.iter().sum::<f64>() / n as f64;
        let mut out = per_band;
        out.extend([
            self.tempo_global,
            n as f64 / FPS,
            cb9,
            cb5,
            tilt,
            lm[0..4].iter().sum::<f64>() / tot,
            lm[4..9].iter().sum::<f64>() / tot,
            lm[9..].iter().sum::<f64>() / tot,
            self.p95,
            20.0 * rms_mean.max(1e-50).log10(),
        ]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_track(bpm: f64, secs: f64) -> Vec<f32> {
        let n = (secs * FS as f64) as usize;
        let period = 60.0 / bpm;
        let mut x = vec![0.0f32; n];
        let mut t = 0.15;
        while t < secs {
            let start = (t * FS as f64) as usize;
            let len = (0.15 * FS as f64) as usize;
            for i in 0..len.min(n - start) {
                let tt = i as f64 / FS as f64;
                let env = (-tt * 12.0).exp();
                let kick = (2.0 * std::f64::consts::PI * 60.0 * tt).sin() * env;
                let hat = (2.0 * std::f64::consts::PI * 8000.0 * tt).sin() * (-tt * 90.0).exp() * 0.3;
                x[start + i] += (kick * 0.8 + hat) as f32;
            }
            t += period / 2.0; // corcheas (hat)
        }
        x
    }

    #[test]
    fn tempo_comb_parity_with_python() {
        // Test de PARIDAD: la misma señal sintética determinista pasa por el
        // Python original (harmonic-engine/engine/ref_energy.py) → 116.4431
        // BPM. El objetivo es validar el port, no la exactitud musical
        // (el comb de energía fue validado con música real: r=0.62, MAE 0.479;
        // la señal sintética no es musicalmente inequívoca para este estimador).
        let x = synth_track(128.0, 60.0);
        let ea = EnergyAnalysis::new(&x).unwrap();
        assert!(
            (ea.tempo_global - 116.4431).abs() < 0.2,
            "tempo {} vs referencia Python 116.4431",
            ea.tempo_global
        );
    }

    #[test]
    fn global_features_dim() {
        let x = synth_track(124.0, 40.0);
        let ea = EnergyAnalysis::new(&x).unwrap();
        let g = ea.global_features();
        assert_eq!(g.len(), 122);
        assert!(g.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn segments_quantized_to_beats() {
        let x = synth_track(120.0, 60.0);
        let ea = EnergyAnalysis::new(&x).unwrap();
        let beats = beats::analyze_pcm(&x);
        assert!(beats.valid());
        let segs = ea.beats_anchored_segments(&beats.beat_times());
        assert!(!segs.is_empty());
        for (t0, t1, _, feats, _) in &segs {
            assert!(t1 > t0);
            assert_eq!(feats.len(), 168);
        }
        // último segmento cubre el final
        let last = segs.last().unwrap();
        assert!(last.1 >= ea.duration - 1.0);
    }
}
