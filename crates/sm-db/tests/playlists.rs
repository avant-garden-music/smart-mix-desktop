//! Tests F-18 (playlists anidadas + emoji en el nombre) y F-19 (orden
//! manual de pistas): migración v3→v4, `set_playlist_parent` con guard
//! anti-ciclo, re-herencia al borrar carpetas, `reorder_playlist`
//! transaccional y orden por `position` en las consultas.

use sm_db::{LibraryDb, LibraryQuery};
use sm_model::TrackMeta;

fn meta(path: &str, title: &str) -> TrackMeta {
    TrackMeta {
        path: path.into(),
        title: title.into(),
        artist: format!("Artist of {title}"),
        album: String::new(),
        genre: String::new(),
        duration_s: 180.0,
        format: "mp3".into(),
        file_size: 1_000_000,
        file_modified_unix: 1_700_000_000,
        ..Default::default()
    }
}

fn db() -> LibraryDb {
    LibraryDb::open_in_memory().expect("abriendo db en memoria")
}

// ---------------------------------------------------------------------------
// F-18: playlists anidadas
// ---------------------------------------------------------------------------

#[test]
fn set_playlist_parent_anida_y_desanida() {
    let db = db();
    let root = db.create_playlist("Gigas", "manual", None).unwrap();
    let sub = db.create_playlist("Warmup", "manual", None).unwrap();

    // por defecto todas las playlists son raíz
    assert_eq!(
        db.playlists()
            .unwrap()
            .iter()
            .find(|p| p.id == sub.id)
            .unwrap()
            .parent_id,
        None
    );

    db.set_playlist_parent(sub.id, Some(root.id)).unwrap();
    let got = db
        .playlists()
        .unwrap()
        .iter()
        .find(|p| p.id == sub.id)
        .unwrap()
        .clone();
    assert_eq!(got.parent_id, Some(root.id), "sub queda anidada");
    assert_eq!(
        db.playlists()
            .unwrap()
            .iter()
            .find(|p| p.id == root.id)
            .unwrap()
            .parent_id,
        None,
        "la raíz no cambia"
    );

    // des-anidar → vuelve a raíz
    db.set_playlist_parent(sub.id, None).unwrap();
    assert_eq!(
        db.playlists()
            .unwrap()
            .iter()
            .find(|p| p.id == sub.id)
            .unwrap()
            .parent_id,
        None
    );
}

#[test]
fn set_playlist_parent_rechaza_ciclos() {
    let db = db();
    let a = db.create_playlist("A", "manual", None).unwrap();
    let b = db.create_playlist("B", "manual", None).unwrap();
    let c = db.create_playlist("C", "manual", None).unwrap();
    // A → B → C
    db.set_playlist_parent(b.id, Some(a.id)).unwrap();
    db.set_playlist_parent(c.id, Some(b.id)).unwrap();

    // auto-referencia
    assert!(
        db.set_playlist_parent(a.id, Some(a.id)).is_err(),
        "uno no puede ser su propio padre"
    );
    // ciclo directo: A bajo B (B ya desciende de A)
    assert!(
        db.set_playlist_parent(a.id, Some(b.id)).is_err(),
        "ciclo directo rechazado"
    );
    // ciclo indirecto: A bajo C (C desciende de B que desciende de A)
    assert!(
        db.set_playlist_parent(a.id, Some(c.id)).is_err(),
        "ciclo indirecto rechazado"
    );
    // el estado quedó intacto tras los rechazos
    let all = db.playlists().unwrap();
    assert_eq!(all.iter().find(|p| p.id == a.id).unwrap().parent_id, None);
    assert_eq!(
        all.iter().find(|p| p.id == b.id).unwrap().parent_id,
        Some(a.id)
    );
    assert_eq!(
        all.iter().find(|p| p.id == c.id).unwrap().parent_id,
        Some(b.id)
    );

    // re-colgar un subárbol válido sí funciona: C bajo A
    db.set_playlist_parent(c.id, Some(a.id)).unwrap();
    assert_eq!(
        db.playlists()
            .unwrap()
            .iter()
            .find(|p| p.id == c.id)
            .unwrap()
            .parent_id,
        Some(a.id)
    );
}

#[test]
fn set_playlist_parent_valida_existencia() {
    let db = db();
    let pl = db.create_playlist("P", "manual", None).unwrap();
    assert!(
        db.set_playlist_parent(999, Some(pl.id)).is_err(),
        "playlist inexistente"
    );
    assert!(
        db.set_playlist_parent(pl.id, Some(999)).is_err(),
        "padre inexistente"
    );
    // limpiar la raíz de una playlist inexistente también es error
    assert!(db.set_playlist_parent(999, None).is_err());
}

#[test]
fn borrar_carpeta_rehereda_hijos_al_abuelo() {
    let db = db();
    let gig = db.create_playlist("Gig", "manual", None).unwrap();
    let warm = db.create_playlist("Warmup", "manual", None).unwrap();
    let open = db.create_playlist("Opener", "manual", None).unwrap();
    db.set_playlist_parent(warm.id, Some(gig.id)).unwrap();
    db.set_playlist_parent(open.id, Some(warm.id)).unwrap();

    // borrar la carpeta intermedia: sus hijos suben al padre de la borrada
    db.delete_playlist(warm.id).unwrap();
    let all = db.playlists().unwrap();
    assert_eq!(all.len(), 2, "solo quedan Gig y Opener");
    assert_eq!(
        all.iter().find(|p| p.id == open.id).unwrap().parent_id,
        Some(gig.id),
        "el hijo sube al abuelo, no queda huérfano"
    );

    // borrar una raíz con hijos: los hijos suben a raíz
    db.delete_playlist(gig.id).unwrap();
    let all = db.playlists().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(
        all.iter().find(|p| p.id == open.id).unwrap().parent_id,
        None
    );
}

#[test]
fn nombre_de_playlist_con_emoji_roundtrip() {
    let db = db();
    let nombre = "\u{1F525} Verano"; // fuego + texto
    let pl = db.create_playlist(nombre, "manual", None).unwrap();
    assert_eq!(
        pl.name, nombre,
        "create_playlist devuelve el nombre intacto"
    );
    let all = db.playlists().unwrap();
    assert_eq!(
        all.iter().find(|p| p.id == pl.id).unwrap().name,
        nombre,
        "persistido intacto"
    );

    // rename con emoji también
    db.rename_playlist(pl.id, "\u{1F3B6} Noche").unwrap();
    assert_eq!(db.playlists().unwrap()[0].name, "\u{1F3B6} Noche");
}

/// BD creada con el esquema v3 (SIN columna parent_id en playlists):
/// `open()` debe migrarla a v4 (ALTER TABLE + user_version=4) conservando
/// las playlists existentes.
#[test]
fn migra_bd_v3_sin_parent_id() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("sm-db-v4-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v3.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE tracks (
                id                  INTEGER PRIMARY KEY,
                path                TEXT NOT NULL UNIQUE,
                title               TEXT NOT NULL DEFAULT '',
                artist              TEXT NOT NULL DEFAULT '',
                album               TEXT NOT NULL DEFAULT '',
                genre               TEXT NOT NULL DEFAULT '',
                year                INTEGER,
                track_no            INTEGER,
                duration_s          REAL NOT NULL DEFAULT 0.0,
                bitrate_kbps        INTEGER,
                sample_rate         INTEGER,
                channels            INTEGER,
                format              TEXT NOT NULL DEFAULT '',
                file_size           INTEGER NOT NULL DEFAULT 0,
                file_modified_unix  INTEGER NOT NULL DEFAULT 0,
                added_at_unix       INTEGER NOT NULL DEFAULT 0,
                key_camelot         TEXT,
                bpm                 REAL,
                energy              REAL,
                analysis_status     TEXT NOT NULL DEFAULT 'pending',
                rating              INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE playlists (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL,
                kind        TEXT NOT NULL DEFAULT 'manual',
                filter_json TEXT
            );
            CREATE TABLE playlist_tracks (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                position    INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, track_id)
            );
            PRAGMA user_version = 3;
            INSERT INTO playlists (name, kind) VALUES ('Legacy', 'manual');
            "#,
        )
        .unwrap();
    }
    let db = LibraryDb::open(&path).unwrap();
    // la playlist v3 sobrevive y es raíz (parent_id NULL → None)
    let all = db.playlists().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "Legacy");
    assert_eq!(all[0].parent_id, None, "migración: parent_id default NULL");
    // la columna nueva es utilizable
    let sub = db.create_playlist("Nueva", "manual", None).unwrap();
    db.set_playlist_parent(sub.id, Some(all[0].id)).unwrap();
    assert_eq!(
        db.playlists()
            .unwrap()
            .iter()
            .find(|p| p.id == sub.id)
            .unwrap()
            .parent_id,
        Some(all[0].id)
    );
    // reabrir: v4 ya aplicada, idempotente, datos intactos
    drop(db);
    let db2 = LibraryDb::open(&path).unwrap();
    assert_eq!(db2.playlists().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// F-19: orden manual de pistas
// ---------------------------------------------------------------------------

fn pl_con_3(db: &LibraryDb) -> (i64, [i64; 3]) {
    let a = db.upsert_track(&meta("C:/o1.mp3", "One")).unwrap();
    let b = db.upsert_track(&meta("C:/o2.mp3", "Two")).unwrap();
    let c = db.upsert_track(&meta("C:/o3.mp3", "Three")).unwrap();
    let pl = db.create_playlist("Orden", "manual", None).unwrap();
    for t in [a, b, c] {
        db.add_track_to_playlist(pl.id, t).unwrap();
    }
    (pl.id, [a, b, c])
}

#[test]
fn reorder_playlist_persiste_el_orden() {
    let db = db();
    let (pid, [a, b, c]) = pl_con_3(&db);
    assert_eq!(
        db.playlist_track_ids(pid).unwrap(),
        vec![a, b, c],
        "orden de inserción"
    );

    // [b, c, a]: mover la última al principio
    db.reorder_playlist(pid, &[c, a, b]).unwrap();
    assert_eq!(db.playlist_track_ids(pid).unwrap(), vec![c, a, b]);

    // la query por playlist SIEMPRE sigue position (fallback id)
    let q = LibraryQuery {
        playlist_id: Some(pid),
        limit: 0,
        ..Default::default()
    };
    let ids: Vec<i64> = db.query(&q).unwrap().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![c, a, b], "query respeta el orden manual");
    assert_eq!(db.count(&q).unwrap(), 3);

    // reorder inverso completo
    db.reorder_playlist(pid, &[b, a, c]).unwrap();
    assert_eq!(db.playlist_track_ids(pid).unwrap(), vec![b, a, c]);

    // vacía es válida (permutación de conjunto vacío)
    let pl2 = db.create_playlist("Vacia", "manual", None).unwrap();
    db.reorder_playlist(pl2.id, &[]).unwrap();
    assert!(db.playlist_track_ids(pl2.id).unwrap().is_empty());
}

#[test]
fn reorder_playlist_rechaza_ids_ajenos_sin_mutar() {
    let db = db();
    let (pid, [a, b, c]) = pl_con_3(&db);
    let outsider = db.upsert_track(&meta("C:/out.mp3", "Out")).unwrap();

    // id que no está en la playlist → error y nada cambia
    assert!(
        db.reorder_playlist(pid, &[a, b, outsider]).is_err(),
        "id ajeno rechazado"
    );
    // omite una pista miembro → error (debe ser una permutación completa)
    assert!(
        db.reorder_playlist(pid, &[a, b]).is_err(),
        "subset rechazado"
    );
    // duplicado → error
    assert!(
        db.reorder_playlist(pid, &[a, a, b]).is_err(),
        "duplicado rechazado"
    );
    // playlist inexistente → error
    assert!(
        db.reorder_playlist(999, &[a, b, c]).is_err(),
        "playlist inexistente"
    );

    // tras todos los rechazos el orden original quedó intacto
    assert_eq!(db.playlist_track_ids(pid).unwrap(), vec![a, b, c]);
}

#[test]
fn remove_y_add_tras_reorder_conservan_coherencia() {
    let db = db();
    let (pid, [a, b, c]) = pl_con_3(&db);
    db.reorder_playlist(pid, &[c, b, a]).unwrap();
    // quitar la del medio mantiene la secuencia relativa
    db.remove_track_from_playlist(pid, b).unwrap();
    assert_eq!(db.playlist_track_ids(pid).unwrap(), vec![c, a]);
    // re-añadir va al FINAL aunque antes estuviera en medio
    db.add_track_to_playlist(pid, b).unwrap();
    assert_eq!(db.playlist_track_ids(pid).unwrap(), vec![c, a, b]);
}
