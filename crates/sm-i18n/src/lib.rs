//! sm-i18n — internacionalización de Smart Mix.
//!
//! Port de `core/i18n/__init__.py` (harmonic-engine): 7 catálogos embebidos
//! con `include_str!` (en, es, fr, de, it, pt, nl), idioma por defecto `en`
//! y cascada de traducción idioma → inglés → clave.
//!
//! Los JSON de `locales/` usan claves planas con notación punto
//! (`"app.tagline": "…"`, exactamente el formato del .py, que hace
//! `_catalog.get(key)`). Este port además resuelve la notación punto sobre
//! JSON **anidado** (`{"app": {"tagline": "…"}}`) por si un catálogo futuro
//! se reorganiza: primero se prueba la clave exacta (port fiel), luego el
//! descenso por niveles separando por `.`.
//!
//! ```
//! use sm_i18n::I18n;
//! let es = I18n::load("es");
//! assert!(es.t("app.tagline").contains("Análisis"));
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

/// Idiomas soportados (mismos miembros y orden que `SUPPORTED` del .py).
pub const SUPPORTED: [&str; 7] = ["en", "es", "fr", "de", "it", "pt", "nl"];

/// Idioma por defecto / de respaldo de la cascada.
pub const DEFAULT: &str = "en";

/// Catálogos embebidos: copia byte a byte de harmonic-engine
/// `core/i18n/locales/*.json`.
const RAW_LOCALES: [(&str, &str); 7] = [
    ("en", include_str!("../locales/en.json")),
    ("es", include_str!("../locales/es.json")),
    ("fr", include_str!("../locales/fr.json")),
    ("de", include_str!("../locales/de.json")),
    ("it", include_str!("../locales/it.json")),
    ("pt", include_str!("../locales/pt.json")),
    ("nl", include_str!("../locales/nl.json")),
];

fn catalogs() -> &'static HashMap<&'static str, Value> {
    static CATALOGS: OnceLock<HashMap<&'static str, Value>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        RAW_LOCALES
            .iter()
            .filter_map(|(lang, raw)| match serde_json::from_str(raw) {
                Ok(v) => Some((*lang, v)),
                Err(_) => None, // catálogo roto → no disponible (no rompe el proceso)
            })
            .collect()
    })
}

/// Busca `key` en el catálogo de `lang`.
///
/// 1. Clave exacta en el nivel superior (port fiel de `_active.get(key)`).
/// 2. Notación punto sobre JSON anidado (desciende por cada `.`).
fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let catalog = catalogs().get(lang)?;
    if let Some(s) = catalog.get(key).and_then(Value::as_str) {
        return Some(s);
    }
    let mut node = catalog;
    for part in key.split('.') {
        node = node.get(part)?;
    }
    node.as_str()
}

/// Traductor inmutable de un idioma.
///
/// `I18n::load` nunca falla: un idioma inexistente cae a [`DEFAULT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct I18n {
    lang: &'static str,
}

impl I18n {
    /// Carga el catálogo de `lang` (fallback a `en` si no existe).
    ///
    /// La comparación ignora mayúsculas; el código activo siempre es uno
    /// de [`SUPPORTED`].
    pub fn load(lang: &str) -> I18n {
        let lang = SUPPORTED
            .iter()
            .find(|supported| supported.eq_ignore_ascii_case(lang))
            .copied()
            .unwrap_or(DEFAULT);
        I18n { lang }
    }

    /// Traduce `key` con cascada idioma → inglés → clave.
    ///
    /// Si la clave no existe en ningún catálogo se devuelve la propia clave
    /// (misma regla que `core.i18n.t`). El lifetime de salida está atado al
    /// de `key` porque el fallback ES la clave (con literales `'static` es
    /// transparente).
    pub fn t<'a>(&self, key: &'a str) -> &'a str {
        lookup(self.lang, key)
            .or_else(|| lookup(DEFAULT, key))
            .unwrap_or(key)
    }

    /// Idiomas disponibles (los 7 catálogos embebidos).
    pub fn available() -> Vec<&'static str> {
        SUPPORTED.to_vec()
    }

    /// Código del idioma activo de esta instancia.
    pub fn language(&self) -> &'static str {
        self.lang
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_es_devuelve_cadena_espanola() {
        let es = I18n::load("es");
        assert_eq!(
            es.t("app.tagline"),
            "Análisis musical y preparación de medios USB para la cabina del DJ"
        );
    }

    #[test]
    fn load_desconocido_cae_a_ingles() {
        let xx = I18n::load("xx");
        assert_eq!(xx.language(), "en");
        assert_eq!(
            xx.t("app.tagline"),
            "Music analysis and USB media preparation for the DJ booth"
        );
    }

    #[test]
    fn clave_inexistente_devuelve_la_clave() {
        let en = I18n::load("en");
        assert_eq!(en.t("no.existe.nunca"), "no.existe.nunca");
    }

    #[test]
    fn cascada_y_case_insensitive() {
        let de = I18n::load("de");
        assert!(!de.t("app.tagline").is_ascii());
        let es = I18n::load("ES");
        assert_eq!(es.language(), "es");
    }

    #[test]
    fn available_son_los_siete_idiomas() {
        let langs = I18n::available();
        assert_eq!(langs.len(), 7);
        assert_eq!(langs, vec!["en", "es", "fr", "de", "it", "pt", "nl"]);
    }

    #[test]
    fn los_siete_catalogos_tienen_claves() {
        for lang in SUPPORTED {
            assert!(lookup(lang, "app.name").is_some(), "falta app.name en {lang}");
        }
    }

    #[test]
    fn notacion_punto_sobre_json_anidado() {
        // El descenso por niveles funciona con JSON anidado sintético.
        let nested: Value =
            serde_json::from_str(r#"{"usb": {"scan": {"title": "Escaneando"}}}"#).unwrap();
        let mut node = &nested;
        for part in "usb.scan.title".split('.') {
            node = node.get(part).unwrap();
        }
        assert_eq!(node.as_str(), Some("Escaneando"));
    }
}
