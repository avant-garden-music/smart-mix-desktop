//! Textos de la interfaz (es/en) con cascada es → en → clave.
//! El resto de idiomas del universo Smart Mix (fr/de/it/pt/nl) vive en
//! sm-i18n con las claves compartidas del toolkit (USB/equipos).

pub fn t(lang: &str, key: &str) -> String {
    if lang.starts_with("es") {
        return es(key);
    }
    en(key, es(key))
}

fn en(key: &str, fallback: String) -> String {
    match key {
        "menu.file" => "File".into(),
        "menu.view" => "View".into(),
        "menu.import_folder" => "Import folder…".into(),
        "menu.analyze_pending" => "Analyze pending".into(),
        "menu.reanalyze" => "Re-analyze selected".into(),
        "menu.export" => "Export…".into(),
        "menu.prefs" => "Preferences…".into(),
        "menu.about" => "About".into(),
        "toolbar.search" => "Search title, artist, album…".into(),
        "toolbar.compat" => "Compatible with".into(),
        "side.collections" => "COLLECTIONS".into(),
        "side.playlists" => "PLAYLISTS".into(),
        "col.all" => "All tracks".into(),
        "col.unanalyzed" => "Unanalyzed".into(),
        "col.duplicates" => "Duplicates".into(),
        "col.title" => "TITLE".into(),
        "col.artist" => "ARTIST".into(),
        "col.album" => "ALBUM".into(),
        "col.key" => "KEY".into(),
        "col.bpm" => "BPM".into(),
        "col.energy" => "ENERGY".into(),
        "col.time" => "TIME".into(),
        "detail.cues" => "CUE POINTS".into(),
        "detail.add_cue" => "+ Add cue at playhead".into(),
        "detail.add_to_playlist" => "Add to playlist…".into(),
        "detail.show_compatible" => "Show compatible".into(),
        "playlist.new" => "New playlist".into(),
        "playlist.clear" => "← All tracks".into(),
        "prefs.title" => "Preferences".into(),
        "export.title" => "Export library".into(),
        "export.hint" => "Exports the CURRENT VIEW (filters applied).".into(),
        _ => fallback,
    }
}

fn es(key: &str) -> String {
    match key {
        "menu.file" => "Archivo".into(),
        "menu.view" => "Ver".into(),
        "menu.import_folder" => "Importar carpeta…".into(),
        "menu.analyze_pending" => "Analizar pendientes".into(),
        "menu.reanalyze" => "Re-analizar selección".into(),
        "menu.export" => "Exportar…".into(),
        "menu.prefs" => "Preferencias…".into(),
        "menu.about" => "Acerca de".into(),
        "toolbar.search" => "Buscar título, artista, álbum…".into(),
        "toolbar.compat" => "Compatible con".into(),
        "side.collections" => "COLECCIONES".into(),
        "side.playlists" => "PLAYLISTS".into(),
        "col.all" => "Todas las pistas".into(),
        "col.unanalyzed" => "Sin analizar".into(),
        "col.duplicates" => "Duplicados".into(),
        "col.title" => "TÍTULO".into(),
        "col.artist" => "ARTISTA".into(),
        "col.album" => "ÁLBUM".into(),
        "col.key" => "TONALIDAD".into(),
        "col.bpm" => "BPM".into(),
        "col.energy" => "ENERGÍA".into(),
        "col.time" => "DURACIÓN".into(),
        "detail.cues" => "CUE POINTS".into(),
        "detail.add_cue" => "+ Añadir cue en el playhead".into(),
        "detail.add_to_playlist" => "Añadir a playlist…".into(),
        "detail.show_compatible" => "Ver compatibles".into(),
        "playlist.new" => "Nueva playlist".into(),
        "playlist.clear" => "← Todas las pistas".into(),
        "prefs.title" => "Preferencias".into(),
        "export.title" => "Exportar biblioteca".into(),
        "export.hint" => "Exporta la VISTA ACTUAL (con filtros aplicados).".into(),
        _ => key.to_string(),
    }
}
