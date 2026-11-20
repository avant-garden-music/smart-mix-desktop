//! F-24 Tag Editor — lógica pura de selección múltiple y edición masiva.
//!
//! Fase 1: transiciones de la selección múltiple de la tabla. Las fases
//! siguientes añaden aquí el diff de campos de `TrackMeta` (fase 2) y el
//! filtro de formatos de archivo escribibles (fase 3) — todo función pura
//! para testear sin UI ni BD.

use std::collections::HashSet;

/// Ctrl+clic: alterna una pista en la selección múltiple.
#[allow(dead_code)] // seleccion multiple via checkboxes del kit
pub fn toggle_selection(set: &mut HashSet<i64>, id: i64) {
    if !set.remove(&id) {
        set.insert(id);
    }
}

/// Shift+clic / ✓TODO: añade ids sin vaciar lo ya seleccionado (unión).
pub fn extend_selection(set: &mut HashSet<i64>, ids: &[i64]) {
    for id in ids {
        set.insert(*id);
    }
}

use sm_model::TrackMeta;

/// F-24 fase 3: formatos de archivo cuyo tag sabe escribir la maquinaria
/// de tags de sm-export (lofty): mp3/aiff(/aif)/wav (ID3v2) y flac
/// (Vorbis). El resto (m4a/ogg/…) se omite y se cuenta como skip.
pub fn is_taggable(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".mp3")
        || p.ends_with(".aiff")
        || p.ends_with(".aif")
        || p.ends_with(".wav")
        || p.ends_with(".flac")
}

/// Campos editables del Tag Editor (los String de TrackMeta que existen en
/// el modelo v1 — NO hay campo Comment). Cadena vacía = "sin cambio" para
/// las N pistas; v1 NO ofrece sobrescribir con vacío.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TagEditFields {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    /// Año como texto: se parsea a i32 al aplicar (inválido = sin cambio).
    pub year: String,
}

impl TagEditFields {
    fn parse_year(&self) -> Option<i32> {
        let t = self.year.trim();
        if t.is_empty() {
            return None;
        }
        t.parse::<i32>().ok().filter(|y| (0..=9999).contains(y))
    }

    /// Año ya validado (None = campo vacío o inválido).
    pub fn year_value(&self) -> Option<i32> {
        self.parse_year()
    }

    /// true si algún campo escribiría algo (habilita el botón Aplicar).
    pub fn any_set(&self) -> bool {
        !self.changed_fields().is_empty()
    }

    /// Campos que quedarían editados (encabezado del preview).
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.title.is_empty() {
            v.push("title");
        }
        if !self.artist.is_empty() {
            v.push("artist");
        }
        if !self.album.is_empty() {
            v.push("album");
        }
        if !self.genre.is_empty() {
            v.push("genre");
        }
        if self.parse_year().is_some() {
            v.push("year");
        }
        v
    }
}

fn disp(v: &str) -> &str {
    if v.is_empty() {
        "(vacío)"
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// F-31: limpieza de tags basura — detección PURA (sin IO ni UI).
//
// Conservador y reversible por diseño: solo se consideran basura el
// COMMENT con firmas típicas (tiendas/herramientas/claves) y, con
// opt-in explícito, los frames iTunes de gapless (iTunSMPB/TCMP).
// NUNCA artista/título/álbum/key/bpm/energy/cues.
// ---------------------------------------------------------------------------

/// Estado de tags del archivo relevante para la limpieza.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JunkTagState {
    pub comment: Option<String>,
    pub itun_smpb: bool,
    pub tcmp: bool,
}

/// Política de limpieza: comments basura por defecto; iTunes gapless SOLO
/// con checkbox explícito (iTunSMPB rompe la reproducción gapless si se
/// borra de pistas que la usan).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JunkPolicy {
    pub clean_comments: bool,
    pub clean_itunes_gapless: bool,
}

impl Default for JunkPolicy {
    fn default() -> Self {
        Self {
            clean_comments: true,
            clean_itunes_gapless: false,
        }
    }
}

/// Firmas típicas de comments basura: la detección (`is_junk_comment`)
/// vive en sm-export junto al formato (fuente única) y se re-exporta aquí
/// para los callers de sm-app.
pub use sm_export::id3::is_junk_comment;

/// true si el comment es basura detectable (url de tienda, firma de
/// herramienta, clave camelot pelada o codificador). Los comments
/// inocentes (notas del usuario) NUNCA se marcan.

/// Campos que la política quitaría de este estado (vacío = nada que hacer).
pub fn junk_removals(st: &JunkTagState, p: &JunkPolicy) -> Vec<&'static str> {
    let mut v = Vec::new();
    if p.clean_comments && st.comment.as_deref().is_some_and(is_junk_comment) {
        v.push("comment");
    }
    if p.clean_itunes_gapless {
        if st.itun_smpb {
            v.push("iTunSMPB");
        }
        if st.tcmp {
            v.push("TCMP");
        }
    }
    v
}

/// Línea de preview "campo: valor" de lo que se quitaría (vacío = nada).
pub fn junk_preview_line(st: &JunkTagState, p: &JunkPolicy) -> String {
    junk_removals(st, p)
        .iter()
        .map(|f| {
            if *f == "comment" {
                format!("comment: {}", st.comment.as_deref().unwrap_or(""))
            } else {
                (*f).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Aplica SOLO los campos no vacíos sobre `meta` (diff in-place).
/// Devuelve true si modificó algo.
pub fn apply_fields(meta: &mut TrackMeta, f: &TagEditFields) -> bool {
    let mut changed = false;
    if !f.title.is_empty() && meta.title != f.title {
        meta.title = f.title.clone();
        changed = true;
    }
    if !f.artist.is_empty() && meta.artist != f.artist {
        meta.artist = f.artist.clone();
        changed = true;
    }
    if !f.album.is_empty() && meta.album != f.album {
        meta.album = f.album.clone();
        changed = true;
    }
    if !f.genre.is_empty() && meta.genre != f.genre {
        meta.genre = f.genre.clone();
        changed = true;
    }
    if let Some(y) = f.parse_year() {
        if meta.year != Some(y) {
            meta.year = Some(y);
            changed = true;
        }
    }
    changed
}

/// Preview "campo: antes → después" de UNA pista, solo los campos que
/// cambiarían; cadena vacía = la pista no cambia.
pub fn preview_line(meta: &TrackMeta, f: &TagEditFields) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !f.title.is_empty() && meta.title != f.title {
        parts.push(format!("title: {} → {}", disp(&meta.title), &f.title));
    }
    if !f.artist.is_empty() && meta.artist != f.artist {
        parts.push(format!("artist: {} → {}", disp(&meta.artist), &f.artist));
    }
    if !f.album.is_empty() && meta.album != f.album {
        parts.push(format!("album: {} → {}", disp(&meta.album), &f.album));
    }
    if !f.genre.is_empty() && meta.genre != f.genre {
        parts.push(format!("genre: {} → {}", disp(&meta.genre), &f.genre));
    }
    if let Some(y) = f.parse_year() {
        if meta.year != Some(y) {
            parts.push(format!(
                "year: {} → {y}",
                meta.year
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "(vacío)".into())
            ));
        }
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_adds_then_removes() {
        let mut s = HashSet::new();
        toggle_selection(&mut s, 7);
        assert_eq!(s.len(), 1);
        assert!(s.contains(&7));
        toggle_selection(&mut s, 7);
        assert!(s.is_empty());
    }

    #[test]
    fn extend_keeps_previous_and_is_idempotent() {
        let mut s = HashSet::new();
        toggle_selection(&mut s, 1);
        extend_selection(&mut s, &[2, 3]);
        assert_eq!(s.len(), 3);
        extend_selection(&mut s, &[2, 3]);
        assert_eq!(s.len(), 3);
        assert!(s.contains(&1));
    }

    fn base_meta() -> sm_model::TrackMeta {
        sm_model::TrackMeta {
            title: "Original".into(),
            artist: "DJ X".into(),
            album: "LP".into(),
            genre: "House".into(),
            year: Some(2020),
            ..Default::default()
        }
    }

    #[test]
    fn campos_vacios_no_tocan_nada() {
        let mut m = base_meta();
        let f = TagEditFields::default();
        assert!(!apply_fields(&mut m, &f));
        assert_eq!(m, base_meta());
        assert!(!f.any_set());
        assert!(preview_line(&m, &f).is_empty());
    }

    #[test]
    fn aplica_solo_lo_no_vacio() {
        let mut m = base_meta();
        let f = TagEditFields {
            genre: "Techno".into(),
            year: "1999".into(),
            ..Default::default()
        };
        assert!(apply_fields(&mut m, &f));
        assert_eq!(m.title, "Original"); // no vaciado
        assert_eq!(m.genre, "Techno");
        assert_eq!(m.year, Some(1999));
        assert_eq!(f.changed_fields(), vec!["genre", "year"]);
        assert_eq!(
            preview_line(&base_meta(), &f),
            "genre: House → Techno · year: 2020 → 1999"
        );
    }

    #[test]
    fn year_invalido_es_sil_cambio() {
        let mut m = base_meta();
        let f = TagEditFields {
            year: "nope".into(),
            ..Default::default()
        };
        assert!(!apply_fields(&mut m, &f));
        assert_eq!(m.year, Some(2020));
        // un año fuera de rango tampoco aplica
        let f2 = TagEditFields {
            year: "99999".into(),
            ..Default::default()
        };
        let mut m2 = base_meta();
        assert!(!apply_fields(&mut m2, &f2));
        assert_eq!(m2.year, Some(2020));
    }

    #[test]
    fn aplicar_valor_identico_no_cuenta_como_cambio() {
        let mut m = base_meta();
        let f = TagEditFields {
            title: "Original".into(),
            ..Default::default()
        };
        assert!(!apply_fields(&mut m, &f)); // mismo valor → sin cambio
        assert_eq!(f.changed_fields(), vec!["title"]); // el campo sí figura
        assert!(preview_line(&m, &f).is_empty());
    }

    #[test]
    fn formatos_etiquetables() {
        assert!(is_taggable("C:/m/a.mp3"));
        assert!(is_taggable("C:/m/a.MP3"));
        assert!(is_taggable("C:/m/a.aiff"));
        assert!(is_taggable("C:/m/a.aif"));
        assert!(is_taggable("C:/m/a.wav"));
        assert!(is_taggable("C:/m/a.flac"));
        assert!(!is_taggable("C:/m/a.m4a"));
        assert!(!is_taggable("C:/m/a.ogg"));
        assert!(!is_taggable("C:/m/a.mp3.bak"));
    }

    #[test]
    fn year_value_publico() {
        let f = TagEditFields {
            year: "2024 ".into(),
            ..Default::default()
        };
        assert_eq!(f.year_value(), Some(2024));
        assert_eq!(TagEditFields::default().year_value(), None);
    }

    #[test]
    fn preview_muestra_vacio_antes() {
        let mut m = base_meta();
        m.genre = String::new();
        let f = TagEditFields {
            genre: "Tech House".into(),
            ..Default::default()
        };
        assert_eq!(preview_line(&m, &f), "genre: (vacío) → Tech House");
    }

    // ---- F-31: detección de comments basura ----

    #[test]
    fn comment_con_url_de_tienda_es_basura() {
        assert!(is_junk_comment("http://www.beatport.com/track/x/123"));
        assert!(is_junk_comment("www.junodownload.com"));
        assert!(is_junk_comment("Purchased at Beatport"));
        assert!(is_junk_comment("Traxsource.com exclusive"));
        assert!(is_junk_comment("https://music.apple.com/album/456"));
    }

    #[test]
    fn comment_de_herramientas_es_basura() {
        assert!(is_junk_comment("Serato DJ Pro analysis"));
        assert!(is_junk_comment("Traktor Library"));
        assert!(is_junk_comment("BPM Analyzer 1.0"));
        assert!(is_junk_comment("Initial Key: 8A"));
        assert!(is_junk_comment("keyfinder 8A - 128"));
        assert!(is_junk_comment("8A"), "camelot pelado");
        assert!(is_junk_comment("12B"));
        assert!(is_junk_comment("LAME3.99r"));
    }

    #[test]
    fn comment_inocente_no_se_toca() {
        assert!(!is_junk_comment("mi nota"));
        assert!(!is_junk_comment("para el set del viernes"));
        assert!(!is_junk_comment("intro corta, entrada en el break"));
        assert!(!is_junk_comment("remember the night"));
        assert!(!is_junk_comment(""), "vacío = nada que borrar");
        assert!(!is_junk_comment("  "), "sólo espacios = nada");
    }

    #[test]
    fn remociones_según_politica() {
        let st = JunkTagState {
            comment: Some("http://beatport.com".into()),
            itun_smpb: true,
            tcmp: true,
        };
        // política por defecto: SOLO el comment basura
        assert_eq!(junk_removals(&st, &JunkPolicy::default()), vec!["comment"]);
        // con gapless explícito: los tres
        let full = JunkPolicy {
            clean_comments: true,
            clean_itunes_gapless: true,
        };
        assert_eq!(
            junk_removals(&st, &full),
            vec!["comment", "iTunSMPB", "TCMP"]
        );
        // comment inocente: solo quedan los frames itunes
        let ok = JunkTagState {
            comment: Some("mi nota".into()),
            itun_smpb: true,
            tcmp: true,
        };
        assert_eq!(junk_removals(&ok, &full), vec!["iTunSMPB", "TCMP"]);
        assert_eq!(
            junk_removals(&ok, &JunkPolicy::default()),
            Vec::<&str>::new()
        );
        // sin nada: vacío
        assert!(junk_removals(&JunkTagState::default(), &full).is_empty());
        // comments desactivado en la política
        let no_c = JunkPolicy {
            clean_comments: false,
            clean_itunes_gapless: true,
        };
        assert_eq!(junk_removals(&st, &no_c), vec!["iTunSMPB", "TCMP"]);
    }

    #[test]
    fn preview_line_lista_campos() {
        let full = JunkPolicy {
            clean_comments: true,
            clean_itunes_gapless: true,
        };
        let st = JunkTagState {
            comment: Some("Serato analysis".into()),
            itun_smpb: false,
            tcmp: true,
        };
        assert_eq!(
            junk_preview_line(&st, &full),
            "comment: Serato analysis · TCMP"
        );
        assert_eq!(junk_preview_line(&JunkTagState::default(), &full), "");
    }
}
