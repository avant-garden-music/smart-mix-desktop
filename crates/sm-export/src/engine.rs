//! Librería Engine DJ (Denon Prime) nativa: USB EngineLibrary NUEVA,
//! schema v2.20.1 — port fiel de `core/tagger/engine_dj.py`.
//!
//! El .py genera bases **SQLite**, no carpetas/CSV: en `out_root` crea
//!
//! ```text
//! out_root/EngineLibrary/m.db   base v2.20.1 que leen players/software
//! out_root/Database2/m.db       copia idéntica (layout legacy de escritorio)
//! ```
//!
//! El DDL es el set COMPLETO de objetos (tablas, índices, triggers y
//! vistas) verbatim de libdjinterop `schema_2_20_1.cpp`; Engine DJ
//! rechaza bases cuyo schema difiera. Semillas documentadas: AlbumArt
//! fila 1 ('sin artwork') e Information con uuid v4 + versión 2.20.1.
//!
//! Por pista (columnas del INSERT, orden de `_TRACK_COLUMNS` del .py):
//! path relativo a `EngineLibrary/` (`../Music/a.mp3`, siempre `/`;
//! absoluto si la pista vive fuera de la raíz), filename, título
//! (fallback: stem), artista, BPM (`bpm` entero + `bpmAnalyzed` REAL),
//! key INTEGER 0-23 (círculo de quintas alternando mayor/menor),
//! energía → `rating` 0-100 (energy×10) y `comment` con el digest
//! completo + marca de software.
//!
//! Deltas documentados respecto al .py (los datos ya viajan en
//! `ExportEntry`): `length` lleva la duración en segundos enteros (el .py
//! escribía 0 porque su contrato de entrada no incluía duración) y
//! `album`/`genre` se pueblan de la entrada (el .py los dejaba vacíos).
//!
//! `INSERT OR IGNORE` + UNIQUE(path) → idempotente por path; el destino se
//! rechaza si ya existe una base (no hay merge en v1).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, Connection};

use crate::values::{camelot_to_musical, engine_key, entry_comment, file_name_of, file_stem_of, ext_lower};
use crate::ExportEntry;

/// Versión del schema que declara la fila Information.
pub const SCHEMA_VERSION: (i64, i64, i64) = (2, 20, 1);

/// DDL del schema Engine v2.20.1, verbatim de libdjinterop
/// (src/djinterop/engine/schema/schema_2_20_1.cpp, rama main, LGPL-3.0),
/// tal como lo copia `SCHEMA_DDL` en engine_dj.py.
const SCHEMA_DDL: &str = r#"
    CREATE TABLE Information (id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT, schemaVersionMajor INTEGER, schemaVersionMinor INTEGER, schemaVersionPatch INTEGER, currentPlayedIndiciator INTEGER, lastRekordBoxLibraryImportReadCounter INTEGER);
    CREATE TABLE Track (id INTEGER PRIMARY KEY AUTOINCREMENT, playOrder INTEGER, length INTEGER, bpm INTEGER, year INTEGER, path TEXT, filename TEXT, bitrate INTEGER, bpmAnalyzed REAL, albumArtId INTEGER, fileBytes INTEGER, title TEXT, artist TEXT, album TEXT, genre TEXT, comment TEXT, label TEXT, composer TEXT, remixer TEXT, key INTEGER, rating INTEGER, albumArt TEXT, timeLastPlayed DATETIME, isPlayed BOOLEAN, fileType TEXT, isAnalyzed BOOLEAN, dateCreated DATETIME, dateAdded DATETIME, isAvailable BOOLEAN, isMetadataOfPackedTrackChanged BOOLEAN, isPerfomanceDataOfPackedTrackChanged BOOLEAN, playedIndicator INTEGER, isMetadataImported BOOLEAN, pdbImportKey INTEGER, streamingSource TEXT, uri TEXT, isBeatGridLocked BOOLEAN, originDatabaseUuid TEXT, originTrackId INTEGER, trackData BLOB, overviewWaveFormData BLOB, beatData BLOB, quickCues BLOB, loops BLOB, thirdPartySourceId INTEGER, streamingFlags INTEGER, explicitLyrics BOOLEAN, activeOnLoadLoops INTEGER, CONSTRAINT C_originDatabaseUuid_originTrackId UNIQUE (originDatabaseUuid, originTrackId), CONSTRAINT C_path UNIQUE (path), FOREIGN KEY (albumArtId) REFERENCES AlbumArt (id) ON DELETE RESTRICT);
    CREATE TABLE ChangeLog (id INTEGER PRIMARY KEY AUTOINCREMENT, trackId INTEGER, FOREIGN KEY (trackId) REFERENCES Track (id) ON DELETE SET NULL);
    CREATE TABLE AlbumArt (id INTEGER PRIMARY KEY AUTOINCREMENT, hash TEXT, albumArt BLOB);
    CREATE TABLE Pack (id INTEGER PRIMARY KEY AUTOINCREMENT, packId TEXT, changeLogDatabaseUuid TEXT, changeLogId INTEGER);
    CREATE TABLE PlaylistEntity (id INTEGER PRIMARY KEY AUTOINCREMENT, listId INTEGER, trackId INTEGER, databaseUuid TEXT, nextEntityId INTEGER, membershipReference INTEGER, CONSTRAINT C_NAME_UNIQUE_FOR_LIST UNIQUE (listId, databaseUuid, trackId), FOREIGN KEY (listId) REFERENCES Playlist (id) ON DELETE CASCADE);
    CREATE TABLE Playlist (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, parentListId INTEGER, isPersisted BOOLEAN, nextListId INTEGER, lastEditTime DATETIME, isExplicitlyExported BOOLEAN, CONSTRAINT C_NAME_UNIQUE_FOR_PARENT UNIQUE (title, parentListId), CONSTRAINT C_NEXT_LIST_ID_UNIQUE_FOR_PARENT UNIQUE (parentListId, nextListId));
    CREATE TABLE PreparelistEntity (id INTEGER PRIMARY KEY AUTOINCREMENT, trackId INTEGER, trackNumber INTEGER, FOREIGN KEY (trackId) REFERENCES Track (id) ON DELETE CASCADE);
    CREATE INDEX index_Track_filename ON Track (filename);
    CREATE INDEX index_Track_albumArtId ON Track (albumArtId);
    CREATE INDEX index_Track_uri ON Track (uri);
    CREATE INDEX index_Track_title ON Track(title);
    CREATE INDEX index_Track_length ON Track(length) ;
    CREATE INDEX index_Track_rating ON Track(rating);
    CREATE INDEX index_Track_year ON Track(year);
    CREATE INDEX index_Track_dateAdded ON Track(dateAdded);
    CREATE INDEX index_Track_genre ON Track(genre);
    CREATE INDEX index_Track_artist ON Track(artist);
    CREATE INDEX index_Track_album ON Track(album);
    CREATE INDEX index_Track_key ON Track(key);
    CREATE INDEX index_Track_bpmAnalyzed ON Track(bpmAnalyzed);
    CREATE TRIGGER trigger_after_insert_Track_check_id AFTER INSERT ON Track WHEN NEW.id <= (SELECT seq FROM sqlite_sequence WHERE name = 'Track') BEGIN SELECT RAISE(ABORT, 'Recycling deleted track id''s are not allowed'); END;
    CREATE TRIGGER trigger_after_update_Track_check_Id BEFORE UPDATE ON Track WHEN NEW.id <> OLD.id BEGIN SELECT RAISE(ABORT, 'Changing track id''s are not allowed'); END;
    CREATE TRIGGER trigger_after_insert_Track_fix_origin AFTER INSERT ON Track WHEN IFNULL(NEW.originTrackId, 0) = 0 OR IFNULL(NEW.originDatabaseUuid, '') = '' BEGIN UPDATE Track SET originTrackId = NEW.id, originDatabaseUuid = (SELECT uuid FROM Information) WHERE track.id = NEW.id; END;
    CREATE TRIGGER trigger_after_update_Track_fix_origin AFTER UPDATE ON Track WHEN IFNULL(NEW.originTrackId, 0) = 0 OR IFNULL(NEW.originDatabaseUuid, '') = '' BEGIN UPDATE Track SET originTrackId = NEW.id, originDatabaseUuid = (SELECT uuid FROM Information) WHERE track.id = NEW.id; END;
    CREATE TRIGGER trigger_after_update_Track AFTER UPDATE ON Track FOR EACH ROW BEGIN INSERT INTO ChangeLog (trackId) VALUES(NEW.id); END;
    CREATE INDEX index_AlbumArt_hash ON AlbumArt (hash);
    CREATE INDEX index_PlaylistEntity_nextEntityId_listId ON PlaylistEntity(nextEntityId, listId);
    CREATE TRIGGER trigger_before_insert_List BEFORE INSERT ON Playlist FOR EACH ROW BEGIN UPDATE Playlist SET nextListId = -(1 + nextListId) WHERE nextListId = NEW.nextListId AND parentListId = NEW.parentListId; END;
    CREATE TRIGGER trigger_after_insert_List AFTER INSERT ON Playlist FOR EACH ROW BEGIN UPDATE Playlist SET nextListId = NEW.id WHERE nextListId = -(1 + NEW.nextListId) AND parentListId = NEW.parentListId; END;
    CREATE TRIGGER trigger_after_delete_List AFTER DELETE ON Playlist FOR EACH ROW BEGIN UPDATE Playlist SET nextListId = OLD.nextListId WHERE nextListId = OLD.id; DELETE FROM Playlist WHERE parentListId = OLD.id; END;
    CREATE TRIGGER trigger_after_update_isPersistParent AFTER UPDATE ON Playlist WHEN (old.isPersisted = 0 AND new.isPersisted = 1) OR (old.parentListId != new.parentListId AND new.isPersisted = 1) BEGIN UPDATE Playlist SET isPersisted = 1 WHERE id IN (SELECT parentListId FROM PlaylistAllParent WHERE id=new.id); END;
    CREATE TRIGGER trigger_after_update_isPersistChild AFTER UPDATE ON Playlist WHEN old.isPersisted = 1 AND new.isPersisted = 0 BEGIN UPDATE Playlist SET isPersisted = 0 WHERE id IN (SELECT childListId FROM PlaylistAllChildren WHERE id=new.id); END;
    CREATE TRIGGER trigger_after_insert_isPersist AFTER INSERT ON Playlist WHEN new.isPersisted = 1 BEGIN UPDATE Playlist SET isPersisted = 1 WHERE id IN (SELECT parentListId FROM PlaylistAllParent WHERE id=new.id); END;
    CREATE VIEW PlaylistPath AS WITH RECURSIVE Heirarchy AS (SELECT id AS child, parentListId AS parent, title AS name, 1 AS depth FROM Playlist UNION ALL SELECT child, parentListId AS parent, title AS name, h.depth + 1 AS depth FROM Playlist c JOIN Heirarchy h ON h.parent = c.id ORDER BY depth DESC), OrderedList AS (SELECT id , nextListId, 1 AS position FROM Playlist WHERE nextListId = 0 UNION ALL SELECT c.id , c.nextListId , l.position + 1 FROM Playlist c INNER JOIN OrderedList l ON c.nextListId = l.id), NameConcat AS (SELECT child AS id, GROUP_CONCAT(name ,';') || ';' AS path FROM (SELECT child, name FROM Heirarchy ORDER BY depth DESC) GROUP BY child) SELECT id, path, ROW_NUMBER() OVER (ORDER BY (SELECT COUNT(*) FROM (SELECT * FROM Heirarchy WHERE child = id)) DESC, (SELECT position FROM OrderedList ol WHERE ol.id = c.id) ASC) AS position FROM Playlist c LEFT JOIN NameConcat g USING (id);
    CREATE VIEW PlaylistAllParent AS WITH FindAllParent AS (SELECT id, parentListId FROM Playlist UNION ALL SELECT recursiveCTE.id, Plist.parentListId FROM Playlist Plist INNER JOIN FindAllParent recursiveCTE ON recursiveCTE.parentListId = Plist.id) SELECT * FROM FindAllParent;
    CREATE VIEW PlaylistAllChildren AS WITH FindAllChild AS (SELECT id, id as childListId FROM Playlist UNION ALL SELECT recursiveCTE.id, Plist.id FROM Playlist Plist INNER JOIN FindAllChild recursiveCTE ON recursiveCTE.childListId = Plist.parentListId) SELECT * FROM FindAllChild WHERE id <> childListId;
    CREATE TRIGGER trigger_before_delete_PlaylistEntity BEFORE DELETE ON PlaylistEntity WHEN OLD.trackId > 0 BEGIN UPDATE PlaylistEntity SET nextEntityId = OLD.nextEntityId WHERE nextEntityId = OLD.id AND listId = OLD.listId; END;
    CREATE INDEX index_PreparelistEntity_trackId ON PreparelistEntity (trackId);
    CREATE VIEW PerformanceData AS SELECT id AS trackId, isAnalyzed, trackData, overviewWaveFormData, beatData, quickCues, loops, thirdPartySourceId, activeOnLoadLoops FROM Track;
    CREATE TRIGGER trigger_instead_insert_PerformanceData INSTEAD OF INSERT ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET isAnalyzed = NEW.isAnalyzed, trackData = NEW.trackData, overviewWaveFormData = NEW.overviewWaveFormData, beatData = NEW.beatData, quickCues = NEW.quickCues, loops = NEW.loops, thirdPartySourceId = NEW.thirdPartySourceId, activeOnLoadLoops = NEW.activeOnLoadLoops WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_isAnalyzed_PerformanceData INSTEAD OF UPDATE OF isAnalyzed ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET isAnalyzed = NEW.isAnalyzed WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_trackData_PerformanceData INSTEAD OF UPDATE OF trackData ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET trackData = NEW.trackData WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_overviewWaveFormData_PerformanceData INSTEAD OF UPDATE OF overviewWaveFormData ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET overviewWaveFormData = NEW.overviewWaveFormData WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_beatData_PerformanceData INSTEAD OF UPDATE OF beatData ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET beatData = NEW.beatData WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_quickCues_PerformanceData INSTEAD OF UPDATE OF quickCues ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET quickCues = NEW.quickCues WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_loops_PerformanceData INSTEAD OF UPDATE OF loops ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET loops = NEW.loops WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_update_thirdPartySourceId_PerformanceData INSTEAD OF UPDATE OF thirdPartySourceId ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET thirdPartySourceId = NEW.thirdPartySourceId WHERE Track.id = NEW.trackId; END;
    CREATE TRIGGER trigger_instead_delete_PerformanceData INSTEAD OF DELETE ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET isAnalyzed = NULL, trackData = NULL, overviewWaveFormData = NULL, beatData = NULL, quickCues = NULL, loops = NULL, thirdPartySourceId = NULL WHERE Track.id = OLD.trackId; END;
    CREATE TRIGGER trigger_instead_update_activeOnLoadLoops_PerformanceData INSTEAD OF UPDATE OF activeOnLoadLoops ON PerformanceData FOR EACH ROW BEGIN UPDATE Track SET activeOnLoadLoops = NEW.activeOnLoadLoops WHERE Track.id = NEW.trackId; END;
"#;

/// Columnas de Track que se escriben, en el orden del INSERT (fuera: id,
/// auto-generado). Verbatim de `_TRACK_COLUMNS` en engine_dj.py (47 columnas).
const TRACK_COLUMNS: [&str; 47] = [
    "playOrder", "length", "bpm", "year",
    "path", "filename", "bitrate", "bpmAnalyzed",
    "albumArtId", "fileBytes",
    "title", "artist", "album", "genre", "comment",
    "label", "composer", "remixer",
    "key", "rating",
    "albumArt", "timeLastPlayed", "isPlayed",
    "fileType", "isAnalyzed",
    "dateCreated", "dateAdded", "isAvailable",
    "isMetadataOfPackedTrackChanged", "isPerfomanceDataOfPackedTrackChanged",
    "playedIndicator", "isMetadataImported",
    "pdbImportKey", "streamingSource", "uri",
    "isBeatGridLocked", "originDatabaseUuid", "originTrackId",
    "trackData", "overviewWaveFormData", "beatData", "quickCues", "loops",
    "thirdPartySourceId", "streamingFlags", "explicitLyrics",
    "activeOnLoadLoops",
];

fn insert_sql() -> String {
    let cols = TRACK_COLUMNS.join(", ");
    let ph = vec!["?"; TRACK_COLUMNS.len()].join(", ");
    format!("INSERT OR IGNORE INTO Track ({cols}) VALUES ({ph})")
}

/// Crea una EngineLibrary v2 nueva en `out_root` con las pistas de
/// `entries`.
///
/// Rechaza el destino si ya existe `EngineLibrary/m.db` o `Database2/m.db`
/// (no hay merge en v1) y una lista de pistas vacía. Al cerrar, replica
/// `m.db` en `Database2/` (layout legacy del software de escritorio).
pub fn write_engine_dj_library(entries: &[ExportEntry], out_root: impl AsRef<Path>) -> Result<()> {
    if entries.is_empty() {
        bail!("no hay pistas que exportar");
    }
    let root = out_root.as_ref();
    let lib_dir = root.join("EngineLibrary");
    let db_path = lib_dir.join("m.db");
    let database2_path = root.join("Database2").join("m.db");
    if db_path.exists() || database2_path.exists() {
        bail!("ya existe una librería Engine DJ en {}", root.display());
    }
    fs::create_dir_all(&lib_dir)?;
    fs::create_dir_all(root.join("Database2"))?;

    // journal DELETE (sin -wal/-shm en la USB) y sin FK enforcement,
    // como hacen libdjinterop/penbridge al poblar.
    let conn = Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA foreign_keys=OFF;")?;
    conn.execute_batch(SCHEMA_DDL)?;
    // Semilla documentada de libdjinterop schema_2_20_1::create():
    conn.execute("INSERT INTO AlbumArt (id, hash, albumArt) VALUES (1, NULL, NULL)", [])?;
    let uuid = uuid_v4(&conn)?;
    let played_indicator: i64 = conn.query_row("SELECT random()", [], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO Information (uuid, schemaVersionMajor, schemaVersionMinor, \
         schemaVersionPatch, currentPlayedIndiciator, \
         lastRekordBoxLibraryImportReadCounter) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![uuid, SCHEMA_VERSION.0, SCHEMA_VERSION.1, SCHEMA_VERSION.2, played_indicator],
    )?;

    let sql = insert_sql();
    for e in entries {
        let rel = relative_library_path(root, e.path);
        let row = track_row(e, &rel);
        conn.execute(&sql, rusqlite::params_from_iter(row))?;
    }
    drop(conn); // commit+cierre antes de copiar (journal DELETE: m.db autocontenido)

    fs::copy(&db_path, &database2_path)?;
    Ok(())
}

/// Fila de Track alineada con [`TRACK_COLUMNS`] (port de `_row`).
fn track_row(e: &ExportEntry, rel_path: &str) -> Vec<SqlValue> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let file_bytes = std::fs::metadata(e.path).ok().map(|m| m.len() as i64);
    let energy_int = e.energy.map(|x| x.round() as i64);
    let key = engine_key(
        e.key_camelot,
        e.key_name.or_else(|| camelot_to_musical(e.key_camelot.unwrap_or(""))),
    );
    let title = if e.title.is_empty() { file_stem_of(e.path) } else { e.title.to_string() };
    let comment = entry_comment(e);
    let by_col: [(&str, SqlValue); 47] = [
        ("playOrder", SqlValue::Null),
        ("length", SqlValue::Integer(e.duration_s.round() as i64)),
        ("bpm", SqlValue::Integer(e.bpm.map(|b| b.round() as i64).unwrap_or(0))),
        ("year", SqlValue::Null),
        ("path", SqlValue::Text(rel_path.to_string())),
        ("filename", SqlValue::Text(file_name_of(e.path))),
        ("bitrate", SqlValue::Null),
        ("bpmAnalyzed", e.bpm.map(SqlValue::Real).unwrap_or(SqlValue::Null)),
        ("albumArtId", SqlValue::Integer(1)), // fila 1 = 'sin artwork'
        ("fileBytes", file_bytes.map(SqlValue::Integer).unwrap_or(SqlValue::Null)),
        ("title", SqlValue::Text(title)),
        ("artist", SqlValue::Text(e.artist.to_string())),
        ("album", SqlValue::Text(e.album.to_string())),
        ("genre", SqlValue::Text(e.genre.to_string())),
        ("comment", SqlValue::Text(comment)),
        ("label", SqlValue::Text(String::new())),
        ("composer", SqlValue::Text(String::new())),
        ("remixer", SqlValue::Text(String::new())),
        ("key", key.map(SqlValue::Integer).unwrap_or(SqlValue::Null)),
        ("rating", energy_int.map(|x| SqlValue::Integer(x * 10)).unwrap_or(SqlValue::Null)),
        ("albumArt", SqlValue::Null),
        ("timeLastPlayed", SqlValue::Null),
        ("isPlayed", SqlValue::Integer(0)),
        ("fileType", SqlValue::Text(ext_lower(e.path))),
        ("isAnalyzed", SqlValue::Integer(0)), // sin blobs: el player re-analiza
        ("dateCreated", SqlValue::Integer(now)),
        ("dateAdded", SqlValue::Integer(now)),
        ("isAvailable", SqlValue::Integer(1)),
        ("isMetadataOfPackedTrackChanged", SqlValue::Integer(0)),
        ("isPerfomanceDataOfPackedTrackChanged", SqlValue::Integer(0)),
        ("playedIndicator", SqlValue::Integer(0)),
        ("isMetadataImported", SqlValue::Integer(0)),
        ("pdbImportKey", SqlValue::Null),
        ("streamingSource", SqlValue::Null),
        ("uri", SqlValue::Null),
        ("isBeatGridLocked", SqlValue::Integer(0)),
        ("originDatabaseUuid", SqlValue::Null), // los rellena el trigger
        ("originTrackId", SqlValue::Null),
        ("trackData", SqlValue::Null),
        ("overviewWaveFormData", SqlValue::Null),
        ("beatData", SqlValue::Null),
        ("quickCues", SqlValue::Null),
        ("loops", SqlValue::Null),
        ("thirdPartySourceId", SqlValue::Null),
        ("streamingFlags", SqlValue::Integer(0)),
        ("explicitLyrics", SqlValue::Integer(0)),
        ("activeOnLoadLoops", SqlValue::Integer(0)),
    ];
    debug_assert_eq!(by_col.len(), TRACK_COLUMNS.len());
    TRACK_COLUMNS
        .iter()
        .map(|col| {
            by_col
                .iter()
                .find(|(k, _)| k == col)
                .map(|(_, v)| v.clone())
                .expect("columna sin valor")
        })
        .collect()
}

/// Path absoluto → relativo a `EngineLibrary/` con `/` (`'../Music/a.mp3'`).
///
/// Fuera de la raíz de la USB se guarda el path absoluto (misma regla que
/// libdjinterop: un relativo con muchos `../` no resolvería en el player).
/// Los paths de entrada relativos se resuelven contra la raíz.
fn relative_library_path(root: &Path, path: &str) -> String {
    let p = PathBuf::from(path.replace('\\', "/"));
    let abs = if p.is_absolute() {
        p
    } else {
        root.join(p)
    };
    match abs.strip_prefix(root) {
        Ok(rel) => {
            let mut s = String::from("../");
            s.push_str(&rel.to_string_lossy().replace('\\', "/"));
            s
        }
        Err(_) => abs.to_string_lossy().replace('\\', "/"),
    }
}

/// UUID v4 generado con la entropía de `random()` de SQLite (sin dep
/// extra): 128 bits aleatorios con los bits de versión/variante puestos.
fn uuid_v4(conn: &Connection) -> Result<String> {
    let a: i64 = conn.query_row("SELECT random()", [], |r| r.get(0))?;
    let b: i64 = conn.query_row("SELECT random()", [], |r| r.get(0))?;
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..].copy_from_slice(&b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0F) | 0x40; // versión 4
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // variante RFC 4122
    let hex: String = bytes.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_y_sql_alineados() {
        assert_eq!(TRACK_COLUMNS.len(), 47);
        let sql = insert_sql();
        assert!(sql.starts_with("INSERT OR IGNORE INTO Track (playOrder"));
        assert_eq!(sql.matches('?').count(), 47);
    }
}
