//! cue_ranker — port de la selección de cues con el ranker HGB entrenado
//! (harmonic-engine training/cue_ranker.pkl, exportado bit-exacto a
//! models/cue_ranker.json por export_ranker.py).
//! candidate_features (50 features, idénticas al entrenamiento) +
//! evaluación de 700 árboles + decodificación DP exacta (_decode_ranked).

use serde::Deserialize;
use std::path::Path;

use crate::cue_engine::phrase_weight;

/// Paso de la rejilla de fills anclada (contrato compartido con la
/// corrección de anclaje de cues en cue_engine).
pub const CAND_FILL_STEP: usize = 16;

#[derive(Deserialize)]
pub struct Ranker {
    pub baseline: f64,
    #[allow(dead_code)]
    pub n_features: usize,
    /// [is_leaf, feature, thr, miss_left, left, right, value][]
    pub trees: Vec<Vec<[f64; 7]>>,
    pub threshold: f64,
    pub decode: String,
    pub max_cues: usize,
    pub min_gap_beats: usize,
}

pub struct RankerModel {
    r: Ranker,
}

#[derive(Deserialize)]
struct RawRanker {
    baseline: f64,
    n_features: usize,
    trees: Vec<Vec<Vec<f64>>>,
    threshold: f64,
    decode: String,
    max_cues: usize,
    min_gap_beats: usize,
}

impl RankerModel {
    pub fn load(path: &Path) -> Option<RankerModel> {
        let txt = std::fs::read_to_string(path).ok()?;
        let raw: RawRanker = serde_json::from_str(&txt).ok()?;
        let trees = raw
            .trees
            .iter()
            .map(|t| {
                t.iter()
                    .map(|n| {
                        let mut a = [0.0f64; 7];
                        for i in 0..7 {
                            a[i] = n[i];
                        }
                        a
                    })
                    .collect()
            })
            .collect();
        Some(RankerModel {
            r: Ranker {
                baseline: raw.baseline,
                n_features: raw.n_features,
                trees,
                threshold: raw.threshold,
                decode: raw.decode,
                max_cues: raw.max_cues,
                min_gap_beats: raw.min_gap_beats,
            },
        })
    }

    /// predict_proba clase 1 (evaluación exacta de los árboles).
    pub fn predict(&self, x: &[f64]) -> f64 {
        let mut raw = self.r.baseline;
        for nodes in &self.r.trees {
            let mut i = 0usize;
            while nodes[i][0] < 0.5 {
                let (_, feat, thr, miss_left, left, right, _) = (
                    nodes[i][0],
                    nodes[i][1] as usize,
                    nodes[i][2],
                    nodes[i][3],
                    nodes[i][4] as usize,
                    nodes[i][5] as usize,
                    nodes[i][6],
                );
                let v = x[feat];
                let go_left = if v.is_nan() {
                    miss_left > 0.5
                } else {
                    v <= thr
                };
                i = if go_left { left } else { right };
            }
            raw += nodes[i][6];
        }
        1.0 / (1.0 + (-raw).exp())
    }
}

struct Ctx {
    n_beats: usize,
    bt: f64,
    #[allow(dead_code)]
    smax: f64,
    sarr: Vec<f64>,
    cummax: Vec<f64>,
    cs: Vec<f64>,
    sm8: Vec<f64>,
    sm32: Vec<f64>,
    sm64: Vec<f64>,
}

impl Ctx {
    fn sc(&self, idx: i64) -> f64 {
        if self.sarr.is_empty() {
            return 0.0;
        }
        let i = idx.clamp(0, self.n_beats as i64 - 1) as usize;
        self.sarr[i]
    }

    fn wmean(&self, a: i64, b: i64) -> f64 {
        let a = a.max(0) as usize;
        let b = (b.min(self.sarr.len() as i64)) as usize;
        if b <= a {
            return 0.0;
        }
        (self.cs[b] - self.cs[a]) / ((b - a) as f64).max(1.0)
    }
}

fn hann_mean(sarr: &[f64], center: usize, n: usize) -> f64 {
    let a = center.saturating_sub(n);
    let b = (center + n + 1).min(sarr.len());
    if b <= a {
        return 0.0;
    }
    let nf = n as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in a..b {
        let w = (std::f64::consts::PI * i as f64 / (2.0 * nf)).cos().powi(2);
        num += sarr[i] * w;
        den += w;
    }
    num / den
}

/// Tabla de candidatos con 50 features (idéntica al entrenamiento).
/// Devuelve (X, bis, forced_bi).
#[allow(clippy::too_many_arguments)]
fn candidate_features(
    segs: &[(f64, f64, i32)],
    beats: &[f64],
    scores: &[f64],
    tempo: Option<f64>,
) -> (Vec<[f64; 50]>, Vec<i64>, i64) {
    let n_beats = beats.len();
    let bt = if n_beats > 64 {
        let mut ds: Vec<f64> = beats[..64].windows(2).map(|w| w[1] - w[0]).collect();
        ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ds[ds.len() / 2]
    } else {
        0.5
    };
    let smax0 = scores.iter().cloned().fold(0.0f64, f64::max);
    let smax = if smax0 > 0.0 { smax0 } else { 1.0 };
    let sarr: Vec<f64> = if scores.is_empty() {
        vec![0.0]
    } else {
        scores.iter().map(|s| s / smax).collect()
    };

    let nb = |t: f64| -> i64 {
        let mut best = 0i64;
        let mut bd = f64::MAX;
        for (i, b) in beats.iter().enumerate() {
            let d = (b - t).abs();
            if d < bd {
                bd = d;
                best = i as i64;
            }
        }
        best
    };

    // ancla = primer beat >= 0 (fix cue[0] negativo). nb(segs[0].0) caía
    // SIEMPRE en el beat 0 (t0 <= 0 del grid propio) desalineando
    // gap_anchor/mod32/phrase32 respecto del entrenamiento (donde el ancla
    // es el primer beat de herramienta de referencia, ~0.34 s > 0)
    let anchor = beats.partition_point(|&b| b < 0.0).min(beats.len() - 1) as i64;
    let n_segs = segs.len();
    let bstart: Vec<i64> = segs.iter().map(|s| nb(s.0)).collect();
    let bend: Vec<i64> = segs.iter().map(|s| nb(s.1)).collect();

    // candidatos: boundaries + rejilla GLOBAL de fills cada CAND_FILL_STEP
    // beats DESDE EL ANCLA (96% de los cues herramienta de referencia del GT caen en múltiplos de
    // 16 beats del ancla — CUEPOINTS_RE; los fills por segmento heredan la
    // deriva de nuestro inicio de segmento: cobertura de cues GT 39.8% vs
    // 76.1% con la rejilla anclada, medida en el diagnóstico de este cambio)
    let mut cand_bi: Vec<i64> = Vec::new();
    let mut cand_bound: Vec<u8> = Vec::new();
    let mut cand_first: Vec<u8> = Vec::new();
    let mut cand_off: Vec<i64> = Vec::new();
    let mut cand_seg: Vec<usize> = Vec::new();
    let mut have = std::collections::HashMap::<i64, usize>::new();
    for (i, &(ss, se, _)) in segs.iter().enumerate() {
        let _ = (ss, se);
        let bi0 = bstart[i];
        let e = have.entry(bi0).or_insert(cand_bi.len());
        if *e == cand_bi.len() {
            cand_bi.push(bi0);
            cand_bound.push(1);
            cand_first.push(if i == 0 { 1 } else { 0 });
            cand_off.push(0);
            cand_seg.push(i);
        } else if cand_bound[*e] == 0 {
            cand_bound[*e] = 1;
        }
        // rejilla anclada: puntos en (bi0, bend[i]]
        let mut m = ((bi0 - anchor) / CAND_FILL_STEP as i64 + 1).max(1);
        while anchor + m * CAND_FILL_STEP as i64 <= bend[i] {
            let bi = anchor + m * CAND_FILL_STEP as i64;
            if bi > bi0 && !have.contains_key(&bi) {
                have.insert(bi, cand_bi.len());
                cand_bi.push(bi);
                cand_bound.push(0);
                cand_first.push(if i == 0 { 1 } else { 0 });
                cand_off.push(bi - bi0);
                cand_seg.push(i);
            }
            m += 1;
        }
    }
    // ordenar por bi (mismo orden que python `sorted(cands)`)
    let mut order: Vec<usize> = (0..cand_bi.len()).collect();
    order.sort_by_key(|&i| cand_bi[i]);
    let bis: Vec<i64> = order.iter().map(|&i| cand_bi[i]).collect();
    let bound: Vec<u8> = order.iter().map(|&i| cand_bound[i]).collect();
    let first: Vec<u8> = order.iter().map(|&i| cand_first[i]).collect();
    let off: Vec<i64> = order.iter().map(|&i| cand_off[i]).collect();
    let segi: Vec<usize> = order.iter().map(|&i| cand_seg[i]).collect();

    // contexto
    let mut cummax = Vec::with_capacity(sarr.len());
    let mut m = 0.0f64;
    for v in &sarr {
        m = m.max(*v);
        cummax.push(m);
    }
    let mut cs = vec![0.0f64];
    for v in &sarr {
        cs.push(cs.last().unwrap() + v);
    }
    let sm8: Vec<f64> = (0..sarr.len()).map(|i| hann_mean(&sarr, i, 8)).collect();
    let sm32: Vec<f64> = (0..sarr.len()).map(|i| hann_mean(&sarr, i, 32)).collect();
    let sm64: Vec<f64> = (0..sarr.len()).map(|i| hann_mean(&sarr, i, 64)).collect();
    let ctx = Ctx {
        n_beats,
        bt,
        smax,
        sarr,
        cummax,
        cs,
        sm8,
        sm32,
        sm64,
    };
    let _ = ctx.bt;

    let n = bis.len();
    let mut x: Vec<[f64; 50]> = Vec::with_capacity(n);
    let mut prev_cand = anchor;
    for c in 0..n {
        let bi = bis[c];
        let i = segi[c];
        let (ss, se, sl) = segs[i];
        let _ = (ss, se);
        let slf = sl as f64;
        let sdur = ((bend[i] - bstart[i]).max(1)) as f64;
        let offv = off[c];
        let lvl_prev = if i > 0 { segs[i - 1].2 } else { sl } as f64;
        let lvl_next = if i + 1 < n_segs { segs[i + 1].2 } else { sl } as f64;
        let pb = if i > 0 { bstart[i - 1] } else { anchor };
        let nxt = if i + 1 < n_segs {
            bstart[i + 1]
        } else {
            n_beats as i64
        };
        let gap_a = bi - anchor;
        let mod32 = gap_a % 32;
        let sc_bi = ctx.sc(bi);
        let mut row = [0.0f64; 50];
        row[0] = bound[c] as f64;
        row[1] = first[c] as f64;
        row[2] = (slf - lvl_prev).abs();
        row[3] = (lvl_next - slf).abs();
        row[4] = slf;
        row[5] = offv as f64 / sdur;
        row[6] = offv as f64;
        row[7] = (offv as f64).ln_1p();
        row[8] = if offv % 32 == 0 { 1.0 } else { 0.0 };
        row[9] = if offv % 64 == 0 { 1.0 } else { 0.0 };
        row[10] = if offv % 96 == 0 { 1.0 } else { 0.0 };
        row[11] = if offv % 128 == 0 { 1.0 } else { 0.0 };
        row[12] = sdur;
        row[13] = i as f64 / n_segs as f64;
        row[14] = (n_segs as f64).ln_1p();
        row[15] = bi as f64 / n_beats as f64;
        row[16] = gap_a as f64;
        row[17] = mod32 as f64;
        row[18] = (bi - prev_cand) as f64;
        row[19] = (bi - pb) as f64;
        row[20] = (nxt - bi) as f64;
        row[21] = sc_bi;
        row[22] = 0.0;
        row[23] = sc_bi - ctx.sc(bi - 16);
        row[24] = sc_bi - ctx.sc(bi - 32);
        row[25] = ctx.sc(bi + 32) - sc_bi;
        row[26] = 0.0;
        row[27] = phrase_weight(mod32);
        row[28] = phrase_weight(gap_a % 16);
        row[29] = if mod32 == 16 { 1.0 } else { 0.0 };
        row[30] = tempo.unwrap_or(120.0);
        // v2
        let lo = (bi - 32).max(0) as usize;
        let hi = ((bi + 33).min(ctx.sarr.len() as i64)) as usize;
        let win = &ctx.sarr[lo.min(ctx.sarr.len())..hi.min(ctx.sarr.len())];
        let wmax = win.iter().cloned().fold(f64::MIN, f64::max);
        let lt = win.iter().filter(|&&v| v < sc_bi).count();
        row[31] = ctx.sc(bi - 16);
        row[32] = ctx.sc(bi + 16);
        row[33] = ctx.sc(bi - 32);
        row[34] = ctx.sc(bi + 32);
        row[35] = if !win.is_empty() && sc_bi >= wmax {
            1.0
        } else {
            0.0
        };
        row[36] = if win.is_empty() {
            0.0
        } else {
            lt as f64 / (win.len().saturating_sub(1)).max(1) as f64
        };
        row[37] = if ctx.cummax[bi as usize] > 0.0 {
            sc_bi / ctx.cummax[bi as usize]
        } else {
            0.0
        };
        row[38] = 0.0;
        // v3
        row[39] = ctx.wmean(bi + 1, bi + 17) - ctx.wmean(bi - 16, bi);
        row[40] = ctx.wmean(bi + 1, bi + 33) - ctx.wmean(bi - 32, bi);
        row[41] = ctx.wmean(bi + 1, bi + 65) - ctx.wmean(bi - 64, bi);
        row[42] = ctx.sm8.get(bi as usize).copied().unwrap_or(0.0);
        row[43] = ctx.sm32.get(bi as usize).copied().unwrap_or(0.0);
        row[44] = ctx.sm64.get(bi as usize).copied().unwrap_or(0.0);
        row[45] = {
            let s = ctx.sm32.get(bi as usize).copied().unwrap_or(0.0);
            let lo = ((bi - 32).max(0)) as usize;
            let hi = ((bi + 33).min(ctx.sm32.len() as i64)) as usize;
            let seg = &ctx.sm32[lo.min(ctx.sm32.len())..hi.min(ctx.sm32.len())];
            let mx = seg.iter().cloned().fold(f64::MIN, f64::max);
            if !seg.is_empty() && s >= mx {
                1.0
            } else {
                0.0
            }
        };
        row[46] = 0.0;
        row[47] = 0.0;
        row[48] = 0.0;
        row[49] = 0.0;
        x.push(row);
        prev_cand = bi;
    }

    // post-llenado por segmento (26, 38, 46..49) y rank global (22)
    if !x.is_empty() {
        for i in 0..n_segs {
            let m_idx: Vec<usize> = (0..n).filter(|&c| segi[c] == i).collect();
            if m_idx.is_empty() {
                continue;
            }
            let mx = m_idx.iter().map(|&c| x[c][21]).fold(f64::MIN, f64::max);
            let mean = m_idx.iter().map(|&c| x[c][21]).sum::<f64>() / m_idx.len() as f64;
            let n_seg = m_idx.len();
            // rank intra-segmento sobre col 21
            let mut v21: Vec<(usize, f64)> = m_idx.iter().map(|&c| (c, x[c][21])).collect();
            v21.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            let rank_of: std::collections::HashMap<usize, f64> = v21
                .iter()
                .enumerate()
                .map(|(r, &(c, _))| (c, r as f64 / (n_seg.saturating_sub(1)).max(1) as f64))
                .collect();
            let best_j = *m_idx
                .iter()
                .max_by(|&&a, &&b| x[a][21].partial_cmp(&x[b][21]).unwrap())
                .unwrap();
            for &c in &m_idx {
                if mx > 0.0 {
                    x[c][26] = x[c][21] / mx;
                }
                x[c][38] = x[c][21] - mean;
                x[c][46] = *rank_of.get(&c).unwrap_or(&0.0);
                x[c][47] = x[best_j][6] - x[c][6];
                x[c][49] = n_seg as f64;
            }
            x[best_j][48] = 1.0;
        }
        // rank global col 22
        let mut v21: Vec<(usize, f64)> = (0..n).map(|c| (c, x[c][21])).collect();
        v21.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let rank_of: std::collections::HashMap<usize, f64> = v21
            .iter()
            .enumerate()
            .map(|(r, &(c, _))| (c, r as f64 / (n.saturating_sub(1)).max(1) as f64))
            .collect();
        for c in 0..n {
            x[c][22] = *rank_of.get(&c).unwrap_or(&0.0);
        }
    }

    // primer cue FORZADO = ancla (primer beat >= 0): invariante herramienta de referencia "primer
    // cue = inicio del 1er segmento" — su 1er segmento empieza en su primer
    // beat >= 0 (~0.34 s); el nuestro arrancaba en t0 <= 0 (cue negativo)
    let forced_bi = anchor;
    (x, bis, forced_bi)
}

/// Decodificación DP exacta (_decode_ranked, decode='dp') o greedy.
fn decode_ranked(
    bis: &[i64],
    prio: &[f64],
    forced_bi: i64,
    max_cues: usize,
    min_gap_beats: usize,
    thr: f64,
    dp: bool,
) -> Vec<i64> {
    let n = bis.len();
    if n == 0 {
        return vec![forced_bi];
    }
    if !dp {
        // greedy
        let mut chosen = vec![forced_bi];
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| {
            (-(prio[a] - thr))
                .partial_cmp(&-(prio[b] - thr))
                .unwrap()
                .then(bis[a].cmp(&bis[b]))
        });
        for &j in &order {
            let b = bis[j];
            if b == forced_bi || prio[j] < thr {
                continue;
            }
            if chosen.iter().all(|c| (b - c).abs() >= min_gap_beats as i64) {
                chosen.push(b);
            }
            if chosen.len() == max_cues {
                break;
            }
        }
        chosen.sort();
        return chosen;
    }

    let k_max = max_cues;
    let mut w: Vec<f64> = prio.iter().map(|p| p - thr).collect();
    let mut f = bis.partition_point(|&b| b < forced_bi);
    if f >= n || bis[f] != forced_bi {
        f = (0..n).min_by_key(|&i| (bis[i] - forced_bi).abs()).unwrap();
    }
    let big = w.iter().fold(0.0f64, |a, v| a.max(v.abs())) + 10.0;
    w[f] += 2.0 * big;
    let neg = -1e18f64;

    // dp[i][k], par[i][k]; M[k][j] prefijo-max
    let mut dp = vec![neg; n * (k_max + 1)];
    let mut par = vec![-1i64; n * (k_max + 1)];
    let mut m_arr = vec![neg; (k_max + 1) * (n + 1)];
    let mut argm = vec![-1i64; (k_max + 1) * (n + 1)];
    for j in 0..=n {
        m_arr[j] = 0.0; // M[0][*] = 0
    }
    let midx = |k: usize, j: usize| k * (n + 1) + j;
    let didx = |i: usize, k: usize| i * (k_max + 1) + k;

    for i in 0..n {
        // p = primer índice con bis[p] > bis[i] - min_gap
        let p = bis.partition_point(|&b| b <= bis[i] - min_gap_beats as i64);
        for k in 1..=k_max {
            if k == 1 {
                dp[didx(i, k)] = w[i];
                par[didx(i, k)] = -1;
            } else if p > 0 && m_arr[midx(k - 1, p)] > neg / 2.0 {
                dp[didx(i, k)] = w[i] + m_arr[midx(k - 1, p)];
                par[didx(i, k)] = argm[midx(k - 1, p)];
            }
        }
        for k in 1..=k_max {
            let cur = dp[didx(i, k)];
            let prev_m = m_arr[midx(k, i)];
            if cur > prev_m {
                m_arr[midx(k, i + 1)] = cur;
                argm[midx(k, i + 1)] = i as i64;
            } else {
                m_arr[midx(k, i + 1)] = prev_m;
                argm[midx(k, i + 1)] = argm[midx(k, i)];
            }
        }
    }
    let (mut bi_, mut bk) = (0usize, 1usize);
    let mut best = neg;
    for k in 1..=k_max {
        for i in 0..n {
            if dp[didx(i, k)] > best {
                best = dp[didx(i, k)];
                bi_ = i;
                bk = k;
            }
        }
    }
    let mut chosen: Vec<i64> = Vec::new();
    let (mut i, mut k) = (bi_ as i64, bk as i64);
    while i >= 0 && k >= 1 {
        chosen.push(bis[i as usize]);
        let p = par[didx(i as usize, k as usize)];
        i = p;
        k -= 1;
    }
    if !chosen.contains(&forced_bi) {
        chosen.push(forced_bi);
    }
    chosen.sort();
    chosen
}

/// Selección final: cues con el ranker entrenado.
/// segs: (start, end, level); beats y scores de detect_pre_analysis.
pub fn cue_points_ranked(
    model: &RankerModel,
    segs: &[(f64, f64, i32)],
    beats: &[f64],
    scores: &[f64],
    tempo: Option<f64>,
) -> Vec<(f64, i32)> {
    let (x, bis, forced_bi) = candidate_features(segs, beats, scores, tempo);
    if x.is_empty() {
        return Vec::new();
    }
    let prio: Vec<f64> = x.iter().map(|row| model.predict(row)).collect();
    let chosen = decode_ranked(
        &bis,
        &prio,
        forced_bi,
        model.r.max_cues,
        model.r.min_gap_beats,
        model.r.threshold,
        model.r.decode == "dp",
    );
    chosen
        .into_iter()
        .map(|b| {
            let t = beats[b as usize];
            let lvl = segs
                .iter()
                .find(|&&(ss, se, _)| ss - 1e-6 <= t && t < se)
                .map(|&(_, _, l)| l)
                .unwrap_or(5);
            (t, lvl)
        })
        .collect()
}
