//! sm-db — persistencia SQLite de la biblioteca de Smart Mix.
//!
//! Esquema versionado con `PRAGMA user_version`, tabla FTS5
//! (`tracks_fts` sobre título/artista/álbum, sync manual en cada upsert/delete)
//! e índices por `key_camelot`/`bpm`/`energy`/`artist`/`title`.
//!
//! Convenciones:
//! - `duration_s` es una sola columna: la refina `save_analysis` con la
//!   duración medida por decodificación (más exacta que la del encabezado).
//! - Cues y segments se persisten como JSON TEXT, el waveform como BLOB
//!   bincode (tripletes min/max/rms por bucket).
//! - `analysis_status` se guarda como TEXT snake_case (`pending`, `queued`,
//!   `analyzing`, `done`, `failed`), igual que su serialización serde.
//! - Duplicados: grupos "exact" (artist+title normalizados iguales) y
//!   "perceptual" (título similar ≥0.90 + duración ±2 s), taxonomía tomada
//!   de `core/dedupe/detector.py` de harmonic-engine.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use sm_model::{
    camelot, AnalysisStatus, CuePoint, Playlist, Preferences, SmartFilter, TrackAnalysis,
    TrackMeta, TrackRow,
};

/// Versión de esquema actual (PRAGMA user_version).
pub const SCHEMA_VERSION: i64 = 4;

// ---------------------------------------------------------------------------
// Tipos públicos
// ---------------------------------------------------------------------------

/// Pista completa: metadatos + análisis + estado persistido.
#[derive(Debug, Clone)]
pub struct TrackFull {
    pub id: i64,
    pub meta: TrackMeta,
    pub analysis: TrackAnalysis,
    pub status: AnalysisStatus,
    /// F-26: rating 1-5 estrellas (0 = sin rating).
    pub rating: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Title,
    Artist,
    Album,
    Key,
    Bpm,
    Energy,
    Duration,
    AddedAt,
    Rating,
}

/// Consulta de biblioteca (tabla virtualizada, playlists y filtros smart).
#[derive(Debug, Clone)]
pub struct LibraryQuery {
    /// Búsqueda libre FTS5 (prefijo por token: "love" → "love*").
    pub text: Option<String>,
    pub sort: SortField,
    pub desc: bool,
    /// 0 = sin límite.
    pub limit: usize,
    pub offset: u64,
    /// Si está fijado, el resultado va en `position` de la playlist
    /// (ignora `sort`/`desc`).
    pub playlist_id: Option<i64>,
    pub smart: Option<SmartFilter>,
    pub status_filter: Option<AnalysisStatus>,
}

impl Default for LibraryQuery {
    fn default() -> Self {
        Self {
            text: None,
            sort: SortField::AddedAt,
            desc: true,
            limit: 100,
            offset: 0,
            playlist_id: None,
            smart: None,
            status_filter: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryStats {
    pub total: i64,
    pub analyzed: i64,
    pub pending: i64,
    pub failed: i64,
}

/// Grupo de duplicados: `(id, path, duration_s)` por pista.
#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateGroup {
    pub reason: String,
    pub tracks: Vec<(i64, String, f64)>,
}

// ---------------------------------------------------------------------------
// Esquema
// ---------------------------------------------------------------------------

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS tracks (
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
    -- F-26: rating 1-5 estrellas (0 = sin rating)
    rating              INTEGER NOT NULL DEFAULT 0,
    -- columnas de análisis (escritas por save_analysis)
    key_camelot         TEXT,
    key_name            TEXT,
    key_confidence      REAL,
    second_key          TEXT,
    second_confidence   REAL,
    bpm                 REAL,
    energy              REAL,
    volume_db           REAL,
    lufs                REAL,
    clipped             INTEGER,
    waveform            BLOB,
    waveform_bands      BLOB,
    segments_json       TEXT,
    cues_json           TEXT,
    beatgrid_json       TEXT,
    analysis_status     TEXT NOT NULL DEFAULT 'pending',
    analyzed_at_unix    INTEGER,
    analysis_ms         INTEGER,
    cover               BLOB
);

CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(title, artist, album);

CREATE INDEX IF NOT EXISTS idx_tracks_key    ON tracks(key_camelot);
CREATE INDEX IF NOT EXISTS idx_tracks_bpm    ON tracks(bpm);
CREATE INDEX IF NOT EXISTS idx_tracks_energy ON tracks(energy);
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_tracks_title  ON tracks(title COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS playlists (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'manual',
    filter_json TEXT,
    -- F-18: playlist padre (anidamiento); NULL = raíz. Sin FK: el borrado
    -- re-hereda (delete_playlist) en vez de caer en cascada.
    parent_id   INTEGER
);
CREATE INDEX IF NOT EXISTS idx_playlists_parent ON playlists(parent_id);

CREATE TABLE IF NOT EXISTS playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, track_id)
);
CREATE INDEX IF NOT EXISTS idx_playlist_tracks_pos ON playlist_tracks(playlist_id, position);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

// ---------------------------------------------------------------------------
// LibraryDb
// ---------------------------------------------------------------------------

pub struct LibraryDb {
    conn: Connection,
}

impl LibraryDb {
    /// Abre (o crea) la biblioteca en `path` y aplica migraciones pendientes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn =
            Connection::open(path).with_context(|| format!("abriendo {}", path.display()))?;
        Self::init(conn)
    }

    /// Biblioteca efímera en memoria (tests y benchmarks).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v > SCHEMA_VERSION {
            bail!("base creada por una versión más nueva (user_version={v})");
        }
        if v < SCHEMA_VERSION {
            if v < 2 {
                // v2: waveform espectral de 3 bandas (backfill aparte)
                let _ = conn.execute("ALTER TABLE tracks ADD COLUMN waveform_bands BLOB", []);
            }
            if v < 3 {
                // v3 (F-26): rating 1-5; `let _` porque en BD recién creada
                // (v0) la columna ya viene en SCHEMA_SQL
                let _ = conn.execute(
                    "ALTER TABLE tracks ADD COLUMN rating INTEGER NOT NULL DEFAULT 0",
                    [],
                );
            }
            if v < 4 {
                // v4 (F-18): playlists anidadas; `let _` porque en BD recién
                // creada (v0) la columna ya viene en SCHEMA_SQL
                let _ = conn.execute("ALTER TABLE playlists ADD COLUMN parent_id INTEGER", []);
                let _ = conn.execute(
                    "CREATE INDEX IF NOT EXISTS idx_playlists_parent ON playlists(parent_id)",
                    [],
                );
            }
            conn.execute_batch(SCHEMA_SQL)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Self { conn })
    }

    /// Transacción explícita para importaciones masivas: los métodos de
    /// `&self` ejecutan dentro de ella hasta el `commit()`.
    pub fn begin(&self) -> Result<Transaction<'_>> {
        Ok(self.conn.unchecked_transaction()?)
    }

    // -- CRUD de pistas ------------------------------------------------------

    /// Inserta o actualiza por `path` UNIQUE; refresca metadatos de archivo
    /// (tamaño/mtime y tags) sin tocar el análisis ya guardado. Devuelve el id.
    pub fn upsert_track(&self, meta: &TrackMeta) -> Result<i64> {
        let now = chrono::Utc::now().timestamp();
        // (id, mtime) en una sola consulta —auditoría 2026-11-10; antes
        // eran dos SELECT por pista (id y luego mtime). La columna es
        // NOT NULL DEFAULT 0: mismos valores que el unwrap_or(0) anterior.
        let existing: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, file_modified_unix FROM tracks WHERE path = ?1",
                params![meta.path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let id = match existing {
            Some((id, old_mtime)) => {
                // F-11: si el archivo cambio (mtime distinto), el analisis
                // guardado es stale -> invalidarlo completo y devolver la
                // pista a la cola (Pending). Con mtime igual se conserva.
                if old_mtime != 0 && old_mtime != meta.file_modified_unix {
                    self.conn.execute(
                        "UPDATE tracks SET
                            key_camelot=NULL, key_name=NULL, key_confidence=NULL,
                            second_key=NULL, second_confidence=NULL,
                            bpm=NULL, energy=NULL, volume_db=NULL, lufs=NULL,
                            clipped=NULL, waveform=NULL, waveform_bands=NULL,
                            segments_json=NULL, cues_json=NULL, beatgrid_json=NULL,
                            analyzed_at_unix=NULL, analysis_ms=NULL,
                            analysis_status='pending'
                         WHERE id=?1",
                        params![id],
                    )?;
                }
                self.conn.execute(
                    "UPDATE tracks SET title=?1, artist=?2, album=?3, genre=?4, year=?5,
                        track_no=?6, duration_s=?7, bitrate_kbps=?8, sample_rate=?9,
                        channels=?10, format=?11, file_size=?12, file_modified_unix=?13
                     WHERE id=?14",
                    params![
                        meta.title,
                        meta.artist,
                        meta.album,
                        meta.genre,
                        meta.year,
                        meta.track_no,
                        meta.duration_s,
                        meta.bitrate_kbps,
                        meta.sample_rate,
                        meta.channels,
                        meta.format,
                        meta.file_size as i64,
                        meta.file_modified_unix,
                        id
                    ],
                )?;
                // re-sync FTS (título/artista/álbum pudieron cambiar)
                self.conn
                    .execute("DELETE FROM tracks_fts WHERE rowid = ?1", params![id])?;
                id
            }
            None => {
                self.conn.execute(
                    "INSERT INTO tracks (path, title, artist, album, genre, year, track_no,
                        duration_s, bitrate_kbps, sample_rate, channels, format, file_size,
                        file_modified_unix, added_at_unix)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                    params![
                        meta.path,
                        meta.title,
                        meta.artist,
                        meta.album,
                        meta.genre,
                        meta.year,
                        meta.track_no,
                        meta.duration_s,
                        meta.bitrate_kbps,
                        meta.sample_rate,
                        meta.channels,
                        meta.format,
                        meta.file_size as i64,
                        meta.file_modified_unix,
                        now
                    ],
                )?;
                self.conn.last_insert_rowid()
            }
        };
        self.conn.execute(
            "INSERT INTO tracks_fts (rowid, title, artist, album) VALUES (?1,?2,?3,?4)",
            params![id, meta.title, meta.artist, meta.album],
        )?;
        Ok(id)
    }

    /// Portada embebida (BLOB) o `None` para limpiar.
    pub fn set_cover(&self, track_id: i64, cover: Option<Vec<u8>>) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE tracks SET cover=?1 WHERE id=?2",
            params![cover, track_id],
        )?;
        ensure_track(n, track_id, "set_cover")
    }

    /// F-26: fija el rating (0-5 estrellas; valores >5 se recortan a 5).
    pub fn set_rating(&self, track_id: i64, rating: u8) -> Result<()> {
        let r = rating.clamp(0, 5) as i64;
        let n = self.conn.execute(
            "UPDATE tracks SET rating=?1 WHERE id=?2",
            params![r, track_id],
        )?;
        ensure_track(n, track_id, "set_rating")
    }

    /// F-26: rating actual (consulta ligera). Error si la pista no existe.
    pub fn rating(&self, track_id: i64) -> Result<u8> {
        let v: Option<i64> = self
            .conn
            .query_row(
                "SELECT rating FROM tracks WHERE id=?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?;
        match v {
            Some(r) => Ok(r.clamp(0, 5) as u8),
            None => {
                track_missing(track_id, "rating")?;
                unreachable!()
            }
        }
    }

    /// Cierre: fija la energía de la pista (escala 1-10; valores fuera de
    /// rango se recortan). `None` limpia el valor. Es un UPDATE de la
    /// columna `energy` del análisis — el esquema NO cambia. Una
    /// re-análisis posterior vuelve a escribir su propia energía.
    pub fn set_energy(&self, track_id: i64, energy: Option<u8>) -> Result<()> {
        let e = energy.map(|e| e.clamp(1, 10) as f64);
        let n = self.conn.execute(
            "UPDATE tracks SET energy=?1 WHERE id=?2",
            params![e, track_id],
        )?;
        ensure_track(n, track_id, "set_energy")
    }

    /// Persiste el análisis completo: columnas planas para key/bpm/energy/
    /// volumen, JSON para cues/segments/beatgrid, bincode BLOB para waveform.
    /// Marca `analysis_status = done` y `analyzed_at = now`.
    pub fn save_analysis(&self, track_id: i64, a: &TrackAnalysis) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let waveform = bincode::serialize(&a.waveform).context("serializando waveform")?;
        let waveform_bands = match &a.waveform_bands {
            Some(b) => Some(bincode::serialize(b).context("serializando waveform_bands")?),
            None => None,
        };
        let segments = serde_json::to_string(&a.segments).context("serializando segments")?;
        let cues = serde_json::to_string(&a.cues).context("serializando cues")?;
        let beatgrid = match &a.beatgrid {
            Some(b) => Some(serde_json::to_string(b).context("serializando beatgrid")?),
            None => None,
        };
        let n = self.conn.execute(
            "UPDATE tracks SET
                key_camelot=?1, key_name=?2, key_confidence=?3,
                second_key=?4, second_confidence=?5,
                bpm=?6, energy=?7, volume_db=?8, lufs=?9, clipped=?10,
                duration_s=?11, waveform=?12, segments_json=?13, cues_json=?14,
                beatgrid_json=?15, analysis_status='done', analyzed_at_unix=?16,
                analysis_ms=?17, waveform_bands=?19
             WHERE id=?18",
            params![
                a.key_camelot,
                a.key_name,
                a.key_confidence,
                a.second_key,
                a.second_confidence,
                a.tempo,
                a.energy,
                a.volume_db,
                a.lufs,
                a.clipped_peak_count,
                a.duration_s,
                waveform,
                segments,
                cues,
                beatgrid,
                now,
                a.analysis_ms.map(|v| v as i64),
                track_id,
                waveform_bands
            ],
        )?;
        ensure_track(n, track_id, "save_analysis")
    }

    /// Corrige la duración persistida (auto-heal cuando el deck revela la
    /// duración real del archivo y difiere del análisis).
    pub fn set_duration(&self, track_id: i64, duration_s: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE tracks SET duration_s=?2 WHERE id=?1",
            params![track_id, duration_s],
        )?;
        Ok(())
    }

    /// Reanalizar biblioteca (paso 1): borra SOLO los tags que genera
    /// Smart Mix (tono/bpm/energia + confianzas) de TODAS las pistas y
    /// las devuelve a la cola. No toca metadatos de archivo ni playlists.
    pub fn clear_sm_tags_all(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE tracks SET
                key_camelot=NULL, key_name=NULL, key_confidence=NULL,
                second_key=NULL, second_confidence=NULL,
                bpm=NULL, energy=NULL,
                analysis_status='pending', analyzed_at_unix=NULL
             WHERE analysis_status != 'pending'",
            [],
        )?;
        Ok(n)
    }

    /// Reinicio TOTAL del análisis (modo dev / "usuario nuevo"): además de los
    /// tags de clear_sm_tags_all, borra TODOS los derivados del análisis —
    /// cues, beatgrid, waveform (envolvente y bandas), segmentos, LUFS,
    /// clipping, tiempos y portada cacheada. Ratings, playlists, rutas y
    /// metadatos de archivo quedan intactos.
    pub fn reset_analysis_full(&self) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE tracks SET
                key_camelot=NULL, key_name=NULL, key_confidence=NULL,
                second_key=NULL, second_confidence=NULL,
                bpm=NULL, energy=NULL,
                volume_db=NULL, lufs=NULL, clipped=NULL,
                waveform=NULL, waveform_bands=NULL, segments_json=NULL,
                cues_json=NULL, beatgrid_json=NULL,
                cover=NULL, analysis_ms=NULL,
                analysis_status='pending', analyzed_at_unix=NULL",
            [],
        )?;
        Ok(n)
    }

    pub fn set_analysis_status(&self, track_id: i64, status: AnalysisStatus) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE tracks SET analysis_status=?1 WHERE id=?2",
            params![status_str(status), track_id],
        )?;
        ensure_track(n, track_id, "set_analysis_status")
    }

    /// Pistas en un estado dado: `(id, path)` en orden de inserción.
    pub fn track_ids_by_status(
        &self,
        status: AnalysisStatus,
        limit: usize,
    ) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path FROM tracks WHERE analysis_status = ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![status_str(status), limit as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Reemplaza los cues de la pista (JSON) sin tocar el resto del análisis.
    pub fn replace_cues(&self, track_id: i64, cues: &[CuePoint]) -> Result<()> {
        let json = serde_json::to_string(cues).context("serializando cues")?;
        let n = self.conn.execute(
            "UPDATE tracks SET cues_json=?1 WHERE id=?2",
            params![json, track_id],
        )?;
        ensure_track(n, track_id, "replace_cues")
    }

    /// Corrección manual de clave/bpm: solo sobrescribe lo provisto.
    pub fn override_key_bpm(
        &self,
        track_id: i64,
        key: Option<&str>,
        bpm: Option<f64>,
    ) -> Result<()> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM tracks WHERE id=?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return track_missing(track_id, "override_key_bpm");
        }
        let mut sets: Vec<&str> = Vec::new();
        let mut pv: Vec<SqlValue> = Vec::new();
        if let Some(k) = key {
            sets.push("key_camelot=?");
            pv.push(SqlValue::Text(k.to_string()));
        }
        if let Some(b) = bpm {
            sets.push("bpm=?");
            pv.push(SqlValue::Real(b));
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!("UPDATE tracks SET {} WHERE id=?", sets.join(", "));
        pv.push(SqlValue::Integer(track_id));
        self.conn.execute(&sql, params_from_iter(pv))?;
        Ok(())
    }

    /// Borrado con cascada: cues/segments (viven en la fila), playlist_tracks
    /// y la entrada FTS.
    pub fn delete_track(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM playlist_tracks WHERE track_id=?1", params![id])?;
        self.conn
            .execute("DELETE FROM tracks_fts WHERE rowid=?1", params![id])?;
        let n = self
            .conn
            .execute("DELETE FROM tracks WHERE id=?1", params![id])?;
        ensure_track(n, id, "delete_track")
    }

    // -- Consulta ------------------------------------------------------------

    pub fn query(&self, q: &LibraryQuery) -> Result<Vec<TrackRow>> {
        let f = Filter::build(q);
        let (join_sql, join_params) = join_parts(f.playlist_id);
        let order = order_clause(q, f.playlist_id.is_some());
        let mut all = join_params;
        all.extend(f.params);
        let limit_sql;
        if q.limit == 0 {
            all.push(SqlValue::Integer(q.offset as i64));
            limit_sql = " LIMIT -1 OFFSET ?".to_string();
        } else {
            all.push(SqlValue::Integer(q.limit as i64));
            all.push(SqlValue::Integer(q.offset as i64));
            limit_sql = " LIMIT ? OFFSET ?".to_string();
        }
        let sql = format!(
            "SELECT t.id, t.title, t.artist, t.album, t.genre, t.key_camelot, t.bpm, \
             t.energy, t.duration_s, t.rating FROM tracks t{join_sql}{}{order}{limit_sql}",
            f.where_sql
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(all), map_track_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Total de pistas que satisfacen el mismo filtro de `query`
    /// (ignora sort/limit/offset).
    pub fn count(&self, q: &LibraryQuery) -> Result<i64> {
        let f = Filter::build(q);
        let (join_sql, join_params) = join_parts(f.playlist_id);
        let mut all = join_params;
        all.extend(f.params);
        let sql = format!("SELECT COUNT(*) FROM tracks t{join_sql}{}", f.where_sql);
        Ok(self
            .conn
            .query_row(&sql, params_from_iter(all), |r| r.get(0))?)
    }

    /// Pista completa por id.
    pub fn track_full(&self, id: i64) -> Result<Option<TrackFull>> {
        let full = self
            .conn
            .query_row(TRACK_FULL_SQL, params![id], map_track_full)
            .optional()?;
        Ok(full)
    }

    /// analysis_ms de una pista (consulta ligera: sin BLOBs ni JSONs).
    pub fn analysis_ms(&self, track_id: i64) -> Result<Option<u64>> {
        let v: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT analysis_ms FROM tracks WHERE id=?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.flatten().map(|v| v as u64))
    }

    /// Waveform decodificada de bincode (None si la pista no existe).
    /// Persiste solo el envelope de bandas (backfill sin re-analizar todo).
    pub fn set_waveform_bands(&self, track_id: i64, bands: &[f32]) -> Result<()> {
        let blob = bincode::serialize(bands)?;
        self.conn.execute(
            "UPDATE tracks SET waveform_bands=?2 WHERE id=?1",
            params![track_id, blob],
        )?;
        Ok(())
    }

    /// Lee solo el envelope de bandas (None si la pista no lo tiene).
    pub fn waveform_bands(&self, track_id: i64) -> Result<Option<Vec<f32>>> {
        let blob: Option<Vec<u8>> = match self.conn.query_row(
            "SELECT waveform_bands FROM tracks WHERE id=?1",
            params![track_id],
            |r| r.get(0),
        ) {
            // Some(bytes) si hay bandas; None si la columna es NULL.
            Ok(v) => v,
            // Pista inexistente = "sin datos" (doc del método): None sin ruido.
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            // Audit B3: antes CUALQUIER error de query se tragaba como None
            // (map_err(..).unwrap_or(None)); ahora se loguea y se propaga —
            // None queda reservado para "sin datos". Firma sin cambios.
            Err(e) => {
                eprintln!("[db] waveform_bands error id={track_id}: {e}");
                return Err(anyhow::anyhow!("waveform_bands id={track_id}: {e}"));
            }
        };
        match blob {
            None => Ok(None),
            Some(bytes) => match bincode::deserialize::<Vec<f32>>(&bytes) {
                Ok(v) => Ok(Some(v)),
                // BLOB corrupto: logueado (audit B3) y tratado como sin datos
                // para que el backfill lo regenere en vez de abortar.
                Err(e) => {
                    eprintln!("[db] waveform_bands blob corrupto id={track_id}: {e}");
                    Ok(None)
                }
            },
        }
    }

    pub fn waveform(&self, track_id: i64) -> Result<Option<Vec<f32>>> {
        let blob: Option<Option<Vec<u8>>> = self
            .conn
            .query_row(
                "SELECT waveform FROM tracks WHERE id=?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?;
        match blob.flatten() {
            None => Ok(None),
            Some(bytes) => Ok(Some(
                bincode::deserialize(&bytes).context("deserializando waveform")?,
            )),
        }
    }

    /// Portada embebida (None si la pista no existe o no tiene).
    pub fn cover(&self, track_id: i64) -> Result<Option<Vec<u8>>> {
        let c: Option<Option<Vec<u8>>> = self
            .conn
            .query_row(
                "SELECT cover FROM tracks WHERE id=?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(c.flatten())
    }

    // -- Playlists -----------------------------------------------------------

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, kind, filter_json, parent_id FROM playlists ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                let filter_json: Option<String> = r.get(3)?;
                Ok(Playlist {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    smart_filter: filter_json.and_then(|j| serde_json::from_str(&j).ok()),
                    parent_id: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn create_playlist(
        &self,
        name: &str,
        kind: &str,
        filter: Option<&SmartFilter>,
    ) -> Result<Playlist> {
        let filter_json = filter.map(serde_json::to_string).transpose()?;
        self.conn.execute(
            "INSERT INTO playlists (name, kind, filter_json) VALUES (?1, ?2, ?3)",
            params![name, kind, filter_json],
        )?;
        Ok(Playlist {
            id: self.conn.last_insert_rowid(),
            name: name.to_string(),
            kind: kind.to_string(),
            smart_filter: filter.cloned(),
            parent_id: None,
        })
    }

    /// F-18: cuelga `id` bajo `parent` (None = raíz). Rechaza ciclos:
    /// el padre no puede ser la propia playlist ni descender de ella.
    pub fn set_playlist_parent(&self, id: i64, parent: Option<i64>) -> Result<()> {
        let exists = |pid: i64| -> Result<bool> {
            let v: Option<i64> = self
                .conn
                .query_row("SELECT id FROM playlists WHERE id=?1", params![pid], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(v.is_some())
        };
        if !exists(id)? {
            bail!("set_playlist_parent: playlist {id} no existe");
        }
        if let Some(p) = parent {
            if p == id {
                bail!("set_playlist_parent: {id} no puede ser su propio padre");
            }
            if !exists(p)? {
                bail!("set_playlist_parent: playlist padre {p} no existe");
            }
            // anti-ciclo: subir por la cadena de padres de `p`; si se
            // alcanza `id`, colgarla ahí crearía un ciclo
            let mut cur = p;
            let mut hops = 0;
            while let Some(up) = self.parent_of(cur)? {
                if up == id {
                    bail!("set_playlist_parent: {p} desciende de {id} (ciclo)");
                }
                if up == cur {
                    break; // ciclo preexistente corrupto: no seguir
                }
                cur = up;
                hops += 1;
                if hops > 10_000 {
                    break; // cota defensiva
                }
            }
        }
        let n = self.conn.execute(
            "UPDATE playlists SET parent_id=?1 WHERE id=?2",
            params![parent, id],
        )?;
        if n == 0 {
            bail!("set_playlist_parent: playlist {id} no existe");
        }
        Ok(())
    }

    fn parent_of(&self, id: i64) -> Result<Option<i64>> {
        let v: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT parent_id FROM playlists WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.flatten())
    }

    /// Idempotente; sus pistas caen por FK CASCADE. F-18: sus sub-playlists
    /// se re-heredan al padre de la borrada (o a raíz si era raíz).
    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE playlists SET parent_id = (SELECT parent_id FROM playlists WHERE id = ?1)
             WHERE parent_id = ?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM playlists WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn rename_playlist(&self, id: i64, new_name: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE playlists SET name=?1 WHERE id=?2",
            params![new_name, id],
        )?;
        if n == 0 {
            bail!("rename_playlist: playlist {id} no existe");
        }
        Ok(())
    }

    /// Añade al final (position = max+1); no-op si ya pertenece.
    pub fn add_track_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        let pl: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM playlists WHERE id=?1",
                params![playlist_id],
                |r| r.get(0),
            )
            .optional()?;
        if pl.is_none() {
            bail!("add_track_to_playlist: playlist {playlist_id} no existe");
        }
        let tr: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM tracks WHERE id=?1",
                params![track_id],
                |r| r.get(0),
            )
            .optional()?;
        if tr.is_none() {
            bail!("add_track_to_playlist: track {track_id} no existe");
        }
        let already: Option<i64> = self
            .conn
            .query_row(
                "SELECT track_id FROM playlist_tracks WHERE playlist_id=?1 AND track_id=?2",
                params![playlist_id, track_id],
                |r| r.get(0),
            )
            .optional()?;
        if already.is_some() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1
                              FROM playlist_tracks WHERE playlist_id = ?1))",
            params![playlist_id, track_id],
        )?;
        Ok(())
    }

    /// Idempotente.
    pub fn remove_track_from_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id=?1 AND track_id=?2",
            params![playlist_id, track_id],
        )?;
        Ok(())
    }

    pub fn playlist_track_ids(&self, playlist_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id FROM playlist_tracks WHERE playlist_id=?1 ORDER BY position",
        )?;
        let rows = stmt
            .query_map(params![playlist_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<i64>, _>>()?;
        Ok(rows)
    }

    /// F-19: fija el orden manual de la playlist. `ordered_ids` debe ser una
    /// permutación EXACTA de sus pistas actuales (ni más ni menos); si no,
    /// error sin tocar nada. Transaccional: o se aplica todo o nada.
    pub fn reorder_playlist(&self, playlist_id: i64, ordered_ids: &[i64]) -> Result<()> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM playlists WHERE id=?1",
                params![playlist_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            bail!("reorder_playlist: playlist {playlist_id} no existe");
        }
        let current = self.playlist_track_ids(playlist_id)?;
        let mut want = ordered_ids.to_vec();
        want.sort();
        let mut have = current.clone();
        have.sort();
        if want != have {
            bail!(
                "reorder_playlist: ids no son una permutación de la playlist \
                 (esperados {:?}, recibidos {:?})",
                have,
                want
            );
        }
        let tx = self.conn.unchecked_transaction()?;
        for (pos, tid) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE playlist_tracks SET position=?1
                 WHERE playlist_id=?2 AND track_id=?3",
                params![pos as i64 + 1, playlist_id, tid],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // -- Ajustes -------------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v)
    }

    pub fn set_setting(&self, key: &str, value_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value_json],
        )?;
        Ok(())
    }

    /// Actualiza la ruta de una pista (tras renombrar el archivo en disco).
    /// Mantiene rowid/FTS (title/artist/album no cambian). Devuelve true si
    /// actualizó exactamente 1 fila; false si la ruta vieja no existe o la
    /// nueva ya está registrada (protección de duplicados).
    pub fn update_track_path(&self, old: &str, new: &str) -> Result<bool> {
        let dup: Option<i64> = self
            .conn
            .query_row("SELECT id FROM tracks WHERE path=?1", params![new], |r| {
                r.get(0)
            })
            .optional()?;
        if dup.is_some() {
            return Ok(false);
        }
        let n = self
            .conn
            .execute("UPDATE tracks SET path=?2 WHERE path=?1", params![old, new])?;
        Ok(n == 1)
    }

    /// Preferencias globales, serializadas como JSON bajo `"preferences"`.
    pub fn get_preferences(&self) -> Result<Preferences> {
        match self.get_setting("preferences")? {
            Some(json) => Ok(serde_json::from_str(&json).context("parseando preferences")?),
            None => Ok(Preferences::default()),
        }
    }

    pub fn set_preferences(&self, p: &Preferences) -> Result<()> {
        let json = serde_json::to_string(p)?;
        self.set_setting("preferences", &json)
    }

    // -- Estadística y duplicados ---------------------------------------------

    pub fn stats(&self) -> Result<LibraryStats> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*),
                    COUNT(*) FILTER (WHERE analysis_status = 'done'),
                    COUNT(*) FILTER (WHERE analysis_status = 'pending'),
                    COUNT(*) FILTER (WHERE analysis_status = 'failed')
             FROM tracks",
            [],
            |r| {
                Ok(LibraryStats {
                    total: r.get(0)?,
                    analyzed: r.get(1)?,
                    pending: r.get(2)?,
                    failed: r.get(3)?,
                })
            },
        )?)
    }

    /// Duplicados por metadatos:
    /// - `"exact"`: artist+title normalizados idénticos (ambos no vacíos).
    /// - `"perceptual"`: título similar (Levenshtein ≥ 0.90) + duración ±2 s.
    ///
    /// Union-find con buckets de duración (2 s, ventana ±1) como
    /// `core/dedupe/detector.py`; un grupo es "perceptual" si ALGUNA de sus
    /// uniones lo fue, y "exact" solo si todas vinieron de la fase exacta.
    pub fn duplicates(&self) -> Result<Vec<DuplicateGroup>> {
        // (id, path, title, artist, duration) en un solo orden consistente
        let mut rows: Vec<(i64, String, String, String, f64)> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, path, title, artist, duration_s FROM tracks")?;
            let it = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, f64>(4)?,
                ))
            })?;
            for row in it {
                rows.push(row?);
            }
        }

        // ---- fase exacta: artist+title normalizados ----
        // Títulos normalizados UNA vez por pista (auditoría 2026-11-10):
        // antes title_similar re-normalizaba ambos títulos por par
        // candidato de la fase perceptual.
        let norm_titles: Vec<String> = rows.iter().map(|r| normalize(&r.2)).collect();
        let mut dsu = Dsu::new(rows.len());
        let mut by_key: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            let na = normalize(&r.3);
            let nt = norm_titles[i].clone();
            if na.is_empty() || nt.is_empty() {
                continue;
            }
            by_key.entry((na, nt)).or_default().push(i);
        }
        for idxs in by_key.values() {
            for w in idxs.windows(2) {
                dsu.union(w[0], w[1]);
            }
        }

        // ---- fase perceptual: título similar + duración ±2 s ----
        const DUR_TOL: f64 = 2.0;
        const SIM_TOL: f64 = 0.90;
        let mut perceptual_pairs: Vec<(usize, usize)> = Vec::new();
        let mut buckets: HashMap<i64, Vec<usize>> = HashMap::new();
        for (i, r) in rows.iter().enumerate() {
            if r.4 > 0.0 && !norm_titles[i].is_empty() {
                buckets
                    .entry((r.4 / 2.0).round() as i64)
                    .or_default()
                    .push(i);
            }
        }
        let mut candidates: Vec<usize> = Vec::new();
        let bucket_keys: Vec<i64> = buckets.keys().copied().collect();
        for b in bucket_keys {
            candidates.clear();
            for bk in b - 1..=b + 1 {
                if let Some(m) = buckets.get(&bk) {
                    candidates.extend_from_slice(m);
                }
            }
            for xi in 0..candidates.len() {
                for yi in xi + 1..candidates.len() {
                    let (i, j) = (candidates[xi], candidates[yi]);
                    if dsu.find(i) == dsu.find(j) {
                        continue; // ya unidos por la fase exacta
                    }
                    if (rows[i].4 - rows[j].4).abs() > DUR_TOL {
                        continue;
                    }
                    if !title_similar_norm(&norm_titles[i], &norm_titles[j], SIM_TOL) {
                        continue;
                    }
                    dsu.union(i, j);
                    perceptual_pairs.push((i, j));
                }
            }
        }

        // ---- emisión de grupos ----
        let mut comps: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..rows.len() {
            comps.entry(dsu.find(i)).or_default().push(i);
        }
        let mut tainted: HashSet<usize> = HashSet::new();
        for (i, _) in &perceptual_pairs {
            tainted.insert(dsu.find(*i));
        }
        let mut groups: Vec<DuplicateGroup> = Vec::new();
        for (root, members) in comps {
            if members.len() < 2 {
                continue;
            }
            let reason = if tainted.contains(&root) {
                "perceptual"
            } else {
                "exact"
            };
            let mut tracks: Vec<(i64, String, f64)> = members
                .iter()
                .map(|&i| (rows[i].0, rows[i].1.clone(), rows[i].4))
                .collect();
            tracks.sort_by(|a, b| a.1.cmp(&b.1));
            groups.push(DuplicateGroup {
                reason: reason.to_string(),
                tracks,
            });
        }
        groups.sort_by(|a, b| {
            b.tracks
                .len()
                .cmp(&a.tracks.len())
                .then_with(|| a.tracks[0].1.cmp(&b.tracks[0].1))
        });
        Ok(groups)
    }
}

// ---------------------------------------------------------------------------
// Mapeo de filas
// ---------------------------------------------------------------------------

fn map_track_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        album: r.get(3)?,
        genre: r.get(4)?,
        key_camelot: r.get(5)?,
        bpm: r.get(6)?,
        energy: r.get(7)?,
        duration_s: r.get(8)?,
        rating: r.get::<_, Option<i64>>(9)?.unwrap_or(0).clamp(0, 5) as u8,
    })
}

const TRACK_FULL_SQL: &str = "SELECT id, path, title, artist, album, genre, year, track_no, \
    duration_s, bitrate_kbps, sample_rate, channels, format, file_size, file_modified_unix, \
    key_camelot, key_name, key_confidence, second_key, second_confidence, bpm, energy, \
    volume_db, lufs, clipped, waveform, segments_json, cues_json, beatgrid_json, \
    analysis_status, analysis_ms, waveform_bands, rating FROM tracks WHERE id = ?1";

/// Deserializa o cae a `default` logueando el fallo (audit B3): los BLOB/JSON
/// corruptos de `map_track_full` antes se tragaban en silencio con
/// `unwrap_or_default`/`.ok()`. Mismo valor devuelto que antes — ahora con
/// rastro en stderr del campo perdido (id + campo + error).
fn logged_or_default<T, E: std::fmt::Display>(
    id: i64,
    campo: &'static str,
    r: Result<T, E>,
    default: T,
) -> T {
    match r {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[db] {campo} corrupto id={id}: {e}");
            default
        }
    }
}

/// Como `logged_or_default` pero devuelve `Option<T>` (audit B3): para los
/// campos cuyo default de "dato corrupto" es `None` (waveform_bands,
/// beatgrid). Mismo valor devuelto que el `.ok()` de antes, ahora logueado.
fn logged_ok<T, E: std::fmt::Display>(id: i64, campo: &'static str, r: Result<T, E>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("[db] {campo} corrupto id={id}: {e}");
            None
        }
    }
}

fn map_track_full(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackFull> {
    let id: i64 = r.get(0)?;
    let duration_s: f64 = r.get(8)?;
    let segments_json: Option<String> = r.get(26)?;
    let cues_json: Option<String> = r.get(27)?;
    let beatgrid_json: Option<String> = r.get(28)?;
    let status_raw: String = r.get(29)?;
    let waveform_blob: Option<Vec<u8>> = r.get(25)?;
    let waveform = waveform_blob
        .map(|b| {
            logged_or_default(
                id,
                "waveform",
                bincode::deserialize::<Vec<f32>>(&b),
                Vec::new(),
            )
        })
        .unwrap_or_default();
    let bands_blob: Option<Vec<u8>> = r.get(31)?;
    let waveform_bands = bands_blob
        .and_then(|b| logged_ok(id, "waveform_bands", bincode::deserialize::<Vec<f32>>(&b)))
        .filter(|v: &Vec<f32>| !v.is_empty());
    let meta = TrackMeta {
        path: r.get(1)?,
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        genre: r.get(5)?,
        year: r.get(6)?,
        track_no: r.get(7)?,
        duration_s,
        bitrate_kbps: r.get(9)?,
        sample_rate: r.get(10)?,
        channels: r.get(11)?,
        format: r.get(12)?,
        file_size: r.get::<_, i64>(13)? as u64,
        file_modified_unix: r.get(14)?,
    };
    let analysis = TrackAnalysis {
        key_camelot: r.get(15)?,
        key_name: r.get(16)?,
        key_confidence: r.get(17)?,
        second_key: r.get(18)?,
        second_confidence: r.get(19)?,
        tempo: r.get(20)?,
        beatgrid: beatgrid_json
            .and_then(|j| logged_ok(id, "beatgrid_json", serde_json::from_str(&j))),
        energy: r.get(21)?,
        segments: segments_json
            .map(|j| logged_or_default(id, "segments_json", serde_json::from_str(&j), Vec::new()))
            .unwrap_or_default(),
        cues: cues_json
            .map(|j| logged_or_default(id, "cues_json", serde_json::from_str(&j), Vec::new()))
            .unwrap_or_default(),
        volume_db: r.get(22)?,
        lufs: r.get(23)?,
        clipped_peak_count: r.get(24)?,
        waveform,
        waveform_bands,
        duration_s,
        analysis_ms: r.get::<_, Option<i64>>(30)?.map(|v| v as u64),
    };
    Ok(TrackFull {
        id,
        meta,
        analysis,
        status: parse_status(&status_raw).unwrap_or(AnalysisStatus::Pending),
        rating: r.get::<_, Option<i64>>(32)?.unwrap_or(0).clamp(0, 5) as u8,
    })
}

fn status_str(s: AnalysisStatus) -> &'static str {
    match s {
        AnalysisStatus::Pending => "pending",
        AnalysisStatus::Queued => "queued",
        AnalysisStatus::Analyzing => "analyzing",
        AnalysisStatus::Done => "done",
        AnalysisStatus::Failed => "failed",
    }
}

fn parse_status(s: &str) -> Option<AnalysisStatus> {
    match s {
        "pending" => Some(AnalysisStatus::Pending),
        "queued" => Some(AnalysisStatus::Queued),
        "analyzing" => Some(AnalysisStatus::Analyzing),
        "done" => Some(AnalysisStatus::Done),
        "failed" => Some(AnalysisStatus::Failed),
        _ => None,
    }
}

fn ensure_track(n: usize, id: i64, op: &str) -> Result<()> {
    if n == 0 {
        track_missing(id, op)
    } else {
        Ok(())
    }
}

fn track_missing(id: i64, op: &str) -> Result<()> {
    bail!("{op}: track {id} no existe")
}

// ---------------------------------------------------------------------------
// Construcción de filtros SQL
// ---------------------------------------------------------------------------

fn join_parts(playlist_id: Option<i64>) -> (&'static str, Vec<SqlValue>) {
    match playlist_id {
        Some(pid) => (
            " JOIN playlist_tracks pt ON pt.playlist_id = ? AND pt.track_id = t.id",
            vec![SqlValue::Integer(pid)],
        ),
        None => ("", Vec::new()),
    }
}

struct Filter {
    playlist_id: Option<i64>,
    where_sql: String,
    params: Vec<SqlValue>,
}

impl Filter {
    fn build(q: &LibraryQuery) -> Self {
        let mut conds: Vec<String> = Vec::new();
        let mut ps: Vec<SqlValue> = Vec::new();

        // FTS: el texto de la query y el del smart filter se combinan (AND).
        let mut texts: Vec<&str> = Vec::new();
        if let Some(t) = &q.text {
            texts.push(t.as_str());
        }
        if let Some(sf) = &q.smart {
            if let Some(t) = &sf.text {
                texts.push(t.as_str());
            }
        }
        if let Some(expr) = fts_expr(&texts) {
            conds.push("t.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?)".into());
            ps.push(SqlValue::Text(expr));
        }

        if let Some(sf) = &q.smart {
            if let Some(v) = sf.energy_min {
                conds.push("t.energy >= ?".into());
                ps.push(SqlValue::Real(v));
            }
            if let Some(v) = sf.energy_max {
                conds.push("t.energy <= ?".into());
                ps.push(SqlValue::Real(v));
            }
            if let Some(v) = sf.bpm_min {
                conds.push("t.bpm >= ?".into());
                ps.push(SqlValue::Real(v));
            }
            if let Some(v) = sf.bpm_max {
                conds.push("t.bpm <= ?".into());
                ps.push(SqlValue::Real(v));
            }
            if let Some(g) = &sf.genre {
                if !g.trim().is_empty() {
                    conds.push("t.genre LIKE ? ESCAPE '\\'".into());
                    ps.push(SqlValue::Text(format!("%{}%", like_escape(g))));
                }
            }
            if let Some(k) = &sf.key_compatible_with {
                let codes = compatible_codes(k);
                if codes.is_empty() {
                    // clave no parseable: nada es compatible
                    conds.push("0".into());
                } else {
                    let ph = vec!["?"; codes.len()].join(",");
                    conds.push(format!("t.key_camelot IN ({ph})"));
                    ps.extend(codes.into_iter().map(SqlValue::Text));
                }
            }
        }

        if let Some(st) = q.status_filter {
            conds.push("t.analysis_status = ?".into());
            ps.push(SqlValue::Text(status_str(st).to_string()));
        }

        let where_sql = if conds.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conds.join(" AND "))
        };
        Filter {
            playlist_id: q.playlist_id,
            where_sql,
            params: ps,
        }
    }
}

fn order_clause(q: &LibraryQuery, playlist: bool) -> String {
    if playlist {
        return " ORDER BY pt.position ASC, t.id ASC".to_string();
    }
    let dir = if q.desc { "DESC" } else { "ASC" };
    let col = match q.sort {
        SortField::Title => "t.title COLLATE NOCASE",
        SortField::Artist => "t.artist COLLATE NOCASE",
        SortField::Album => "t.album COLLATE NOCASE",
        SortField::Key => {
            // orden numérico del código camelot (1A..12B), no lexicográfico
            return format!(
                " ORDER BY (t.key_camelot IS NULL) ASC, \
                 CAST(substr(t.key_camelot, 1, LENGTH(t.key_camelot)-1) AS INTEGER) {dir}, \
                 substr(t.key_camelot, LENGTH(t.key_camelot), 1) {dir}, t.id {dir}"
            );
        }
        SortField::Bpm => "t.bpm",
        SortField::Energy => "t.energy",
        SortField::Duration => "t.duration_s",
        SortField::AddedAt => "t.added_at_unix",
        SortField::Rating => "t.rating",
    };
    format!(" ORDER BY ({col} IS NULL) ASC, {col} {dir}, t.id {dir}")
}

/// Expresión FTS5 de prefijos: cada token alfanumérico → `"tok"*` (AND implícito).
fn fts_expr(texts: &[&str]) -> Option<String> {
    let mut tokens: Vec<String> = Vec::new();
    for t in texts {
        for w in t.split_whitespace() {
            let w: String = w
                .chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(|c| c.to_lowercase())
                .collect();
            if !w.is_empty() {
                tokens.push(w);
            }
        }
    }
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|t| format!("\"{t}\"*"))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Los códigos camelot compatibles con `target` (misma, ±1 mismo lado,
/// relativa) según `sm_model::camelot::is_compatible` — se traduce a un
/// `IN (...)` para que el filtro viva en SQL y respete limit/offset.
fn compatible_codes(target: &str) -> Vec<String> {
    let mut codes = Vec::new();
    for num in 1..=12u8 {
        for minor in [true, false] {
            let code = format!("{num}{}", if minor { 'A' } else { 'B' });
            if camelot::is_compatible(&code, target) {
                codes.push(code);
            }
        }
    }
    codes
}

// ---------------------------------------------------------------------------
// Normalización / similitud / union-find (dedupe por metadatos)
// ---------------------------------------------------------------------------

/// minúsculas, solo alfanuméricos, espacios colapsados.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim_end().to_string()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Títulos YA normalizados similares: idénticos o ratio Levenshtein ≥ tol.
///
/// Prefiltro de longitud (auditoría 2026-11-10) con equivalencia exacta:
/// lev(a,b) ≥ |len(a)−len(b)| ⇒ ratio ≤ 1 − diff/max; si ese límite
/// superior ya es < tol, el Levenshtein no puede alcanzar el umbral y la
/// decisión (false) es la misma que sin el prefiltro.
fn title_similar_norm(na: &str, nb: &str, tol: f64) -> bool {
    if na == nb {
        return true;
    }
    let (ca, cb) = (na.chars().count(), nb.chars().count());
    let max = ca.max(cb);
    if max == 0 {
        return false;
    }
    let diff = ca.abs_diff(cb);
    if 1.0 - (diff as f64 / max as f64) < tol {
        return false;
    }
    1.0 - (levenshtein(na, nb) as f64 / max as f64) >= tol
}

struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra.max(rb)] = ra.min(rb);
        }
    }
}
