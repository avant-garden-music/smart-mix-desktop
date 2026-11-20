//! Volumen/LUFS/waveform — port de `harmonic-engine/engine/ref_volume.py`.
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
///
/// En streaming desde la auditoría 2026-11-10: filtra y acumula los
/// bloques de 400 ms (hop 100 ms) sin materializar el PCM filtrado —la
/// versión anterior alocaba 2 buffers f64 del tamaño completo de la pista
/// por canal (~128 MB/pista a 3 min estéreo). El recorrido por muestra,
/// el orden de acumulación de cada suma y el ensamblado `p` por canal son
/// los mismos → bit-exacto (ver `characterization_lufs_bits`).
pub fn lufs_integrated(interleaved: &[f32], channels: usize, sr: u32) -> f64 {
    let nch = channels.min(2).max(1);
    let frames = interleaved.len() / channels.max(1);
    let (s1, s2) = k_filters(sr as f64);
    // estado de filtros por canal (antes: copia del PCM completo por canal)
    let mut states: Vec<(Biquad, Biquad)> = (0..nch).map(|_| (s1.clone(), s2.clone())).collect();
    let weights = [1.0f64, 1.0f64];
    let blk = (0.4 * sr as f64) as usize;
    let hop = (0.1 * sr as f64) as usize;
    let mut powers: Vec<f64> = Vec::new();
    let n = frames;
    if n < blk {
        return -70.0;
    }
    // bloques deslizantes: hop = blk/4 → máx. 4 acumuladores vivos.
    // Cada acumulador suma y² en orden ascendente de frame, igual que el
    // `slice.sum()` del materializado; se abre uno por inicio s = k·hop
    // con s + blk <= n (el mismo conjunto de bloques que el while original).
    let mut acc: Vec<(usize, [f64; 2])> = Vec::with_capacity(blk / hop + 1);
    for i in 0..n {
        if i % hop == 0 && i + blk <= n {
            acc.push((i, [0.0; 2]));
        }
        for c in 0..nch {
            let x = interleaved[i * channels + c] as f64;
            let y = {
                let st = &mut states[c];
                st.1.process(st.0.process(x))
            };
            for (_, a) in acc.iter_mut() {
                a[c] += y * y;
            }
        }
        // cerrar los bloques que terminan en este frame
        let mut k = 0;
        while k < acc.len() {
            if i + 1 == acc[k].0 + blk {
                let a = acc[k].1;
                let mut p = 0.0;
                for c in 0..nch {
                    p += weights[c] * a[c] / blk as f64;
                }
                powers.push(p);
                acc.remove(k);
            } else {
                k += 1;
            }
        }
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
///
/// Sin gathers por bloque desde la auditoría 2026-11-10 (antes: 2 Vec por
/// bloque = ~15k allocs por pista de 3 min). Los mismos índices se
/// recorren en el mismo orden (L = muestras pares, R = impares si
/// channels >= 2; ambas = señal completa en mono) y las sumas son f32
/// secuenciales → bit-exacto (ver `characterization_waveform_and_bands_bits`).
pub fn waveform_records(interleaved: &[f32], channels: usize, block: usize) -> Vec<[f32; 6]> {
    let n = interleaved.len() / block * block;
    let mut recs = Vec::with_capacity(n / block);
    for s in (0..n).step_by(block) {
        let blk = &interleaved[s..s + block];
        // [canal][abs_sum, peak, sq_sum] — L=0, R=1
        let mut st = [[0.0f32, 0.0f32, 0.0f32]; 2];
        let (len_l, len_r): (usize, usize);
        if channels >= 2 {
            // == len de los gathers step_by(2) / skip(1).step_by(2)
            len_l = (block + 1) / 2;
            len_r = block / 2;
            for (j, &x) in blk.iter().enumerate() {
                let c = j & 1;
                let a = x.abs();
                st[c][0] += a;
                if a > st[c][1] {
                    st[c][1] = a;
                }
                st[c][2] += x * x;
            }
        } else {
            len_l = block;
            len_r = block;
            for &x in blk {
                let a = x.abs();
                let sq = x * x;
                for c in 0..2 {
                    st[c][0] += a;
                    if a > st[c][1] {
                        st[c][1] = a;
                    }
                    st[c][2] += sq;
                }
            }
        }
        let mean = |t: &[f32; 3], len: usize| t[0] / len as f32;
        let peak = |t: &[f32; 3]| t[1];
        let rms = |t: &[f32; 3], len: usize| (t[2] / len as f32).sqrt();
        recs.push([
            mean(&st[0], len_l),
            peak(&st[0]),
            mean(&st[1], len_r),
            peak(&st[1]),
            rms(&st[0], len_l),
            rms(&st[1], len_r),
        ]);
    }
    recs
}

/// Envelope de 3 bandas (bajo/medio/alto) para el waveform espectral.
///
/// Crossover de 3 vías con filtros one-pole: low = LP(250 Hz),
/// high = señal − LP(4 kHz), mid = LP(4 kHz) − low. Por bucket devuelve el
/// RMS de cada banda (buckets × 3: low, mid, high) sobre el stream
/// **interleaved completo (L,R,L,R,…)**, NO sobre el canal L solo —
/// así corre desde la primera versión y así está persistido en
/// `waveform_bands`; un cambio a un solo canal alteraría los BLOBs y el
/// render (propuesta B1 del audit 2026-11-10, pendiente de decisión).
pub fn band_envelope(
    interleaved: &[f32],
    channels: usize,
    buckets: usize,
    sample_rate: u32,
) -> Vec<f32> {
    let ch = channels.max(1);
    let n = interleaved.len() / ch * ch;
    // coeficientes one-pole (lineal en amplitud)
    let alpha = |f: f64| -> f64 {
        (-2.0 * std::f64::consts::PI * f / sample_rate as f64).exp()
    };
    let a_lo = alpha(250.0);
    let a_hi = alpha(4000.0);
    let mut lp_lo = 0.0f32;
    let mut lp_hi = 0.0f32;
    let per = (n / ch) as f64 / buckets.max(1) as f64;
    let mut out = Vec::with_capacity(buckets * 3);
    let mut acc = [0.0f64; 3];
    let mut cnt = 0usize;
    let mut target = per;
    for i in 0..n {
        let x = interleaved[i]; // canal L (canal 0 de cada frame)
        lp_lo = a_lo as f32 * lp_lo + (1.0 - a_lo as f32) * x;
        lp_hi = a_hi as f32 * lp_hi + (1.0 - a_hi as f32) * x;
        let low = lp_lo;
        let mid = lp_hi - lp_lo;
        let high = x - lp_hi;
        acc[0] += (low * low) as f64;
        acc[1] += (mid * mid) as f64;
        acc[2] += (high * high) as f64;
        cnt += 1;
        if (i / ch) as f64 >= target {
            for k in 0..3 {
                out.push((acc[k] / cnt.max(1) as f64).sqrt() as f32);
                acc[k] = 0.0;
            }
            cnt = 0;
            target += per;
        }
    }
    while out.len() < buckets * 3 {
        out.push(0.0);
    }
    out.truncate(buckets * 3);
    out
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

    // ------------------------------------------------ characterization ----
    // Valores dorados bit-exactos generados con la implementación vigente
    // (auditoría 2026-11-10, example audit_golden). Cualquier cambio de
    // precisión/orden de acumulación en estas funciones rompe el test.

    struct Lcg(u64);
    impl Lcg {
        fn next_f32(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
        }
    }

    fn fnv1a(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn hash_f32(v: &[f32]) -> u64 {
        let mut bytes = Vec::with_capacity(v.len() * 4);
        for x in v {
            bytes.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        fnv1a(&bytes)
    }

    fn hash_rec(recs: &[[f32; 6]]) -> u64 {
        let mut bytes = Vec::with_capacity(recs.len() * 24);
        for r in recs {
            for x in r {
                bytes.extend_from_slice(&x.to_bits().to_le_bytes());
            }
        }
        fnv1a(&bytes)
    }

    /// Señal A: seno estéreo 997 Hz −6 dBFS con L = −R, 3 s.
    fn signal_a() -> Vec<f32> {
        let sr = 44_100usize;
        let amp = 10f64.powf(-6.0 / 20.0);
        let mut a = Vec::with_capacity(3 * sr * 2);
        for i in 0..3 * sr {
            let v = (2.0 * std::f64::consts::PI * 997.0 * i as f64 / sr as f64).sin() * amp;
            a.push(v as f32);
            a.push((-v) as f32);
        }
        a
    }

    #[test]
    fn characterization_lufs_bits() {
        let lufs_a = lufs_integrated(&signal_a(), 2, 44_100);
        assert_eq!(lufs_a.to_bits(), 0xc0182d5d4bd25213);
        // ruido mono LCG 2 s
        let mut lcg = Lcg(0xDEADBEEF);
        let b: Vec<f32> = (0..2 * 44_100).map(|_| lcg.next_f32() * 0.5).collect();
        let lufs_b = lufs_integrated(&b, 1, 44_100);
        assert_eq!(lufs_b.to_bits(), 0xc02b75baa8f54f1e);
        // corta (< bloque 400 ms) → exactamente −70.0
        let c: Vec<f32> = (0..(44_100 / 4) * 2)
            .map(|i| ((i as f32 * 0.01).sin() * 0.3) as f32)
            .collect();
        assert_eq!(lufs_integrated(&c, 2, 44_100).to_bits(), (-70.0f64).to_bits());
    }

    #[test]
    fn characterization_waveform_and_bands_bits() {
        let a = signal_a();
        let recs_a = waveform_records(&a, 2, 1024);
        assert_eq!(recs_a.len(), 258);
        assert_eq!(hash_rec(&recs_a), 0xf7ac0b9bb32e3bbd);
        assert_eq!(hash_f32(&display_waveform(&recs_a, 2048)), 0x94e771562a283127);
        assert_eq!(hash_f32(&band_envelope(&a, 2, 2048, 44_100)), 0xf4ea09a44dfe773e);
        let mut lcg = Lcg(0xDEADBEEF);
        let b: Vec<f32> = (0..2 * 44_100).map(|_| lcg.next_f32() * 0.5).collect();
        let recs_b = waveform_records(&b, 1, 1024);
        assert_eq!(recs_b.len(), 86);
        assert_eq!(hash_rec(&recs_b), 0xfa0b339723a15add);
        assert_eq!(hash_f32(&band_envelope(&b, 1, 2048, 44_100)), 0xf951c9180df0090f);
    }

    #[test]
    fn characterization_histogram_and_volume_bits() {
        let a = signal_a();
        let hist = volume_histogram(&a);
        let mut h = [0u64; BINS];
        h.copy_from_slice(&hist);
        assert_eq!(
            fnv1a(&h.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()),
            0x19c823f5d10a8d02
        );
        assert_eq!(volume_db(&h).to_bits(), 0xc01cd6520e0e75bb);
        assert_eq!(clipped_peak_count(&a), 0);
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
