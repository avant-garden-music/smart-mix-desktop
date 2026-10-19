//! Inferencia ONNX (key/energía) sobre los modelos entrenados por
//! harmonic-engine (originales, entrenados contra labels de su propia DB).
//!
//! El clasificador 1-10 y el de tonalidad de MIK viven en su nube; aquí se
//! usan los sustitutos entrenados por el proyecto (90.5% CV en tonalidad).
//!
//! Si `ort` o los modelos no están disponibles, el motor degrada
//! transparentemente al fallback Krumhansl (tonalidad).

use std::path::{Path, PathBuf};

pub struct Models {
    #[cfg(feature = "onnx")]
    key: Option<ort::session::Session>,
    #[cfg(feature = "onnx")]
    key_input: Option<String>,
    #[cfg(feature = "onnx")]
    energy_global: Option<ort::session::Session>,
    #[cfg(feature = "onnx")]
    energy_segment: Option<ort::session::Session>,
    /// Clasificador lineal puro (mismos pesos del ONNX v4, sin runtime ext).
    key_linear: Option<KeyLinear>,
    pub dir: Option<PathBuf>,
}

/// RegLog 112→24 con softmax — pesos extraídos de key_model_v4.onnx
/// (coef [112×24] row-major + intercept [24]) en models/key_linear.json.
pub struct KeyLinear {
    coef: Vec<f64>,      // [i*24 + c]
    intercept: Vec<f64>, // [c]
}

impl KeyLinear {
    pub fn load(dir: &Path) -> Option<KeyLinear> {
        let p = dir.join("key_linear.json");
        let data = std::fs::read_to_string(p).ok()?;
        #[derive(serde::Deserialize)]
        struct Js {
            coef: Vec<f64>,
            intercept: Vec<f64>,
        }
        // tolerante: el json tiene dims/values
        #[derive(serde::Deserialize)]
        struct Wrap {
            #[serde(default)]
            coef: Option<serde_json::Value>,
            #[serde(default)]
            intercept: Option<serde_json::Value>,
        }
        let w: Wrap = serde_json::from_str(&data).ok()?;
        let pick = |v: Option<serde_json::Value>| -> Option<Vec<f64>> {
            let v = v?;
            if let Some(vals) = v.get("values").and_then(|x| x.as_array()) {
                return Some(vals.iter().filter_map(|x| x.as_f64()).collect());
            }
            if let Some(vals) = v.as_array() {
                return Some(vals.iter().filter_map(|x| x.as_f64()).collect());
            }
            None
        };
        let coef = pick(w.coef)?;
        let intercept = pick(w.intercept)?;
        if coef.len() != 112 * 24 || intercept.len() != 24 {
            return None;
        }
        let _: Option<Js> = None;
        Some(KeyLinear { coef, intercept })
    }

    /// scores = X·W + b → softmax → (camelot, proba) desc.
    pub fn classify(&self, feats112: &[f64]) -> Vec<(String, f64)> {
        let mut scores = self.intercept.clone();
        for (i, &x) in feats112.iter().enumerate() {
            if x == 0.0 {
                continue;
            }
            for c in 0..24 {
                scores[c] += x * self.coef[i * 24 + c];
            }
        }
        let mx = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = scores.iter().map(|s| (s - mx).exp()).collect();
        let sum: f64 = exps.iter().sum();
        let classes = classes_sorted();
        let mut pairs: Vec<(String, f64)> = exps
            .iter()
            .enumerate()
            .map(|(c, e)| (classes[c].clone(), e / sum))
            .collect();
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs
    }
}

impl Default for Models {
    fn default() -> Self {
        Self::none()
    }
}

fn classes_sorted() -> Vec<String> {
    // CLASSES = sorted(set(camelot)) — 24 códigos lexicográficos
    let mut codes: Vec<String> = (0..12u8)
        .flat_map(|pc| {
            [
                sm_model::camelot::camelot_code(pc, false),
                sm_model::camelot::camelot_code(pc, true),
            ]
        })
        .collect();
    codes.sort();
    codes
}

impl Models {
    pub fn none() -> Self {
        Models {
            #[cfg(feature = "onnx")]
            key: None,
            #[cfg(feature = "onnx")]
            key_input: None,
            #[cfg(feature = "onnx")]
            energy_global: None,
            #[cfg(feature = "onnx")]
            energy_segment: None,
            key_linear: None,
            dir: None,
        }
    }

    /// Carga perezosa desde `dir` (p.ej. `<exe>/models` o `repo/models`).
    /// Los fallos son no-fatales: se registra y se sigue sin ese modelo.
    pub fn load(dir: &Path) -> Models {
        let mut m = Models::none();
        m.dir = Some(dir.to_path_buf());
        if !dir.is_dir() {
            eprintln!("[models] dir no existe: {}", dir.display());
            return m;
        }
        // clasificador lineal puro (preferente: sin runtime externo)
        match KeyLinear::load(dir) {
            Some(kl) => {
                m.key_linear = Some(kl);
                eprintln!("[models] key_linear.json cargado (clasificador puro)");
            }
            None => eprintln!("[models] key_linear.json ausente"),
        }
        #[cfg(feature = "onnx")]
        {
            // key: probar variantes en orden
            for file in ["key_model_v4_nomap.onnx", "key_model_v4.onnx", "key_model_v2.onnx"] {
                let p = dir.join(file);
                if !p.exists() {
                    continue;
                }
                match ort::session::Session::builder().and_then(|b| b.commit_from_file(&p)) {
                    Ok(s) => {
                        m.key_input = Some(s.inputs[0].name.clone());
                        m.key = Some(s);
                        eprintln!("[models] cargado: {}", p.display());
                        break;
                    }
                    Err(e) => eprintln!("[models] error cargando {}: {e}", p.display()),
                }
            }
            for (slot, file) in [
                (&mut m.energy_global, "energy_global.onnx"),
                (&mut m.energy_segment, "energy_segment.onnx"),
            ] {
                let p = dir.join(file);
                if !p.exists() {
                    eprintln!("[models] modelo ausente: {}", p.display());
                    continue;
                }
                match ort::session::Session::builder().and_then(|b| b.commit_from_file(&p)) {
                    Ok(s) => {
                        *slot = Some(s);
                        eprintln!("[models] cargado: {}", p.display());
                    }
                    Err(e) => eprintln!("[models] error cargando {}: {e}", p.display()),
                }
            }
        }
        m
    }

    pub fn available(&self) -> bool {
        self.key_linear.is_some()
            || {
                #[cfg(feature = "onnx")]
                {
                    self.key.is_some()
                }
                #[cfg(not(feature = "onnx"))]
                {
                    false
                }
            }
    }

    /// Clasifica tonalidad: (camelot, proba) ordenadas desc.
    /// Preferencia: lineal puro → ONNX. Mapeo: índice → CLASSES sorted.
    pub fn classify_key(&self, feats112: &[f64]) -> Option<Vec<(String, f64)>> {
        if let Some(kl) = &self.key_linear {
            return Some(kl.classify(feats112));
        }
        #[cfg(feature = "onnx")]
        {
            self.classify_key_onnx(feats112)
        }
        #[cfg(not(feature = "onnx"))]
        {
            None
        }
    }

    #[cfg(feature = "onnx")]
    fn classify_key_onnx(&self, feats112: &[f64]) -> Option<Vec<(String, f64)>> {
        let session = self.key.as_ref()?;
        let input = self.key_input.clone().unwrap_or_else(|| "input".into());
        let data: Vec<f32> = feats112.iter().map(|&v| v as f32).collect();
        let tensor = ort::value::Tensor::from_array(([1, feats112.len()], data)).ok()?;
        let outputs = session
            .run(ort::inputs![input.as_str() => tensor].ok()?)
            .ok()?;
        // skl2onnx: salida "probabilities" (o la última)
        let names: Vec<String> = session.outputs.iter().map(|o| o.name.clone()).collect();
        let out_name = names
            .iter()
            .find(|n| n.contains("probabilit"))
            .cloned()
            .or_else(|| names.last().cloned())?;
        let flat: Vec<f32> = outputs[out_name.as_str()]
            .try_extract_tensor::<f32>()
            .ok()?
            .view()
            .iter()
            .copied()
            .collect();
        let classes = classes_sorted();
        let mut pairs: Vec<(String, f64)> = flat
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < classes.len())
            .map(|(i, &p)| (classes[i].clone(), p as f64))
            .collect();
        let total: f64 = pairs.iter().map(|(_, p)| p).sum();
        if total > 0.0 {
            for (_, p) in pairs.iter_mut() {
                *p /= total;
            }
        }
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Some(pairs)
    }

    #[cfg(feature = "onnx")]
    fn regress(&self, session: &ort::session::Session, feats: &[f64]) -> Option<f64> {
        let input = session.inputs[0].name.clone();
        let data: Vec<f32> = feats.iter().map(|&v| v as f32).collect();
        let tensor = ort::value::Tensor::from_array(([1, feats.len()], data)).ok()?;
        let outputs = session.run(ort::inputs![input.as_str() => tensor].ok()?).ok()?;
        let name = session
            .outputs
            .last()
            .map(|o| o.name.clone())
            .unwrap_or_else(|| "variable".into());
        let flat: Vec<f32> = outputs[name.as_str()]
            .try_extract_tensor::<f32>()
            .ok()?
            .view()
            .iter()
            .copied()
            .collect();
        flat.first().map(|&v| v as f64)
    }

    pub fn energy_global(&self, feats122: &[f64]) -> Option<f64> {
        #[cfg(feature = "onnx")]
        {
            self.energy_global.as_ref().and_then(|s| self.regress(s, feats122))
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = feats122;
            None
        }
    }

    pub fn energy_segment(&self, feats168: &[f64]) -> Option<f64> {
        #[cfg(feature = "onnx")]
        {
            self.energy_segment.as_ref().and_then(|s| self.regress(s, feats168))
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = feats168;
            None
        }
    }
}
