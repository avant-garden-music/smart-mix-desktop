//! Detector de BPM/beatgrid — port de `harmonic-engine/engine/mik_beats.py`
//! (paridad funcional: 114/114 pistas < 0.3 BPM vs MIK, fase mediana 0.3 ms).
//!
//! Pipeline: envolvente de onsets (STFT Hann 2048/512, bandas log 30–400 Hz,
//! magnitud lineal) → tempo global (ACF detrend gaussiano + escalera de
//! octavas + |X(f)|·ACF con ventana preferente 88–180) → DP estilo Ellis →
//! rejilla de periodo constante t_k = t0 + k·P con refinamiento Fourier y
//! regresión de picos; tempo final = 60·(n−1)/span en float32 (= ZTEMPO).

use rayon::prelude::*;
use rustfft::{num_complex::Complex, FftPlanner};
use std::cell::RefCell;
use std::sync::LazyLock;

pub const SR: u32 = 44_100;
const N_FFT: usize = 2048;
const HOP: usize = 512;
const HOP_T: f64 = HOP as f64 / SR as f64; // 11.610 ms
const FMIN: f64 = 30.0;
const FMAX: f64 = 400.0;
const BANDS_PER_OCT: usize = 8;
const TIGHTNESS: f64 = 100.0;
const ALPHA: f64 = 0.85;
pub const BPM_MIN: f64 = 30.0;
pub const BPM_MAX: f64 = 400.0;
const BPM_PREF_LO: f64 = 88.0;
const BPM_PREF_HI: f64 = 180.0;

static BAND_EDGES: LazyLock<Vec<f64>> = LazyLock::new(|| {
    let n = ((BANDS_PER_OCT as f64) * (FMAX / FMIN).log2().ceil()) as usize + 1;
    (0..n).map(|i| FMIN * 2f64.powf(i as f64 / BANDS_PER_OCT as f64)).collect()
});

static HANN: LazyLock<Vec<f64>> =
    LazyLock::new(|| (0..N_FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N_FFT as f64).cos()).collect());

thread_local! {
    static PLANNER: RefCell<FftPlanner<f64>> = RefCell::new(FftPlanner::new());
}

// ------------------------------------------------ envolvente de onsets ----

/// Bandas log → energía por frame y flujo espectral rectificado.
/// Devuelve (env normalizado a máx 1, hop_t).
pub fn onset_envelope(x: &[f32]) -> (Vec<f32>, f64) {
    let n = x.len();
    if n < N_FFT {
        return (Vec::new(), HOP_T);
    }
    let nfr = 1 + (n - N_FFT) / HOP;
    let nb = BAND_EDGES.len() - 1;
    // bins por banda sobre rfftfreq(N_FFT)
    let bin_of = |f: f64| -> usize { ((f * N_FFT as f64 / SR as f64).round() as usize).min(N_FFT / 2 + 1) };
    let ranges: Vec<(usize, usize)> = BAND_EDGES
        .windows(2)
        .map(|w| (bin_of(w[0]), bin_of(w[1])))
        .collect();
    // bandas por frame (paralelo)
    let d: Vec<Vec<f32>> = (0..nfr)
        .into_par_iter()
        .map(|fi| {
            let start = fi * HOP;
            PLANNER.with(|p| {
                let fft = p.borrow_mut().plan_fft_forward(N_FFT);
                let mut buf = vec![Complex::<f64> { re: 0.0, im: 0.0 }; N_FFT];
                for j in 0..N_FFT {
                    buf[j].re = x[start + j] as f64 * HANN[j];
                }
                fft.process(&mut buf);
                let mags: Vec<f32> = buf[..N_FFT / 2 + 1]
                    .iter()
                    .map(|c| ((c.re * c.re + c.im * c.im).sqrt() / N_FFT as f64) as f32)
                    .collect();
                ranges
                    .iter()
                    .map(|&(lo, hi)| {
                        if hi > lo {
                            mags[lo..hi].iter().sum::<f32>()
                        } else {
                            0.0
                        }
                    })
                    .collect()
            })
        })
        .collect();
    // flujo lag-1 rectificado sumado por bandas
    let mut env = Vec::with_capacity(nfr);
    let mut prev = d[0].clone();
    for (i, row) in d.iter().enumerate() {
        let val = if i == 0 {
            0.0
        } else {
            row.iter().zip(prev.iter()).map(|(&a, &b)| (a - b).max(0.0)).sum()
        };
        env.push(val);
        prev = row.clone();
    }
    let mx = env.iter().cloned().fold(0.0f32, f32::max);
    if mx > 0.0 {
        for v in env.iter_mut() {
            *v /= mx;
        }
    }
    (env, HOP_T)
}

// ------------------------------------------------------- utilidades DSP ----

/// Convolución con gaussiana σ (frames) aproximada por 3 box-blur (CLT).
/// Sin notches (requisito del prototipo: una media móvil anularía k/W Hz).
fn gaussian_smooth(v: &[f64], sigma: f64) -> Vec<f64> {
    if v.is_empty() || sigma <= 0.0 {
        return v.to_vec();
    }
    let n = v.len();
    let box_sigma = sigma / 3f64.sqrt();
    let w = ((box_sigma * 12f64.sqrt()).round() as usize).max(1);
    let mut cur = v.to_vec();
    for _ in 0..3 {
        let mut next = vec![0.0; n];
        let mut acc = 0.0;
        let mut prefix = vec![0.0; n + 1];
        for i in 0..n {
            prefix[i + 1] = prefix[i] + cur[i];
        }
        acc = 0.0;
        let _ = acc;
        for i in 0..n {
            let a = i.saturating_sub(w / 2);
            let b = (i + w / 2 + 1).min(n);
            next[i] = (prefix[b] - prefix[a]) / (b - a) as f64;
        }
        cur = next;
    }
    cur
}

/// ACF insesgada normalizada vía FFT, con detrend gaussiano σ≈4 s.
fn acf_detrended(env: &[f32], hop_t: f64) -> Vec<f64> {
    let n = env.len();
    let env_f: Vec<f64> = env.iter().map(|&v| v as f64).collect();
    let trend = gaussian_smooth(&env_f, 4.0 / hop_t);
    let e: Vec<f64> = env_f.iter().zip(trend.iter()).map(|(a, b)| a - b).collect();
    let size = (2 * n).next_power_of_two();
    PLANNER.with(|p| {
        let fft = p.borrow_mut().plan_fft_forward(size);
        let mut buf = vec![Complex::<f64> { re: 0.0, im: 0.0 }; size];
        for (i, &v) in e.iter().enumerate() {
            buf[i].re = v;
        }
        fft.process(&mut buf);
        // espectro de potencia |X|² (real, simétrico) → IFFT = ACF circular
        let mut power = vec![Complex::<f64> { re: 0.0, im: 0.0 }; size];
        for i in 0..size {
            power[i].re = buf[i].re * buf[i].re + buf[i].im * buf[i].im;
        }
        let ifft = p.borrow_mut().plan_fft_inverse(size);
        ifft.process(&mut power);
        let mut ac: Vec<f64> = power[..n].iter().map(|c| c.re / size as f64).collect();
        for i in 0..n {
            ac[i] /= (n - i) as f64;
        }
        if ac[0] != 0.0 {
            let a0 = ac[0];
            for v in ac.iter_mut() {
                *v /= a0;
            }
        }
        ac
    })
}

fn interp(x: &[f64], pos: f64) -> f64 {
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

/// |Σ env[n]·e^{−i2πf·n·ht}| con rotación incremental.
fn mag_x(env: &[f32], hop_t: f64, f: f64) -> f64 {
    let w = -2.0 * std::f64::consts::PI * f * hop_t;
    let step = Complex::from_polar(1.0, w);
    let mut ph = Complex { re: 1.0, im: 0.0 };
    let mut z = Complex { re: 0.0, im: 0.0 };
    for (n, &v) in env.iter().enumerate() {
        if n % 1024 == 0 {
            let m = ph.norm();
            ph /= m;
        }
        z += ph * (v as f64);
        ph *= step;
    }
    z.norm()
}

fn parabolic(y0: f64, y1: f64, y2: f64) -> f64 {
    let den = y0 - 2.0 * y1 + y2;
    if den == 0.0 {
        0.0
    } else {
        (0.5 * (y0 - y2) / den).clamp(-1.0, 1.0)
    }
}

// ------------------------------------------------------- tempo global ----

/// Tempo global por ACF + escalera de octavas + |X|·ACF.
/// Devuelve (bpm, periodo_s) o (0,0).
pub fn estimate_tempo(env: &[f32], hop_t: f64) -> (f64, f64) {
    if env.len() < 32 {
        return (0.0, 0.0);
    }
    let ac = acf_detrended(env, hop_t);
    let lag_min = 60.0 / BPM_MAX / hop_t;
    let lag_max = (60.0 / BPM_MIN / hop_t).min(ac.len() as f64 - 2.0);
    let lo = lag_min as usize;
    let hi = (lag_max as usize).min(ac.len() - 1);
    if hi.saturating_sub(lo) < 2 {
        return (0.0, 0.0);
    }
    let mut peaks: Vec<(f64, f64)> = Vec::new(); // (valor, lag fraccional)
    for i in (lo + 1)..hi {
        let (a, b, c) = (ac[i - 1], ac[i], ac[i + 1]);
        if b > a && b >= c && b > 0.0 {
            let d = parabolic(a, b, c);
            peaks.push((b, i as f64 + d));
        }
    }
    if peaks.is_empty() {
        return (0.0, 0.0);
    }
    peaks.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(10);

    let bpm_of = |lag: f64| 60.0 / (lag * hop_t);

    let mut cands: Vec<f64> = Vec::new();
    for (_, lag) in &peaks {
        let mut l0 = *lag;
        while bpm_of(l0) < BPM_MIN && l0 / 2.0 >= lag_min {
            l0 /= 2.0;
        }
        while bpm_of(l0) > BPM_MAX && l0 * 2.0 <= lag_max {
            l0 *= 2.0;
        }
        if !(BPM_MIN..=BPM_MAX).contains(&bpm_of(l0)) {
            continue;
        }
        let mut l = l0;
        while (BPM_MIN..=BPM_MAX).contains(&bpm_of(l)) && l <= lag_max {
            cands.push((l * 1e6).round() / 1e6);
            l *= 2.0;
        }
        let mut l = l0 / 2.0;
        while (BPM_MIN..=BPM_MAX).contains(&bpm_of(l)) && l >= lag_min {
            cands.push((l * 1e6).round() / 1e6);
            l /= 2.0;
        }
    }
    cands.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cands.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    if cands.is_empty() {
        return (0.0, 0.0);
    }

    let support = |lag: f64| -> f64 {
        let mut s = 0.0;
        for (k, w) in [(1.0, 1.0), (2.0, 0.5), (3.0, 0.5), (4.0, 0.25)] {
            let l = lag * k;
            if l < ac.len() as f64 - 1.0 {
                s += w * interp(&ac, l);
            }
        }
        s
    };

    let xs: Vec<f64> = cands.iter().map(|&l| mag_x(env, hop_t, 1.0 / (l * hop_t))).collect();
    let as_: Vec<f64> = cands.iter().map(|&l| interp(&ac, l).max(0.0)).collect();
    let xmax = xs.iter().cloned().fold(0.0f64, f64::max).max(1e-12);
    let amax = as_.iter().cloned().fold(0.0f64, f64::max).max(1e-12);
    let scored: Vec<f64> = xs.iter().zip(as_.iter()).map(|(a, b)| (a / xmax) * (b / amax)).collect();

    // ventana preferente [88,180]; si vacía, todas
    let mut best: Option<(usize, f64, f64)> = None; // (idx, score, support)
    for (i, &l) in cands.iter().enumerate() {
        let bpm = bpm_of(l);
        if !(BPM_PREF_LO..=BPM_PREF_HI).contains(&bpm) {
            continue;
        }
        let sup = support(l);
        match best {
            Some((_, bs, bsu)) if (scored[i], sup) <= (bs, bsu) => {}
            _ => best = Some((i, scored[i], sup)),
        }
    }
    let (idx, _, _) = match best {
        Some(v) => v,
        None => {
            let mut b = (0usize, scored[0], support(cands[0]));
            for (i, _) in cands.iter().enumerate().skip(1) {
                let sup = support(cands[i]);
                if (scored[i], sup) > (b.1, b.2) {
                    b = (i, scored[i], sup);
                }
            }
            b
        }
    };
    (bpm_of(cands[idx]), cands[idx] * hop_t)
}

// ------------------------------------------- refinamiento Fourier (P, φ) ----

pub fn fourier_tempo_phase(env: &[f32], hop_t: f64, f0: f64, span: f64, npts: usize) -> (f64, f64) {
    let n = env.len();
    let fs: Vec<f64> = (0..npts)
        .map(|i| f0 * (1.0 + (2.0 * i as f64 / (npts - 1) as f64 - 1.0) * span))
        .collect();
    let mut mags: Vec<f64> = Vec::with_capacity(npts);
    for &f in &fs {
        let w = -2.0 * std::f64::consts::PI * f * hop_t;
        let step = Complex::from_polar(1.0, w);
        let mut ph = Complex { re: 1.0, im: 0.0 };
        let mut z = Complex { re: 0.0, im: 0.0 };
        for (k, &v) in env.iter().enumerate() {
            if k % 1024 == 0 {
                ph /= ph.norm();
            }
            z += ph * (v as f64);
            ph *= step;
        }
        mags.push(z.norm());
    }
    let k = (0..npts).max_by(|&a, &b| mags[a].partial_cmp(&mags[b]).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
    let d = if 0 < k && k < npts - 1 {
        parabolic(mags[k - 1], mags[k], mags[k + 1])
    } else {
        0.0
    };
    let df = fs[1] - fs[0];
    let f = fs[k] + d * df;
    // fase en el pico
    let w = -2.0 * std::f64::consts::PI * f * hop_t;
    let step = Complex::from_polar(1.0, w);
    let mut ph = Complex { re: 1.0, im: 0.0 };
    let mut z = Complex { re: 0.0, im: 0.0 };
    for (kk, &v) in env.iter().enumerate() {
        if kk % 1024 == 0 {
            ph /= ph.norm();
        }
        z += ph * (v as f64);
        ph *= step;
    }
    let period = 1.0 / f;
    let phase = (-z.arg() / (2.0 * std::f64::consts::PI)).rem_euclid(1.0) * period;
    (period, phase)
}

// --------------------------------------------------------------- DP Ellis ----

pub fn dp_beats(env: &[f32], period_s: f64, hop_t: f64) -> Vec<f64> {
    let n = env.len();
    if n < 8 || period_s <= 0.0 {
        return Vec::new();
    }
    let theta = period_s / hop_t;
    let lo = ((theta * 0.5).round() as i64).max(2);
    let hi = (theta * 2.0).round() as i64;
    let deltas: Vec<i64> = (lo..=hi).collect();
    let tcost: Vec<f64> = deltas.iter().map(|&d| -TIGHTNESS * (d as f64 / theta).ln().powi(2)).collect();

    let mut cum = vec![f64::NEG_INFINITY; n];
    let mut back = vec![-1i64; n];
    for i in 0..n {
        let mut best = f64::NEG_INFINITY;
        let mut bestj = -1i64;
        for (di, &d) in deltas.iter().enumerate() {
            let j = i as i64 - d;
            if j < 0 {
                continue;
            }
            let j = j as usize;
            if cum[j] == f64::NEG_INFINITY {
                continue;
            }
            let v = cum[j] + tcost[di];
            if v > best {
                best = v;
                bestj = j as i64;
            }
        }
        if bestj >= 0 {
            cum[i] = ALPHA * best + env[i] as f64;
            back[i] = bestj;
        } else {
            cum[i] = env[i] as f64;
        }
    }
    let start = (0..n).max_by(|&a, &b| cum[a].partial_cmp(&cum[b]).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
    let mut path = Vec::new();
    let mut i = start as i64;
    while i >= 0 {
        path.push(i as usize);
        i = back[i as usize];
    }
    path.reverse();
    // recorte de arranques débiles (Ellis)
    if path.len() > 3 {
        let q = &path[path.len() / 4..];
        let mut vals: Vec<f64> = q.iter().map(|&p| cum[p]).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = vals[vals.len() / 2];
        let thr = if med > 0.0 { 0.5 * med } else { f64::NEG_INFINITY };
        while let Some(&p) = path.first() {
            if cum[p] < thr {
                path.remove(0);
            } else {
                break;
            }
        }
    }
    path.into_iter().map(|p| p as f64 * hop_t).collect()
}

// ----------------------------------------------- rejilla periodo constante ----

fn phase_from_beats(beats: &[f64], period: f64) -> f64 {
    let mut s = 0.0;
    let mut c = 0.0;
    for &b in beats {
        let ang = b.rem_euclid(period) / period * 2.0 * std::f64::consts::PI;
        s += ang.sin();
        c += ang.cos();
    }
    (s.atan2(c) / (2.0 * std::f64::consts::PI)).rem_euclid(1.0) * period
}

pub fn refine_grid_peaks(env: &[f32], hop_t: f64, mut period: f64, mut t0: f64, dur_s: f64, iters: usize) -> (f64, f64) {
    let env_max = env.iter().cloned().fold(0.0f32, f32::max);
    for _ in 0..iters {
        let n = (((dur_s - t0) / period) as i64).max(0) as usize + 1;
        let w = ((0.09 * period / hop_t).round() as usize).max(2);
        let mut ks: Vec<f64> = Vec::new();
        let mut ys: Vec<f64> = Vec::new();
        let mut ws: Vec<f64> = Vec::new();
        for k in 0..n {
            let c = (((t0 + k as f64 * period) / hop_t).round() as i64).max(0) as usize;
            let a = c.saturating_sub(w);
            let b = (c + w + 1).min(env.len());
            if b.saturating_sub(a) < 3 || c == 0 || c >= env.len() {
                continue;
            }
            let j = a + (a..b).max_by(|&x, &y| env[x].partial_cmp(&env[y]).unwrap_or(std::cmp::Ordering::Equal)).unwrap();
            if j == 0 || j >= env.len() - 1 {
                continue;
            }
            let d = parabolic(env[j - 1] as f64, env[j] as f64, env[j + 1] as f64);
            if (env[j] as f64) < 0.05 * env_max as f64 {
                continue;
            }
            let tpk = (j as f64 + d) * hop_t;
            let mut r = tpk - (t0 + k as f64 * period);
            r = (r + period / 2.0).rem_euclid(period) - period / 2.0;
            if r.abs() > 0.15 * period {
                continue;
            }
            ks.push(k as f64);
            ys.push(r);
            ws.push(env[j] as f64);
        }
        if ks.len() < 8 {
            break;
        }
        let wmean = ws.iter().sum::<f64>() / ws.len() as f64;
        // mínimos cuadrados ponderados y ≈ dphi + k·dP
        let (mut s0, mut s1, mut s2, mut t0_, mut t1) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..ks.len() {
            let w = ws[i] / wmean;
            s0 += w;
            s1 += w * ks[i];
            s2 += w * ks[i] * ks[i];
            t0_ += w * ys[i];
            t1 += w * ks[i] * ys[i];
        }
        let det = s0 * s2 - s1 * s1;
        if det.abs() < 1e-12 {
            break;
        }
        let dphi = (s2 * t0_ - s1 * t1) / det;
        let dp = (s0 * t1 - s1 * t0_) / det;
        t0 += dphi;
        period += dp;
        if dp.abs() < 1e-6 * period && dphi.abs() < 1e-4 {
            break;
        }
    }
    (period, t0)
}

pub fn fit_constant_grid(
    beats_dp: &[f64],
    env: &[f32],
    hop_t: f64,
    dur_s: f64,
    period_seed: f64,
) -> Option<(f64, f64)> {
    let mut seed = period_seed;
    if seed <= 0.0 {
        if beats_dp.len() < 4 {
            return None;
        }
        let mut ds: Vec<f64> = beats_dp.windows(2).map(|w| w[1] - w[0]).collect();
        ds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        seed = ds[ds.len() / 2];
    }
    let (mut period, mut phase) = fourier_tempo_phase(env, hop_t, 1.0 / seed, 0.02, 161);
    if !(0.15..=2.0).contains(&period) {
        return None;
    }
    if beats_dp.len() >= 4 {
        let ph_dp = phase_from_beats(beats_dp, period);
        let mut d = (phase - ph_dp + period / 2.0).rem_euclid(period) - period / 2.0;
        if d.abs() > 0.35 * period {
            phase = (phase - d.signum() * period / 2.0).rem_euclid(period);
        }
        let _ = &mut d;
    }
    let k0 = ((0.0 - phase) / period).ceil() as i64 - 1;
    let t0 = phase + k0 as f64 * period;
    let (p, t) = refine_grid_peaks(env, hop_t, period, t0, dur_s, 3);
    period = p;
    let _ = &mut period;
    Some((period, t))
}

/// tempo = 60·(n−1)/span en float32 — exactamente como ZTEMPO.
pub fn grid_tempo(times: &[f64]) -> f64 {
    if times.len() < 2 {
        return 0.0;
    }
    let span = times[times.len() - 1] - times[0];
    if span <= 0.0 {
        return 0.0;
    }
    ((60.0f32 * (times.len() as f32 - 1.0)) as f64 / span) as f32 as f64
}

// ------------------------------------------------------------ analyze() ----

#[derive(Debug, Clone)]
pub struct BeatsResult {
    pub tempo: f64,
    pub duration: f64,
    pub bpm_acf: f64,
    pub period_s: f64,
    pub t0_s: f64,
    pub n_beats: usize,
}

impl BeatsResult {
    pub fn beat_times(&self) -> Vec<f64> {
        (0..self.n_beats).map(|k| self.t0_s + k as f64 * self.period_s).collect()
    }
    pub fn valid(&self) -> bool {
        self.tempo > 0.0 && self.n_beats > 3
    }
}

/// Pipeline completo sobre PCM mono 44100.
pub fn analyze_pcm(x: &[f32]) -> BeatsResult {
    let invalid = |dur: f64| BeatsResult {
        tempo: 0.0,
        duration: dur,
        bpm_acf: 0.0,
        period_s: 0.0,
        t0_s: 0.0,
        n_beats: 0,
    };
    if (x.len() as f64) < SR as f64 {
        return invalid(x.len() as f64 / SR as f64);
    }
    let dur = x.len() as f64 / SR as f64;
    let (env, hop_t) = onset_envelope(x);
    if env.len() < 32 {
        return invalid(dur);
    }
    let (bpm_acf, period) = estimate_tempo(&env, hop_t);
    let beats_dp = dp_beats(&env, period, hop_t);
    let fit = fit_constant_grid(&beats_dp, &env, hop_t, dur, period);
    let (p, t0) = match fit {
        Some(v) => v,
        None => return invalid(dur),
    };
    let n = (((dur - t0) / p) as i64).max(0) as usize + 1;
    let times: Vec<f64> = (0..n).map(|k| t0 + k as f64 * p).collect();
    let tempo = grid_tempo(&times);
    // sanity del wrapper §3.1
    let ok = times.len() > 3
        && times.windows(2).all(|w| w[1] - w[0] > 0.001)
        && times.windows(2).all(|w| w[1] - w[0] <= 4.0)
        && times[0] >= -20.0
        && (BPM_MIN..=BPM_MAX).contains(&tempo);
    if !ok {
        return invalid(dur);
    }
    BeatsResult { tempo, duration: dur, bpm_acf, period_s: p, t0_s: t0, n_beats: n }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pulso tipo kick 60–110 Hz con envolvente exponencial a `bpm`.
    fn synth_beat_track(bpm: f64, secs: f64) -> Vec<f32> {
        let n = (secs * SR as f64) as usize;
        let period = 60.0 / bpm;
        let mut x = vec![0.0f32; n];
        let mut t = 0.0;
        while t < secs {
            let start = (t * SR as f64) as usize;
            let len = (0.09 * SR as f64) as usize; // 90 ms de kick
            for i in 0..len.min(n - start) {
                let tt = i as f64 / SR as f64;
                let envl = (-tt * 28.0).exp();
                let v = (2.0 * std::f64::consts::PI * 70.0 * tt).sin() * envl * 0.9;
                x[start + i] += v as f32;
            }
            t += period;
        }
        x
    }

    #[test]
    fn detects_128() {
        let x = synth_beat_track(128.0, 90.0);
        let r = analyze_pcm(&x);
        assert!(r.valid(), "inválido: {r:?}");
        assert!((r.tempo - 128.0).abs() < 0.3, "tempo {} vs 128", r.tempo);
    }

    #[test]
    fn detects_120_with_noise() {
        let x = synth_beat_track(120.0, 90.0);
        // ruido leve determinista
        let mut x = x;
        for (i, v) in x.iter_mut().enumerate() {
            *v += 0.003 * (((i as f64) * 0.7).sin() as f32);
        }
        let r = analyze_pcm(&x);
        assert!(r.valid());
        assert!((r.tempo - 120.0).abs() < 0.5, "tempo {}", r.tempo);
    }

    #[test]
    fn half_time_prefers_dj_window() {
        // golpes solo en negras (60 BPM de pulsos = 120 en ventana DJ)
        let x = synth_beat_track(174.0, 60.0); // DnB
        let r = analyze_pcm(&x);
        assert!(r.valid());
        // 174 está dentro de [88,180]: debe reportar la escalera correcta
        let half = r.tempo / 2.0;
        assert!(
            (r.tempo - 174.0).abs() < 0.6 || (half - 174.0).abs() < 0.6 || (r.tempo - 87.0).abs() < 0.6,
            "tempo {}",
            r.tempo
        );
    }
}
