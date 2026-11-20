//! cue_engine — port fiel de la ruta de cues de herramienta de referencia (harmonic-engine):
//! envolvente EXACTA (WaveformAnalyzerFast v2, ref_waveform_env.py) +
//! DetectLikelyCuePointsV3 (granules + scores, ref_cues.py) + selección con
//! modelo lineal LR (regla empírica ajustada, time_match ~79% @ ±1 beat).
//!
//! Todo el cálculo es float32 donde el binario lo es (acumuladores f1..f3),
//! float64 donde Python/numpy lo eran. Constantes [VERIF] del decompilado.

pub const FS_DEFAULT: f64 = 44_100.0;
pub const N_FFT: usize = 4096;
pub const HOP: usize = N_FFT / 4; // 1024 = samplesPerBucket
pub const DUAL_STEP: usize = 2 * HOP; // 2048
pub const MAG_KMAX: usize = 767;
pub const MAG_LEN: usize = MAG_KMAX; // resize(maxBin-1)=766 → índices 0..765
pub const PYRAMID_HEAD0: usize = 80;
pub const PYRAMID_HEAD_STEP: usize = 40;
pub const FFT_SCALE: f32 = 49_152.0; // 32768·3·0.5

// Valores de referencia del ingenieria inversa; se conservan documentados.
#[allow(dead_code)]
const W11_RAW: [f32; 11] = [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0];
const W11: [f32; 11] = [
    2.0 / 47.0,
    3.0 / 47.0,
    4.0 / 47.0,
    5.0 / 47.0,
    6.0 / 47.0,
    7.0 / 47.0,
    6.0 / 47.0,
    5.0 / 47.0,
    4.0 / 47.0,
    3.0 / 47.0,
    2.0 / 47.0,
];
const G11: [f32; 11] = [1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 3.0, 2.0, 2.0, 1.0, 1.0];
const C_137745: f32 = 1.37745;
#[allow(dead_code)] // referencia RE, sin uso actual
const R_TWEAK: f32 = 0.89;
const R: f32 = 0.89 * 0.3061; // 0.2724290

// ref_cues.py
const W_PHRASE: [f64; 5] = [0.4, 0.3, 0.1, 0.0, 0.0];
const GRAN_MARGIN_IN: f64 = 0.02;
const GRAN_MARGIN_OUT: f64 = 0.12;
const GRAN_MARGIN_PRE: f64 = 0.18;
const GRAN_P_LOW: f64 = 1.15;
const GRAN_P_HIGH: f64 = 5.0;
const GRAN_THR_CH1: f64 = 0.3;
pub const ENV_BUCKET: f64 = 1024.0;
const CS_WEIGHTS: [f64; 5] = [96.0, 96.0, 96.0, 98.304, 245.76];
const CS_RISE_W: [f64; 3] = [3.2, 1.6, 0.8];
pub const MAX_CUES: usize = 8;
const LR_COEF: [f64; 6] = [1.659, -0.573, -2.001, 4.783, 0.071, -0.430];
const LR_INTERCEPT: f64 = -0.566;

// ---------------------------------------------------------------- eq A-weight
fn a_weight_eq(n: usize, fs: f64) -> Vec<f32> {
    let mut eq = vec![0.0f32; n];
    for k in 0..n {
        let f = fs * k as f64 / n as f64;
        let f2 = f * f;
        let num = f2 * f2 * 148_840_000.0;
        let den =
            (f2 + 148_840_000.0) * (f2 + 424.36) * ((f2 + 11_599.29) * (f2 + 544_496.41)).sqrt();
        eq[k] = if den > 0.0 { (num / den) as f32 } else { 0.0 };
    }
    eq
}

fn w578(b: usize) -> Vec<f32> {
    (0..b)
        .map(|k| {
            let kf = k as f32;
            let a = ((kf - 16.0) * 0.0625).clamp(0.0, 1.0);
            let bb = (1.0 - (kf - 70.0) / 40.0).clamp(0.0, 1.0);
            a * 16.0 * bb
        })
        .collect()
}

// ---------------------------------------------------------------- pirámide log
fn pyramid(mut v: Vec<f32>) -> Vec<f32> {
    let mut head = PYRAMID_HEAD0;
    while head < v.len() {
        let size = v.len();
        let mut out = vec![0.0f32; size];
        out[..head].copy_from_slice(&v[..head]);
        let mut o = head;
        let mut i = head;
        while i + 7 < size {
            for j in 0..4 {
                out[o + j] = (v[i + 2 * j] * v[i + 2 * j + 1]).sqrt();
            }
            o += 4;
            i += 8;
        }
        while i + 1 < size {
            out[o] = (v[i] * v[i + 1]).sqrt();
            o += 1;
            i += 2;
        }
        // T7 fix (2026-11-17): la reducción piramidal escrita en `out` se
        // DESCARTABA (v.truncate(o) conservaba el frente SIN reducir de v).
        // El port Python (validado EXACTO vs blob ZWAVEFORM de herramienta de referencia) hace
        // `v = out[:o]`: el buffer reducido ES el resultado del nivel.
        // Caracterización: tests/pyramid_matches_python_reference (bit-exacto).
        out.truncate(o);
        v = out;
        head += PYRAMID_HEAD_STEP;
    }
    v
}

// ------------------------------------------------- media geom. ponderada (1/21)
fn smooth7(v: &[f32]) -> Vec<f32> {
    let n = v.len();
    let mut acc = vec![1.0f64; n];
    const OFFS: [(i32, f64); 11] = [
        (-1, 3.0),
        (0, 3.0),
        (1, 3.0),
        (-3, 2.0),
        (-2, 2.0),
        (2, 2.0),
        (3, 2.0),
        (-5, 1.0),
        (-4, 1.0),
        (4, 1.0),
        (5, 1.0),
    ];
    for &(off, w) in &OFFS {
        if off >= 0 {
            let j = off as usize;
            if j < n {
                for k in 0..n - j {
                    acc[k] *= (v[k + j] as f64).powf(w);
                }
            }
        } else {
            let k0 = (-off) as usize;
            if k0 < n {
                for k in k0..n {
                    acc[k] *= (v[k - k0] as f64).powf(w);
                }
            }
        }
    }
    acc.iter().map(|a| a.powf(1.0 / 21.0) as f32).collect()
}

fn fir11_freq(x: &[f32]) -> Vec<f32> {
    let n = x.len();
    let mut acc: Vec<f32> = x.iter().map(|v| 7.0 * v).collect();
    for d in 1..=5usize {
        let w = 7.0 - d as f32; // taps (1,6),(2,5),(3,4),(4,3),(5,2)
        for i in d..n {
            acc[i] += w * x[i - d];
        }
        for i in 0..n - d {
            acc[i] += w * x[i + d];
        }
    }
    acc.iter().map(|a| a / 47.0).collect()
}

// ------------------------------------------------------------- AnalyzerFast
struct AnalyzerFast {
    hist12: std::collections::VecDeque<Vec<f32>>,
    hist3: std::collections::VecDeque<Vec<f32>>,
    arr1: Vec<f32>,
    arr2: Vec<f32>,
    zeros: Vec<f32>,
    w578v: Vec<f32>,
    b: usize,
}

impl AnalyzerFast {
    fn new() -> Self {
        AnalyzerFast {
            hist12: Default::default(),
            hist3: Default::default(),
            arr1: Vec::new(),
            arr2: Vec::new(),
            zeros: Vec::new(),
            w578v: Vec::new(),
            b: 0,
        }
    }

    fn feed_frame(&mut self, mags: Vec<f32>) {
        self.hist12.push_back(mags);
        if self.hist12.len() > 12 {
            self.hist12.pop_front();
        }
        if self.hist12.len() > 5 {
            let sm = smooth7(&self.hist12[self.hist12.len() - 6]);
            self.hist3.push_back(sm);
            if self.hist3.len() > 3 {
                self.hist3.pop_front();
            }
        }
        if self.hist12.len() > 6 {
            self.emit();
        }
    }

    fn emit(&mut self) {
        if self.b == 0 {
            let b0 = self.hist12.back().map(|v| v.len()).unwrap_or(0);
            if b0 > 0 {
                self.b = b0;
                self.zeros = vec![0.0; b0];
                self.w578v = w578(b0);
            }
        }
        let b = self.b;
        let len_h = self.hist12.len();
        let j = self.arr1.len() as i64;
        let base = j + 6 - (len_h as i64 - 1);

        let mut vec500 = vec![0.0f32; b];
        let mut acc = vec![1.0f64; b];
        for k in 0..11 {
            let t = j - 5 + k as i64;
            let idx = t - base;
            let src: &[f32] = if idx >= 0 && (idx as usize) < len_h {
                &self.hist12[idx as usize]
            } else {
                &self.zeros
            };
            for i in 0..b {
                vec500[i] += W11[k] * src[i];
                let v = if src[i] > 1e-30 { src[i] as f64 } else { 1e-30 };
                acc[i] *= v.powf(G11[k] as f64);
            }
        }
        let vec518: Vec<f32> = acc.iter().map(|a| a.powf(1.0 / 21.0) as f32).collect();
        let vec530 = fir11_freq(&vec500);

        // b trio = sm(j-1), sm(j), sm(j+1)
        let sm3 = &self.hist3;
        let mut trio: Vec<Vec<f32>> = Vec::new();
        let start = sm3.len() as i64 - 3;
        for i in start..start + 3 {
            if i >= 0 && (i as usize) < sm3.len() {
                trio.push(sm3[i as usize].clone());
            } else {
                trio.push(self.zeros.clone());
            }
        }
        let mut c = vec![0.0f32; b];
        for k in 0..b {
            c[k] = (trio[0][k] + 2.0 * trio[1][k] + trio[2][k]) * 0.0625;
        }
        let s: Vec<f32> = vec530.iter().map(|v| R * v).collect();

        // f1
        let mut f1: f32 = 0.0;
        for k in 1..b {
            let term = ((c[k] - s[k]) * 1200.0) / (k as f32);
            if term > 0.0 {
                f1 += term;
            }
        }
        // f2 (k = 3..B-3)
        let mut f2: f32 = 0.0;
        if b > 5 {
            let mut d = vec![0.0f32; b];
            for k in 3..b - 3 {
                d[k] =
                    vec518[k] * 0.5 + (vec518[k - 1] + vec518[k + 1]) * 0.25 - C_137745 * vec530[k];
            }
            for k in 3..b - 3 {
                let rect = if d[k] > 0.0 { d[k] } else { 0.0 };
                let t = self.w578v[k] * rect;
                f2 += t * t;
            }
        }
        // f3 (k=0 con divisor 1.0, como np.concatenate(([1], kk)))
        let mut f3: f32 = 0.0;
        for k in 0..b {
            let c3 = c[k] * c[k] * c[k];
            let s3 = s[k] * s[k] * s[k];
            let base = if c3 - s3 > 0.0 { c3 - s3 } else { 0.0 };
            let x = (base.powf(1.0 / 3.0) * 0.2).powf(0.3);
            let yv = if vec530[k] * 0.035 > 0.0 {
                (vec530[k] * 0.035).powf(0.7)
            } else {
                0.0
            };
            let div = if k == 0 { 1.0 } else { k as f32 };
            let val = (yv * x * 3960.0) / div;
            if val > 0.0 {
                f3 += val;
            }
        }
        let a1 = (f1 * 2f32.powi(-24)).abs().sqrt() * (3.0 * 0.2);
        let v1 = a1 * 6.0;
        let t2 = (f2 * 2f32.powi(-48)).abs().sqrt();
        let v2 = t2.sqrt() * 6.0;
        let a3 = (f3 * 2f32.powi(-24)).abs().sqrt() * 0.6;
        let _v3 = a3 * 6.0;
        self.arr1.push(v1);
        self.arr2.push(v2);
        // arr3 no se usa para cues; se omite (ch3 = tonalidad)
    }

    fn finish(mut self) -> (Vec<f32>, Vec<f32>) {
        let n = self.hist12.front().map(|v| v.len()).unwrap_or(0);
        for _ in 0..6 {
            self.feed_frame(vec![0.0; n]);
        }
        let m = self.arr2.len();
        let mut a2 = self.arr2.clone();
        if m > 0 {
            let mut out = vec![0.0f32; m];
            for i in 0..m {
                let prev = if i > 0 { a2[i - 1] } else { 0.0 };
                let nxt = if i + 1 < m { a2[i + 1] } else { 0.0 };
                out[i] = prev * 0.25 + a2[i] * 0.5 + nxt * 0.25;
            }
            a2 = out;
        }
        (self.arr1, a2)
    }
}

/// Envolvente exacta: pcm mono (L+R, sin 1/2) → (ch1, ch2).
pub fn envelope_exact(mono: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let eq = a_weight_eq(N_FFT, FS_DEFAULT);
    // planner thread-local (como beats/energy/key): el plan de N_FFT se
    // cachea entre pistas en vez de re-planificarse por llamada.
    thread_local! {
        static PLANNER: std::cell::RefCell<rustfft::FftPlanner<f32>> =
            std::cell::RefCell::new(rustfft::FftPlanner::new());
    }
    let fft = PLANNER.with(|p| p.borrow_mut().plan_fft_forward(N_FFT));
    let mut an = AnalyzerFast::new();
    let t = mono.len();
    let mut pos = 0usize;
    let mut buf: Vec<rustfft::num_complex::Complex<f32>> =
        vec![rustfft::num_complex::Complex::new(0.0, 0.0); N_FFT];
    while pos + N_FFT + HOP <= t {
        for i in 0..N_FFT {
            let a = mono[pos + i];
            let b = mono[pos + HOP + i];
            buf[i] = rustfft::num_complex::Complex::new(a, b);
        }
        fft.process(&mut buf);
        // unpack dual (2 espectros reales de una FFT compleja)
        let n = N_FFT;
        for which in 0..2 {
            let mut mags = vec![0.0f32; MAG_LEN];
            for k in 1..=MAG_KMAX - 1 {
                let zk = buf[k];
                let zc = buf[(n - k) % n].conj();
                let spec = if which == 0 {
                    (zk + zc) * 0.5
                } else {
                    (zk - zc).conj() * 0.5
                };
                // mag[k] = eq[k]·|X[k]−½(X[k−1]+X[k+1])|·49152
                let km1 = if k >= 1 { buf[k - 1] } else { buf[0] };
                let kp1 = buf[k + 1];
                let spm1 = if k >= 1 {
                    let zc1 = buf[(n - (k - 1)) % n].conj();
                    if which == 0 {
                        (km1 + zc1) * 0.5
                    } else {
                        (km1 - zc1).conj() * 0.5
                    }
                } else {
                    rustfft::num_complex::Complex::new(0.0, 0.0)
                };
                let zcp1 = buf[(n - (k + 1)) % n].conj();
                let spp1 = if which == 0 {
                    (kp1 + zcp1) * 0.5
                } else {
                    (kp1 - zcp1).conj() * 0.5
                };
                let d = spec - (spm1 + spp1) * 0.5;
                let m = eq[k] * (d.re * d.re + d.im * d.im).sqrt();
                mags[k] = FFT_SCALE * m;
            }
            let bands = pyramid(mags);
            an.feed_frame(bands);
        }
        pos += DUAL_STEP;
    }
    an.finish()
}

// -------------------------------------------------------- WeightedAverage
fn trapz_pow_mean(
    env: &[f32],
    i0: usize,
    i1: usize,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    thr: f64,
    p: f64,
) -> f64 {
    let mut acc = 0.0f64;
    for i in i0..i1 {
        let mut v = env[i] as f64;
        if v < 0.0 {
            v = 0.0;
        }
        if v < thr {
            v = 0.0;
        }
        let t = i as f64 + 0.5;
        let w = if a < t && t < b {
            let x = (t - a) * 0.5 / (b - a);
            if x > 0.0 && x < 1.0 {
                0.5 - 0.5 * (2.0 * std::f64::consts::PI * x).cos()
            } else {
                0.0
            }
        } else if t <= c {
            1.0
        } else if c < t && t < d {
            let x = (t - c) * 0.5 / (d - c) + 0.5;
            if x > 0.0 && x < 1.0 {
                0.5 - 0.5 * (2.0 * std::f64::consts::PI * x).cos()
            } else {
                0.0
            }
        } else {
            0.0
        };
        acc += v.powf(p) * w;
    }
    let mut length = (c - b) + ((b - a) + (d - c)) * 0.5;
    if length <= 0.0 {
        length = 1.0;
    }
    (acc / length).powf(1.0 / p)
}

/// 6 features por beat [t0,t1) (GranuleFeatures@0x3457c).
fn granule_features(t0: f64, t1: f64, ch1: &[f32], ch2: &[f32]) -> [f64; 6] {
    let span = t1 - t0;
    let sr = FS_DEFAULT;
    let bucket = ENV_BUCKET;
    let wa = |ch: &[f32], m0: f64, m1: f64, thr: f64, p: f64| -> f64 {
        let a = (t0 - span * m0) * sr / bucket;
        let b = (t0 - span * m1) * sr / bucket;
        let c = (t1 - span * m0) * sr / bucket;
        let d = (t1 - span * m1) * sr / bucket;
        let i0 = a.max(0.0) as usize;
        let i1 = (d as usize).min(ch.len());
        if i1 <= i0 {
            return 0.0;
        }
        trapz_pow_mean(ch, i0, i1, a, b, c, d, thr, p)
    };
    let gf0 = wa(ch2, GRAN_MARGIN_OUT, GRAN_MARGIN_IN, 0.0, GRAN_P_LOW);
    let gf1 = wa(
        ch1,
        GRAN_MARGIN_PRE,
        GRAN_MARGIN_OUT,
        GRAN_THR_CH1,
        GRAN_P_LOW,
    );
    let gf2 = wa(ch2, GRAN_MARGIN_OUT, GRAN_MARGIN_IN, 0.0, GRAN_P_HIGH);
    let gf3 = wa(
        ch1,
        GRAN_MARGIN_PRE,
        GRAN_MARGIN_OUT,
        GRAN_THR_CH1,
        GRAN_P_HIGH,
    );
    [gf0, gf1, gf2, gf3, gf0 + gf1, 0.0]
}

fn difference_sum(arr: &[f64], sign: f64, start: usize, step: usize, width: usize) -> f64 {
    let n = arr.len();
    let nsub = start / step;
    let npos = if width > 0 {
        ((width.max(step) - 1) / width) + 1
    } else {
        1
    };
    let mut total = 0.0f64;
    for k in 0..npos {
        let b = 64usize + k * width;
        let (mut s_after, mut s_before) = (0.0f64, 0.0f64);
        let mut cnt = 0usize;
        for j in 0..nsub {
            let bi = b + j * step;
            let ai = bi as i64 - start as i64;
            if ai >= 0 && (ai as usize) < n && bi < n {
                s_after += arr[bi];
                s_before += arr[ai as usize];
                cnt += 1;
            }
        }
        if cnt == 0 {
            continue;
        }
        let diff = (s_after - s_before) / cnt as f64;
        total += if diff >= 0.0 {
            diff
        } else if sign < 0.0 {
            -diff.abs()
        } else {
            (sign * diff).abs()
        };
    }
    total / npos as f64
}

fn calculate_score(gf: &[[f64; 6]], i: usize) -> f64 {
    let n = gf.len();
    let build = |field: usize| -> Vec<f64> {
        (0..128)
            .rev()
            .map(|k| {
                let idx = i as i64 - 64 + k;
                if idx >= 0 && (idx as usize) < n {
                    gf[idx as usize][field]
                } else {
                    0.0
                }
            })
            .collect()
    };
    let arr0 = build(0);
    let arr1 = build(3);
    let arr2 = build(1);
    let s = CS_WEIGHTS[0] * difference_sum(&arr1, 0.8, 32, 32, 1)
        + CS_WEIGHTS[1] * difference_sum(&arr1, 0.8, 16, 16, 1)
        + CS_WEIGHTS[2] * difference_sum(&arr1, 0.9, 8, 8, 1)
        + CS_WEIGHTS[3] * difference_sum(&arr0, 0.4, 32, 4, 1)
        + CS_WEIGHTS[4] * difference_sum(&arr2, 0.4, 32, 4, 1);
    let rise = CS_RISE_W[0] * difference_sum(&arr1, -1.0, 32, 32, 32)
        + CS_RISE_W[1] * difference_sum(&arr1, -1.0, 16, 16, 16)
        + CS_RISE_W[2] * difference_sum(&arr1, -1.0, 8, 8, 8);
    s + rise.max(0.0)
}

/// DetectLikelyCuePointsV3 (parte local): scores por beat.
/// Array de LARGO COMPLETO (n), ceros fuera de [max(32,64), min(n-32,n-64)).
/// Devuelve None si hay señal insuficiente (<48 beats o sin envolvente).
pub fn detect_pre_analysis(beats: &[f64], ch1: &[f32], ch2: &[f32]) -> Option<Vec<f64>> {
    if ch1.is_empty() || beats.len() < 48 {
        return None;
    }
    let n = beats.len();
    let start = 32usize;
    let i1 = (n - 32).max(start + 1);
    let med = if beats.len() > 64 {
        let mut ds: Vec<f64> = beats[..64].windows(2).map(|w| w[1] - w[0]).collect();
        ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ds[ds.len() / 2]
    } else {
        0.5
    };
    let gf: Vec<[f64; 6]> = (0..n)
        .map(|i| {
            let t1 = if i + 1 < n {
                beats[i + 1]
            } else {
                beats[n - 1] + med
            };
            granule_features(beats[i], t1, ch1, ch2)
        })
        .collect();
    let mut scores = vec![0.0f64; n];
    let lo = start.max(64);
    let hi = i1.min(n - 64);
    for i in lo..hi {
        scores[i] = calculate_score(&gf, i);
    }
    Some(scores)
}

pub fn phrase_weight(x: i64) -> f64 {
    let w = &W_PHRASE;
    let d16 = (x % 16).min(16 - x % 16) as f64;
    let c16 = (d16 / 16.0).clamp(0.0, 1.0);
    let d32 = (x % 32).min(32 - x % 32) as f64;
    let c32 = (d32 / 8.0).clamp(0.0, 1.0);
    let d8 = (x % 8).min(8 - x % 8) as f64;
    let c8 = (d8 / 4.0).clamp(0.0, 1.0);
    let d4 = (x % 4).min(4 - x % 4) as f64;
    let c4 = (d4 / 2.0).clamp(0.0, 1.0);
    let par = x & 1;
    (1.0 + c16 * (w[0] - 1.0))
        * (1.0 + c32 * (w[1] - 1.0))
        * (1.0 + c8 * (w[2] - 1.0))
        * (1.0 + c4 * (w[3] - 1.0))
        * (if par == 1 { w[4] } else { 1.0 })
}

/// Clamp del primer cue al primer beat >= 0 (fix del cue negativo).
///
/// CAUSA: nuestro beatgrid arranca en t0 ∈ (−P, 0] y cue[0] = inicio del
/// primer segmento = beats[0] < 0 → POSITION_MARK Start<0 en rekordbox.xml
/// (la importación puede rechazarse). herramienta de referencia SIEMPRE emite cues >= 0.339 s en
/// el GT (su primer cue = su primer beat >= 0). Si no existe ningún beat
/// >= 0, se descartan los cues negativos.
pub fn clamp_first_cue_nonneg(cues: &[(f64, i32)], beats: &[f64]) -> Vec<(f64, i32)> {
    if cues.is_empty() {
        return Vec::new();
    }
    let k0 = beats.partition_point(|&b| b < 0.0);
    let mut out: Vec<(f64, i32)> = Vec::with_capacity(cues.len());
    if k0 < beats.len() {
        out.push((beats[k0], cues[0].1));
    }
    for &(t, l) in &cues[1..] {
        if t < 0.0 {
            continue; // ningún cue negativo sobrevive al clamp
        }
        if k0 < beats.len() && (t - beats[k0]).abs() < 1e-9 {
            continue; // dedupe: ya está el cue ancla
        }
        out.push((t, l));
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

// ------------------------------------------------ fase de onsets (cues) --
//
// Diagnóstico medido (docs/CUE-PARITY-DIAGNOSIS.md §5 + dumps 2026-11-19):
// el 48% de las cues cubiertas cae a +0.70 beats de herramienta de referencia (pistas cuyo t0
// apenas es negativo y cuya fase de onsets es +0.25: el "primer beat >= 0"
// salta un beat ENTERO y la rejilla de fills anclada hereda +0.75), otro
// 15% cae a −0.25 (solo fase sub-beat). La fase de onsets se estima con
// la envolvente propia de beats.rs (beats::onset_phase_offset — el canal
// ch1 de la envolvente herramienta de referencia difumina el argmax por su smoothing y NO
// sirve: medido, degradaba el harness). Las correcciones de abajo la
// consumen SOLO en la selección de cues: el beatgrid exportado
// (BPM/XML/serato) no se toca — una corrección global de fase arriesgaría
// los exports y la geometría de waveform ya validada.

/// Tolerancia del salto de ancla: el "primer beat corregido >= 0" a la
/// distancia tol de la línea absorbe el error del estimador de fase (±0.05)
/// y el sesgo del ranker v4 a anclar fills al beat siguiente (medido: los
/// 3 pares fill a +0.8 se pierden sin esta histéresis).
pub const ANCHOR_TOL: f64 = 0.1;

/// Shift entero del ancla: cuántos beats se mueve el "primer beat >= 0"
/// cuando el grid se corrige por la fase de onsets φ.
///
/// Con t0 ∈ (−P, 0] y fase φ > 0 (onsets DESPUÉS de la línea de beat), el
/// primer punto corregido >= 0 puede ser un beat ANTES que el ancla actual:
/// es la familia +0.75 del diagnóstico — toda la rejilla de fills de 16
/// queda un beat tarde. Simétrico (+1) para φ < 0 cuando el ancla corregida
/// cruza el siguiente entero.
pub fn anchor_shift_beats(t0: f64, period: f64, phase: f64) -> i64 {
    if period <= 0.0 {
        return 0;
    }
    let u = -t0 / period;
    let k0 = u.ceil();
    if phase > 0.0 && u - phase - ANCHOR_TOL <= k0 - 1.0 {
        -1
    } else if phase < 0.0 && (u - phase - 1e-9).ceil() > k0 {
        1
    } else {
        ((u - phase - 1e-9).ceil() - k0) as i64
    }
}

/// Re-ancla los cues de la REJILLA (ancla + fills a múltiplos de
/// CAND_FILL_STEP del ancla) al beat que dicta la fase corregida. Los
/// boundaries (inicios de segmento propios, ya cuantizados al grid) NO se
/// tocan: el censo de pares (sm_shots/cues_pair_census.py) muestra que el
/// shift entero blanket rompía exactamente los boundaries bien cubiertos a
/// −0.25. El primer cue se clampa a >= 0 tras el shift.
pub fn anchor_prev_cues(cues: &[(f64, i32)], beats: &[f64], phase: f64) -> Vec<(f64, i32)> {
    if cues.is_empty() || beats.len() < 2 {
        return cues.to_vec();
    }
    let period = beats[1] - beats[0];
    let t0 = beats[0];
    let shift = anchor_shift_beats(t0, period, phase);
    if shift == 0 {
        return cues.to_vec();
    }
    let anchor_i = beats.partition_point(|&b| b < 0.0).min(beats.len() - 1) as i64;
    let step = crate::cue_ranker::CAND_FILL_STEP as i64;
    let mut out: Vec<(f64, i32)> = cues
        .iter()
        .map(|&(t, lvl)| {
            let k = ((t - t0) / period).round() as i64;
            let on_grid = k == anchor_i || (k > anchor_i && (k - anchor_i) % step == 0);
            let k = if on_grid { k + shift } else { k };
            let idx = (k.max(0) as usize).min(beats.len() - 1);
            (beats[idx], lvl)
        })
        .collect();
    if out[0].0 < 0.0 {
        out[0].0 = 0.0;
    }
    out
}

/// Snap sub-beat de fase: recoloca cada cue en beats[k] + φ·P (la línea de
/// beat FASE-CORREGIDA, donde el audio tiene sus ataques). Aplicar SOLO en
/// selección de cues (ver nota de módulo). φ = 0 → salida idéntica.
pub fn phase_snap_cues(cues: &[(f64, i32)], beats: &[f64], phase: f64) -> Vec<(f64, i32)> {
    if cues.is_empty() || beats.len() < 2 || phase == 0.0 {
        return cues.to_vec();
    }
    let period = beats[1] - beats[0];
    let t0 = beats[0];
    let mut out: Vec<(f64, i32)> = cues
        .iter()
        .map(|&(t, lvl)| {
            let k = ((t - t0) / period).round() as i64;
            let mut tt = t0 + k as f64 * period + phase * period;
            if tt < 0.0 {
                tt = 0.0;
            }
            (tt, lvl)
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Selección de cues con la regla LR (ref_cues.cue_points con scores).
/// segs: (start, end, level). beats: tiempos de beat.
pub fn cue_points(
    segs: &[(f64, f64, i32)],
    beats: &[f64],
    scores: Option<&[f64]>,
    max_cues: usize,
    min_gap_beats: usize,
    fill_every: usize,
) -> Vec<(f64, i32)> {
    if segs.is_empty() || beats.is_empty() {
        return Vec::new();
    }
    let bt = if beats.len() > 64 {
        let mut ds: Vec<f64> = beats[..64].windows(2).map(|w| w[1] - w[0]).collect();
        ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ds[ds.len() / 2]
    } else {
        0.5
    };
    let anchor = beats.partition_point(|&b| b < segs[0].0) as i64;
    let dur = beats[beats.len() - 1];
    let smax = scores
        .map(|s| {
            let m = s.iter().cloned().fold(0.0f64, f64::max);
            if m > 0.0 {
                m
            } else {
                1.0
            }
        })
        .unwrap_or(1.0);

    let mut cands: Vec<(f64, bool, f64)> = vec![(segs[0].0, true, 0.0)];
    for i in 1..segs.len() {
        let dlev = (segs[i].2 - segs[i - 1].2).abs() as f64;
        cands.push((segs[i].0, true, dlev));
    }
    for &(ss, se, _) in segs {
        let mut k = fill_every;
        while ss + k as f64 * bt < se - 0.05 {
            cands.push((ss + k as f64 * bt, false, 0.0));
            k += fill_every;
        }
    }

    let mut scored: Vec<(f64, f64)> = cands[1..]
        .iter()
        .map(|&(ct, isb, dlev)| {
            let pr = if let Some(sc) = scores {
                let bi = beats.partition_point(|&b| b < ct).min(beats.len() - 1);
                let sn = if smax > 0.0 {
                    (sc[bi] / smax).min(20.0)
                } else {
                    0.0
                };
                let ph = phrase_weight((bi as i64 - anchor) % 32);
                let f = [if isb { 1.0 } else { 0.0 }, dlev, ct / dur, sn, ph, sn * ph];
                LR_INTERCEPT
                    + LR_COEF
                        .iter()
                        .zip(f.iter())
                        .map(|(a, b)| a * b)
                        .sum::<f64>()
            } else {
                (if isb { 2.0 + dlev } else { 1.0 }) - 0.001 * (ct / dur)
            };
            (pr, ct)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut chosen: Vec<f64> = vec![segs[0].0];
    for (_, ct) in scored {
        if chosen.len() >= max_cues {
            break;
        }
        if chosen
            .iter()
            .all(|c| (ct - c).abs() >= min_gap_beats as f64 * bt - 1e-9)
        {
            chosen.push(ct);
        }
    }
    chosen.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    chosen
        .into_iter()
        .map(|t| {
            let bi = beats
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (**a - t)
                        .abs()
                        .partial_cmp(&((**b - t).abs()))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            let lvl = segs
                .iter()
                .find(|&&(ss, se, _)| ss - 1e-6 <= t && t < se)
                .map(|&(_, _, l)| l)
                .unwrap_or(5);
            (beats[bi], lvl)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------- fase de onsets / anclaje --

    fn grid(t0: f64, period: f64, n: usize) -> Vec<f64> {
        (0..n).map(|k| t0 + k as f64 * period).collect()
    }

    #[test]
    fn anchor_shift_tabla_de_casos() {
        // (t0, P, fase, shift esperado) — u = −t0/P, tol = ANCHOR_TOL
        let cases: &[(f64, f64, f64, i64)] = &[
            (-0.05, 0.5, 0.25, -1),  // u=0.10: salto de ancla (familia +0.75)
            (-0.125, 0.5, 0.25, -1), // u = fase exacta (borde)
            (-0.11, 0.5, 0.169, -1), // u=0.22: solo entra por la tol (histéresis)
            (-0.30, 0.5, 0.25, 0),   // u=0.60 > fase+tol
            (-0.80, 1.0, -0.30, 1),  // u=0.80, u−fase=1.10 → ancla corr. +1
            (0.20, 0.5, 0.25, 0),    // t0 >= 0: ancla en beat 0
            (-0.50, 0.5, 0.0, 0),    // fase nula: inmutable
        ];
        for &(t0, p, ph, exp) in cases {
            let got = anchor_shift_beats(t0, p, ph);
            assert_eq!(
                got, exp,
                "t0={t0} P={p} fase={ph}: esperado {exp}, salió {got}"
            );
        }
    }

    #[test]
    fn anchor_prev_cues_mueve_solo_la_rejilla() {
        // familia +0.75: t0 apenas negativo + fase +0.25 → shift −1 en
        // ancla + fills (múltiplos de 16); el boundary NO se toca
        let beats = grid(-0.05, 0.5, 200);
        // beats[1]=ancla, [33]=ancla+32 (fill), [65]=ancla+64 (fill),
        // [40]=ancla+39 (boundary: no múltiplo de 16)
        let cues = vec![(0.45, 4), (16.45, 5), (19.95, 6), (32.45, 7)];
        let out = anchor_prev_cues(&cues, &beats, 0.25);
        assert_eq!(out.len(), 4);
        assert!(out[0].0 >= 0.0, "cue[0] debe clamparse a >= 0: {:?}", out);
        assert_eq!(out[0].0, 0.0, "cue[0] = beats[0] < 0 clampeado: {out:?}");
        assert!((out[1].0 - 15.95).abs() < 1e-9, "fill beats[32]: {out:?}");
        assert!(
            (out[2].0 - 19.95).abs() < 1e-9,
            "boundary inmutable: {out:?}"
        );
        assert!((out[3].0 - 31.95).abs() < 1e-9, "fill beats[64]: {out:?}");
        assert_eq!(out[0].1, 4);
        assert_eq!(out[2].1, 6);
    }

    #[test]
    fn anchor_prev_cues_fase_cero_inmutable() {
        let beats = grid(-0.30, 0.5, 200);
        let cues = vec![(0.20, 4), (16.20, 5)];
        assert_eq!(anchor_prev_cues(&cues, &beats, 0.0), cues);
    }

    #[test]
    fn phase_snap_cues_suma_fase_sub_beat() {
        let beats = grid(-0.05, 0.5, 200);
        let cues = vec![(0.45, 4), (16.45, 5)];
        let out = phase_snap_cues(&cues, &beats, 0.25);
        // beats[1] + 0.25·P = 0.45 + 0.125 ; beats[33] + 0.125
        assert!((out[0].0 - 0.575).abs() < 1e-9, "{out:?}");
        assert!((out[1].0 - 16.575).abs() < 1e-9, "{out:?}");
        assert_eq!(out[0].1, 4);
        // fase 0 → idéntico
        assert_eq!(phase_snap_cues(&cues, &beats, 0.0), cues);
    }

    #[test]
    fn phase_snap_cue_negativo_se_clampa() {
        let beats = grid(-0.02, 0.5, 200);
        let cues = vec![(0.02, 4)];
        let out = phase_snap_cues(&cues, &beats, -0.25);
        assert_eq!(out[0].0, 0.0, "cue bajo 0 tras el snap se clampa: {out:?}");
        assert_eq!(out[0].1, 4);
    }

    #[test]
    fn clamp_first_cue_beatgrid_con_beat0_negativo() {
        // beatgrid sintético 120 bpm con t0 = -0.284 (beat 0 negativo, como
        // emite beats::analyze_pcm): el primer cue sale -0.284 → debe
        // clamearse al primer beat >= 0 (0.216) sin tocar el resto
        let t0 = -0.284;
        let p = 0.5;
        let beats: Vec<f64> = (0..200).map(|k| t0 + k as f64 * p).collect();
        let cues = vec![(-0.284, 4), (15.716, 5), (47.716, 6)];
        let out = clamp_first_cue_nonneg(&cues, &beats);
        assert!(
            out.iter().all(|&(t, _)| t >= 0.0),
            "cue negativo sobrevivió: {out:?}"
        );
        assert_eq!(out.len(), 3);
        assert!(
            (out[0].0 - 0.216).abs() < 1e-9,
            "cue[0] != primer beat >= 0: {out:?}"
        );
        assert_eq!(out[0].1, 4, "nivel del primer cue se pierde");
        assert!((out[1].0 - 15.716).abs() < 1e-9 && (out[2].0 - 47.716).abs() < 1e-9);
    }

    #[test]
    fn clamp_first_cue_sin_beat_positivo_descarta() {
        // grid degenerado todo negativo: el cue negativo se descarta
        let beats: Vec<f64> = (-10..0).map(|k| k as f64 * 0.5).collect();
        let cues = vec![(-1.0, 4), (-0.2, 5)];
        let out = clamp_first_cue_nonneg(&cues, &beats);
        assert!(out.is_empty(), "esperado vacío, salió {out:?}");
        // y un cue posterior positivo se conserva
        let cues2 = vec![(-1.0, 4), (5.0, 5)];
        let out2 = clamp_first_cue_nonneg(&cues2, &beats);
        assert_eq!(out2, vec![(5.0, 5)]);
    }

    #[test]
    fn clamp_first_cue_dedup_ya_snapeado() {
        // si un cue posterior ya está en el beat ancla, no se duplica
        let t0 = -0.3;
        let beats: Vec<f64> = (0..100).map(|k| t0 + k as f64 * 0.5).collect();
        let cues = vec![(-0.3, 4), (0.2, 6)]; // 0.2 = beats[1] (primer beat >= 0)
        let out = clamp_first_cue_nonneg(&cues, &beats);
        assert_eq!(out.len(), 1);
        assert!((out[0].0 - 0.2).abs() < 1e-9);
    }

    #[test]
    fn clamp_first_cue_no_negativo_inmutable() {
        // cues ya válidos: salida idéntica
        let beats: Vec<f64> = (0..100).map(|k| 0.34 + k as f64 * 0.5).collect();
        let cues = vec![(0.34, 4), (16.34, 5)];
        let out = clamp_first_cue_nonneg(&cues, &beats);
        assert_eq!(out, cues);
    }

    #[test]
    fn sine_envelope_nonzero() {
        // 5 s de seno 1 kHz amplitud 0.5 → envolvente claramente > 0
        let n = 220_500;
        let mono: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 44100.0).sin() * 0.5)
            .collect();
        let (ch1, ch2) = envelope_exact(&mono);
        let m1 = ch1.iter().cloned().fold(0.0f32, f32::max);
        let m2 = ch2.iter().cloned().fold(0.0f32, f32::max);
        eprintln!("sine: n={} max_ch1={} max_ch2={}", ch1.len(), m1, m2);
        assert!(m1 > 0.0, "ch1 todo cero");
        assert!(m2 > 0.0, "ch2 todo cero");
    }

    // ------------------------------------------------ T7: pyramid vs Python
    // Caracterización (fix pyramid 2026-11-17): los valores esperados son
    // bit-exactos los que produce `_pyramid` de harmonic-engine
    // (ref_waveform_env.py) sobre los mismos inputs deterministas — la
    // referencia Python fue validada EXACTA contra el blob ZWAVEFORM de
    // herramienta de referencia. Inputs reproducibles sin RNG compartida: fórmulas + xorshift32.
    fn t7_from_hex(hex: &str) -> Vec<f32> {
        // el hex es el dump de BYTES little-endian (np .tobytes().hex()):
        // cada float son 4 pares hex en orden LSB→MSB, no el u32 literal.
        (0..hex.len() / 8)
            .map(|i| {
                let s = &hex[i * 8..i * 8 + 8];
                let b = |k: usize| u8::from_str_radix(&s[k * 2..k * 2 + 2], 16).unwrap();
                f32::from_bits(
                    (b(0) as u32)
                        | ((b(1) as u32) << 8)
                        | ((b(2) as u32) << 16)
                        | ((b(3) as u32) << 24),
                )
            })
            .collect()
    }

    fn t7_inputs() -> Vec<(&'static str, Vec<f32>)> {
        // impulso: piso 1e-4, deltas en 100 y 300
        let mut imp = vec![1e-4f32; 767];
        imp[100] = 1.0;
        imp[300] = 8.0;
        imp[0] = 0.0;
        // rampa: v[k] = k * 0.01 (f32), v[0] = 0
        let mut ramp = vec![0.0f32; 767];
        for k in 1..767 {
            ramp[k] = k as f32 * 0.01;
        }
        // pseudo-ruido xorshift32 seed 0x9E3779B9: v[k] = ((s>>8)%1000)/100
        let mut noise = vec![0.0f32; 767];
        let mut s: u32 = 0x9E37_79B9;
        for k in 1..767 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            noise[k] = ((s >> 8) % 1000) as f32 / 100.0;
        }
        vec![("impulse", imp), ("ramp", ramp), ("noise_x", noise)]
    }

    #[test]
    fn pyramid_matches_python_reference() {
        let expected: &[(&str, &str)] = &[
            ("impulse", "0000000017b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d1380ad7233c17b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d1389a6fdc3a17b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d13817b7d138"),
            ("ramp", "000000000ad7233c0ad7a33c8fc2f53c0ad7233dcccc4c3d8fc2753d295c8f3d0ad7a33deb51b83dcccccc3dae47e13d8fc2f53db81e053e295c0f3e9999193e0ad7233e7b142e3eeb51383e5c8f423ecccc4c3e3d0a573eae47613e1e856b3e8fc2753e0000803eb81e853e703d8a3e295c8f3ee17a943e9999993e52b89e3e0ad7a33ec2f5a83e7b14ae3e3333b33eeb51b83ea470bd3e5c8fc23e14aec73ecccccc3e85ebd13e3d0ad73ef528dc3eae47e13e6666e63e1e85eb3ed7a3f03e8fc2f53e47e1fa3e0000003f5c8f023fb81e053f14ae073f703d0a3fcdcc0c3f295c0f3f85eb113fe17a143f3d0a173f9999193ff6281c3f52b81e3fae47213f0ad7233f6666263fc2f5283f1e852b3f7b142e3fd7a3303f3333333f8fc2353feb51383f47e13a3fa4703d3f0000403f5c8f423fb81e453f14ae473f703d4a3f76134e3f3532533ff350583fb16f5d3f6f8e623f2cad673feacb6c3fa7ea713f6409773f20287c3f6fa3803fcd32833f2bc2853f8951883fe7e08a3f45708d3fa3ff8f3f018f923f5e1e953fbbad973f193d9a3f78cc9c3fd55b9f3f32eba13f8f7aa43fed09a73f4a99a93fa728ac3f05b8ae3f6247b13fc0d6b33f1c66b63f79f5b83fd684bb3f3414be3f91a3c03fee32c33f4bc2c53fa851c83f05e1ca3f0cb7ce3fcdd5d33f8df4d83f4c13de3f0c32e33fca50e83f896fed3f478ef23f05adf73fc3cbfc3f40f500409f840340fe1306405ca30840ba320b4019c20d4077511040d6e012403370154091ff1740ef8e1a404d1e1d40acad1f40093d224066cc2440c45b274022eb2940807a2c40dd092f403b99314099283440f5b7364052473940b1d63b400e663e406bf54040c88443402614464083a34840e0324b40e9084f40aa2754406946594029655e40e9836340a8a2684066c16d4025e07240e3fe7740a11d7d402f1e81408ead8340ed3c86404ccc8840aa5b8b4009eb8d40677a9040c60993402399954082289840e0b79a403e479d409bd69f40f965a24058f5a440b584a7401314aa4071a3ac40cf32af402cc2b1408a51b440e8e0b6404470b940a2ffbb40ff8ebe405c1ec140baadc340183dc64075ccc840d25bcb40db31cf409b50d4405b6fd9401c8ede40dbace3409bcbe84059eaed40"),
            ("noise_x", "000000005c8fb2400000144185eb89409a99194066668e4052b8ee408fc20540cdccb4408fc2e54085ebb13f295c1341ec51f84052b81e41ec51d83ff628dc40713dba405c8f9240713d0a417b146e40ae47a9400000803f3d0a9f40cdcc0441713d1a411f857b409a99014148e1d2401f854b40e17aac40f62804413333c340ae47d940ec51383e33338b40e17abc40666676405c8f0241ec51084085eb5140f6281c40713dda407b14be407b148640c3f5c04014ae2740b81e254066661640f628bc3f1f850f410000d840b81e25409a990d411f850b410000304000003040a47019416666b64000001040d7a3e84052b82e4014ae7740ec5118408fc2954052b85e400ad713403d0a6740a4707d4052b8a640c3f5b8406666664014ae174185ebd940e17a5440713dd240a470fd400000803f0ad7ab40f628dc3f666646404b8ed3408ae69e3f20e3a53fcb97a8409a670541e6d64f40e6a1a84069ff47401410334026003d406ca28640d50765407f6f7a40e2f0993e9e4586404c1bf5404ef460403310964093d25d40544a9d40f0321941756311402d8ad93fe1050841461a4940175ab740ac153140e386f43f5fad394067cca44039ad75407a00093fab278440dac3664069260341480f51404b340840f3b6ac40d801e73f5239a24065d8af40fbc48440559a6a40828f704003fd3740f4af87401c034240795a924094df89402ca1894071f3ca40b7f21c4000e4a7407e54ef409b93c4405deda0400f957440d96aaa40014b61409a2d0f406657a6402de4384015d7c540fb90123f6b2fbb40ae329240d8c2f840fce164401426fd3fdec9e73f3e581440d1551c404b7d9740e3a2a840c292d640c248fa3fac87fb3fd9677140d8ed914036d0eb3f1e19494046f595409a22574039c0244086989e4067334640a3352a40783b7740618ac040c9ad734056d38d408c809a404267ac40d56e7340e9ce88403504b140b0acb140b92b6f405296853fdb9ae8409a4e4a404b468340c5c057403f2caa40923dc540f6ce8740b5bb98403a7b80404c7dcc40bcb9c240b6ac4940517a6d40d27a87406f7ba4403eb97f400d44b4403ea5484090b25740e3232b40cc4f414077d96640730f2640251677405a1f4f40aac359402e0d6e4070282e40"),
        ];
        for (name, input) in t7_inputs() {
            let exp = t7_from_hex(expected.iter().find(|(n, _)| *n == name).unwrap().1);
            let got = pyramid(input);
            assert_eq!(got.len(), exp.len(), "{name}: largo");
            let bad = got
                .iter()
                .zip(exp.iter())
                .enumerate()
                .find(|(_i, (a, b))| a.to_bits() != b.to_bits())
                .map(|(i, (a, b))| (i, *a, *b));
            if let Some((i, a, b)) = bad {
                panic!("{name}: banda {i} rust={a:.7e} py={b:.7e} (primera divergencia)");
            }
        }
    }
}
