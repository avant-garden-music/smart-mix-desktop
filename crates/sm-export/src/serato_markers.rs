//! Hot cues de Serato DJ DENTRO del archivo de audio: frame GEOB
//! `Serato Markers2` en un tag ID3v2.4.
//!
//! Port fiel de `harmonic-engine/core/tagger/serato.py` (payload) más la
//! parte de `id3v2.py`/`containers.py` que necesita para leer y escribir
//! el tag sin romper los frames existentes. Paridad de bytes verificada
//! contra la referencia Python (`tests/fixtures/markers2_py_{A,B}.bin`
//! son payloads generados por `core.tagger.serato.build_markers2`).
//!
//! Dónde vive, por contenedor (siempre dentro de un tag ID3v2.4):
//!
//! ===== ===========================================================
//! MP3   GEOB en el tag ID3v2.4 de cabecera
//! AIFF  GEOB dentro del chunk `ID3 ` del FORM
//! WAV   GEOB dentro del chunk `id3 ` de RIFF (el canal que lee Serato)
//! ===== ===========================================================
//!
//! Payload del GEOB: `01 01` + base64(inner) con `\n` cada 72 caracteres,
//! SIN padding `=`, relleno NUL hasta 470 bytes (pre-asignación de
//! Serato). `inner` = `01 01` + entradas (nombre ASCII + NUL + longitud
//! u32 BE + datos) + `\x00` centinela. CUE (13 bytes con nombre vacío):
//! `00 | índice u8 | posición u32 BE (ms) | 00 | RGB(3) | 00 00 | NUL`;
//! BPMLOCK: 1 byte (`00` beatgrid libre / `01` bloqueado).
//!
//! Por qué no lofty aquí: el payload GEOB es binario byte-exacto y lofty
//! re-serializa el tag completo al guardar (reordena/re-codifica frames);
//! un parse/serialize ID3v2 mínimo permite conservar los frames ajenos
//! byte a byte, como hace el escritor Python (`id3v2.py`), y emitir el
//! GEOB con la disposición exacta de Serato/Mixxx.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use sm_model::CuePoint;

// ----------------------------------------------------------- constantes

/// Description del frame GEOB que identifica los markers de Serato.
pub const GEOB_DESCRIPTION: &str = "Serato Markers2";
/// MIME del GEOB (la disposition exacta que escriben Serato/Mixxx).
pub const GEOB_MIME: &str = "application/octet-stream";
/// Cabecera de versión que Serato escribe dentro y fuera del base64.
pub const MARKERS2_VERSION: [u8; 2] = [0x01, 0x01];
/// Tamaño mínimo que Serato pre-asigna al tag.
pub const ALLOCATED_SIZE: usize = 470;
/// `\n` insertado cada N caracteres de base64.
pub const BASE64_LINE: usize = 72;
/// Hot cues que soporta Serato.
pub const MAX_CUES: usize = 8;

/// Energía 1-10 → color de la paleta de cues de Serato (frío → caliente).
pub const ENERGY_COLORS: [(u8, [u8; 3]); 10] = [
    (1, [0x00, 0x00, 0xCC]),
    (2, [0x00, 0x44, 0xCC]),
    (3, [0x00, 0x88, 0xCC]),
    (4, [0x00, 0xCC, 0xCC]),
    (5, [0x00, 0xCC, 0x88]),
    (6, [0x00, 0xCC, 0x00]),
    (7, [0xCC, 0xCC, 0x00]),
    (8, [0xCC, 0x88, 0x00]),
    (9, [0xCC, 0x44, 0x00]),
    (10, [0xCC, 0x00, 0x00]),
];

/// Colores por defecto de los 8 hot cues de Serato (HC1..HC8).
pub const DEFAULT_CUE_COLORS: [[u8; 3]; 8] = [
    [0xCC, 0x00, 0x00],
    [0xCC, 0x88, 0x00],
    [0x00, 0x00, 0xCC],
    [0xCC, 0xCC, 0x00],
    [0x00, 0xCC, 0x00],
    [0xCC, 0x00, 0xCC],
    [0x00, 0xCC, 0xCC],
    [0x88, 0x00, 0xCC],
];

// --------------------------------------------------------------- tipos

/// Cue leído de un payload Markers2 (roundtrip/verificación).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerCue {
    /// Índice 0-based del hot cue (0..7).
    pub index: u8,
    /// Posición en segundos.
    pub time_s: f64,
    /// Color RGB del cue.
    pub color: [u8; 3],
}

/// Contenido decodificado de un payload Markers2.
#[derive(Debug, Clone, PartialEq)]
pub struct Markers2 {
    pub cues: Vec<MarkerCue>,
    pub bpmlock: bool,
}

/// Cue a escribir: `(time_s, energy)` — energía 1-10; `None` (o fuera de
/// rango) usa el color por defecto del índice, como `_cue_color` en el .py.
pub type CueInput = (f64, Option<u8>);

// ------------------------------------------------------------- payload

/// Nivel de energía (1-10) → 3 bytes RGB de la paleta Serato.
/// `None` si el nivel no es mapeable (el llamador decide el respaldo).
pub fn energy_color(level: u8) -> Option<[u8; 3]> {
    ENERGY_COLORS
        .iter()
        .find(|(l, _)| *l == level)
        .map(|&(_, c)| c)
}

fn cue_color(level: Option<u8>, index1: usize) -> [u8; 3] {
    level
        .and_then(energy_color)
        .unwrap_or(DEFAULT_CUE_COLORS[(index1 - 1) % DEFAULT_CUE_COLORS.len()])
}

/// Redondeo de Python (`round`): los empates exactos en .5 van al PAR
/// (half-to-even). Necesario para que `time*1000` produzca los mismos
/// milisegundos que la referencia.
fn py_round(x: f64) -> f64 {
    let floor = x.floor();
    let diff = x - floor;
    if diff > 0.5 {
        floor + 1.0
    } else if diff < 0.5 {
        floor
    } else if (floor as i64) % 2 == 0 {
        floor
    } else {
        floor + 1.0
    }
}

/// `time` (s) → milisegundos u32 (el reloj de posiciones de Serato).
fn time_to_ms(t: f64) -> i64 {
    py_round(t * 1000.0) as i64
}

/// Entrada autodescritta: nombre + NUL + longitud u32 BE + datos.
fn wrap_entry(name: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len() + 5 + data.len());
    out.extend_from_slice(name.as_bytes()); // ASCII por contrato
    out.push(0);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// Base64 estándar SIN padding (la variante que decodifican sin pérdida
/// todos los lectores conocidos; el truco de sustituir '=' por 'A' de
/// Serato añade bytes que parsers estrictos rechazan).
fn b64_encode_nopad(data: &[u8]) -> Vec<u8> {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TBL[(n >> 18) as usize & 63]);
        out.push(TBL[(n >> 12) as usize & 63]);
        if chunk.len() > 1 {
            out.push(TBL[(n >> 6) as usize & 63]);
        }
        if chunk.len() > 2 {
            out.push(TBL[n as usize & 63]);
        }
    }
    out
}

/// Base64 estilo Serato: sin padding `=`, `\n` cada 72 caracteres
/// (solo ENTRE líneas completas, sin `\n` final).
fn b64_wrap(inner: &[u8]) -> Vec<u8> {
    let enc = b64_encode_nopad(inner);
    let mut out = Vec::with_capacity(enc.len() + enc.len() / BASE64_LINE);
    for (i, line) in enc.chunks(BASE64_LINE).enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(line);
    }
    out
}

/// Decodifica base64 ignorando `\n`, NULs de relleno, `=` y cualquier
/// otro byte ajeno al alfabeto (los restos sin padding se decodifican
/// con los bits disponibles, como cualquier lector tolerante).
fn b64_decode(data: &[u8]) -> Vec<u8> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut acc = 0u32;
    let mut nbits = 0u32;
    for &c in data {
        if let Some(v) = val(c) {
            acc = (acc << 6) | v;
            nbits += 6;
            if nbits >= 8 {
                nbits -= 8;
                out.push((acc >> nbits) as u8);
            }
        }
    }
    out
}

/// Construir el payload completo (470 bytes) del GEOB 'Serato Markers2'.
///
/// `cues`: se escriben como CUE en orden, hasta [`MAX_CUES`] (índice
/// 0..n-1); posición = `time_s` redondeado a milisegundos con el
/// redondeo de Python, color = [`energy_color`] o el color por defecto
/// del índice. `bpmlock` añade la entrada BPMLOCK al final.
///
/// Err con lista vacía o tiempos fuera de `0..2^32` ms (igual que el
/// `ValueError` del .py).
pub fn build_markers2(cues: &[CueInput], bpmlock: bool) -> Result<Vec<u8>> {
    if cues.is_empty() {
        bail!("serato markers: no hay cues que escribir");
    }
    let mut inner = Vec::new();
    inner.extend_from_slice(&MARKERS2_VERSION);
    for (i, (t, energy)) in cues.iter().enumerate().take(MAX_CUES) {
        let ms = time_to_ms(*t);
        if !(0..(1i64 << 32)).contains(&ms) {
            bail!("serato markers: tiempo fuera de rango (0..2^32 ms): {t}s");
        }
        let rgb = cue_color(*energy, i + 1);
        // struct '>BBIB3sH' (0, índice, ms, 0, RGB, 0) + NUL (nombre vacío)
        let mut data = Vec::with_capacity(13);
        data.push(0);
        data.push(i as u8);
        data.extend_from_slice(&(ms as u32).to_be_bytes());
        data.push(0);
        data.extend_from_slice(&rgb);
        data.extend_from_slice(&0u16.to_be_bytes());
        data.push(0);
        inner.extend_from_slice(&wrap_entry("CUE", &data));
    }
    inner.extend_from_slice(&wrap_entry("BPMLOCK", &[u8::from(bpmlock)]));
    inner.push(0); // centinela de fin de entradas
    let mut payload = Vec::with_capacity(ALLOCATED_SIZE);
    payload.extend_from_slice(&MARKERS2_VERSION);
    payload.extend_from_slice(&b64_wrap(&inner));
    if payload.len() < ALLOCATED_SIZE {
        payload.resize(ALLOCATED_SIZE, 0);
    }
    Ok(payload)
}

/// Payload → bytes del frame GEOB completo (cabecera ID3 incluida):
/// `GEOB` + tamaño syncsafe + flags 00 00 + encoding latin-1 + MIME
/// octet-stream + filename vacío + description `Serato Markers2` + payload.
pub fn geob_frame(payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(27 + GEOB_DESCRIPTION.len() + payload.len());
    body.push(0x00); // encoding latin-1
    body.extend_from_slice(GEOB_MIME.as_bytes());
    body.push(0x00);
    body.push(0x00); // filename vacío
    body.extend_from_slice(GEOB_DESCRIPTION.as_bytes());
    body.push(0x00);
    body.extend_from_slice(payload);
    let mut frame = Vec::with_capacity(10 + body.len());
    frame.extend_from_slice(b"GEOB");
    frame.extend_from_slice(&encode_syncsafe(body.len()).expect("GEOB cabe en syncsafe"));
    frame.push(0);
    frame.push(0);
    frame.extend_from_slice(&body);
    frame
}

/// Decodificar un payload Markers2 → cues + bpmlock (para roundtrip).
pub fn parse_markers2(payload: &[u8]) -> Result<Markers2> {
    if payload.len() < 2 || payload[..2] != MARKERS2_VERSION {
        bail!("payload Markers2 inválido: falta la cabecera de versión 01 01");
    }
    let inner = b64_decode(&payload[2..]);
    if inner.len() < 2 || inner[..2] != MARKERS2_VERSION {
        bail!("contenido Markers2 inválido: falta la cabecera de versión 01 01");
    }
    let mut cues = Vec::new();
    let mut bpmlock = false;
    let mut pos = 2usize;
    while pos < inner.len() {
        if inner[pos] == 0 {
            break; // centinela de fin de entradas
        }
        let name_end = pos + inner[pos..]
            .iter()
            .position(|&b| b == 0)
            .context("entrada Markers2 sin terminador NUL")?;
        let name = inner[pos..name_end].to_vec();
        pos = name_end + 1;
        let len_bytes: [u8; 4] = inner
            .get(pos..pos + 4)
            .context("entrada Markers2 truncada (longitud)")?
            .try_into()
            .expect("4 bytes");
        let dlen = u32::from_be_bytes(len_bytes) as usize;
        pos += 4;
        let data = inner
            .get(pos..pos + dlen)
            .context("entrada Markers2 truncada (datos)")?;
        pos += dlen;
        if name == b"CUE" {
            if data.len() < 13 {
                bail!("datos CUE malformados: {} bytes", data.len());
            }
            let ms = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
            cues.push(MarkerCue {
                index: data[1],
                time_s: f64::from(ms) / 1000.0,
                color: [data[7], data[8], data[9]],
            });
        } else if name == b"BPMLOCK" {
            if data.is_empty() {
                bail!("datos BPMLOCK vacíos");
            }
            bpmlock = data[0] == 1;
        }
        // nombres desconocidos (LOOP/FLIP/…) se ignoran (fuera de v1)
    }
    Ok(Markers2 { cues, bpmlock })
}

/// Convierte cues del modelo (`sm_model::CuePoint`) a la entrada de
/// [`build_markers2`]. Energías fuera de u8 caen al color por defecto
/// del índice (misma semántica que niveles no mapeables en el .py).
pub fn cues_from_points(points: &[CuePoint]) -> Vec<CueInput> {
    points
        .iter()
        .map(|p| (p.time_s, p.energy_level.and_then(|e| u8::try_from(e).ok())))
        .collect()
}

// ---------------------------------------------------- id3v2 mínimo
// Port de core/tagger/id3v2.py: solo lo que el escritor de markers
// necesita (parse tolerante, serialize determinista v2.4).

const TAG_MAGIC: &[u8; 3] = b"ID3";
/// Padding mínimo / bloque del tag reescrito (lo que usa `build_tag`).
const TAG_MIN_PADDING: usize = 256;
const TAG_BLOCK: usize = 512;

/// Frame ID3 crudo: id de 4 chars + payload (cabecera NO incluida).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Id3Frame {
    pub id: [u8; 4],
    pub payload: Vec<u8>,
}

/// 28 bits big-endian, 7 bits por byte (tamaños de tag/frame v2.4).
fn encode_syncsafe(n: usize) -> Result<[u8; 4]> {
    if n >= 1 << 28 {
        bail!("tamaño ID3 fuera de rango syncsafe: {n}");
    }
    Ok([
        (n >> 21) as u8 & 0x7F,
        (n >> 14) as u8 & 0x7F,
        (n >> 7) as u8 & 0x7F,
        n as u8 & 0x7F,
    ])
}

fn decode_syncsafe(b: &[u8; 4]) -> usize {
    ((b[0] & 0x7F) as usize) << 21
        | ((b[1] & 0x7F) as usize) << 14
        | ((b[2] & 0x7F) as usize) << 7
        | (b[3] & 0x7F) as usize
}

fn valid_frame_id(id: &[u8; 4]) -> bool {
    id.iter()
        .all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Deshacer el esquema de unsynchronisation FF 00 → FF.
fn deunsync(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        out.push(data[i]);
        if data[i] == 0xFF && data.get(i + 1) == Some(&0x00) {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

impl Id3Frame {
    /// `id` + tamaño syncsafe + flags 00 00 + payload (regla v2.4).
    fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(10 + self.payload.len());
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&encode_syncsafe(self.payload.len())?);
        out.push(0);
        out.push(0);
        out.extend_from_slice(&self.payload);
        Ok(out)
    }
}

/// Tag ID3v2.4 completo: cabecera (flags 0) + frames + relleno NUL
/// (≥ `min_padding`, tamaño total múltiplo de `block`).
pub(crate) fn build_id3_tag(frames: &[Id3Frame], min_padding: usize, block: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    for f in frames {
        body.extend_from_slice(&f.to_bytes()?);
    }
    let mut size = body.len() + min_padding;
    size += (block - size % block) % block;
    let mut out = Vec::with_capacity(10 + size);
    out.extend_from_slice(TAG_MAGIC);
    out.extend_from_slice(&[4, 0, 0]); // v2.4.0, flags 0
    out.extend_from_slice(&encode_syncsafe(size)?);
    out.extend_from_slice(&body);
    out.resize(10 + size, 0);
    Ok(out)
}

/// Frames ID3 de unos bytes de archivo (v2.3/v2.4, tolerante como
/// `parse_tag` del .py). `[]` si no hay tag o es v2.2; Err si el tag
/// está estructuralmente roto.
pub(crate) fn parse_id3_tag(data: &[u8]) -> Result<Vec<Id3Frame>> {
    if data.len() < 10 || &data[..3] != TAG_MAGIC {
        return Ok(vec![]);
    }
    let major = data[3];
    let flags = data[5];
    let size = decode_syncsafe(&[data[6], data[7], data[8], data[9]]);
    let body = data
        .get(10..10 + size)
        .context("tag ID3 corrupto: el tamaño excede los datos")?;
    if major != 3 && major != 4 {
        return Ok(vec![]); // v2.2 y exóticos: nada útil
    }
    let body = if major == 3 && flags & 0x80 != 0 {
        deunsync(body)
    } else {
        body.to_vec()
    };
    let mut pos = 0usize;
    if flags & 0x40 != 0 && body.len() >= 4 {
        // cabecera extendida
        pos = if major == 4 {
            4 + decode_syncsafe(&[body[0], body[1], body[2], body[3]])
        } else {
            4 + u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize
        };
    }
    let mut frames = Vec::new();
    while pos + 10 <= body.len() {
        let fid = [body[pos], body[pos + 1], body[pos + 2], body[pos + 3]];
        if fid[0] == 0 || !valid_frame_id(&fid) {
            break; // relleno cero (o junk) → fin de frames
        }
        let fsize = if major == 4 {
            decode_syncsafe(&[body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]])
        } else {
            u32::from_be_bytes([body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7]]) as usize
        };
        let fflags = [body[pos + 8], body[pos + 9]];
        pos += 10;
        let mut payload = match body.get(pos..pos + fsize) {
            Some(p) => p.to_vec(),
            None => break,
        };
        pos += fsize;
        if major == 4 && fflags[1] & 0x02 != 0 {
            payload = deunsync(&payload);
        }
        if major == 4 && fflags[1] & 0x01 != 0 {
            // indicador de longitud de datos: 4 bytes delante
            payload = payload.get(4..).map(|p| p.to_vec()).unwrap_or_default();
        }
        frames.push(Id3Frame { id: fid, payload });
    }
    Ok(frames)
}

/// Tamaño total del tag ID3 inicial (cabecera incluida); 0 si no hay.
fn id3_tag_size(data: &[u8]) -> usize {
    if data.len() < 10 || &data[..3] != TAG_MAGIC {
        0
    } else {
        10 + decode_syncsafe(&[data[6], data[7], data[8], data[9]])
    }
}

/// Description de un frame GEOB (`None` si no es GEOB o está roto).
fn geob_description(frame: &Id3Frame) -> Option<&[u8]> {
    if frame.id != *b"GEOB" || frame.payload.len() < 2 {
        return None;
    }
    let mut rest = &frame.payload[1..];
    let mut desc = &rest[..0];
    for _ in 0..3 {
        // mime, filename, description (todas NUL-terminadas)
        let term = rest.iter().position(|&b| b == 0)?;
        desc = &rest[..term];
        rest = &rest[term + 1..];
    }
    Some(desc)
}

/// Payload del GEOB (bytes tras la description NUL-terminada).
fn geob_payload(frame: &Id3Frame) -> Option<&[u8]> {
    if frame.id != *b"GEOB" || frame.payload.len() < 2 {
        return None;
    }
    let mut rest = &frame.payload[1..];
    for _ in 0..3 {
        let term = rest.iter().position(|&b| b == 0)?;
        rest = &rest[term + 1..];
    }
    Some(rest)
}

// ------------------------------------------------------ contenedores

/// Chunk AIFF/RIFF: id de 4 chars + payload (el pad byte de chunks de
/// tamaño impar NO se guarda; se re-emite al construir).
pub(crate) struct Chunk {
    pub id: [u8; 4],
    pub payload: Vec<u8>,
}

/// Parsea los chunks de un contenedor (pos 12 en adelante; los tamaños
/// impares llevan 1 byte de pad, como en el .py). `expect_form` valida
/// el tipo de FORM (p. ej. `WAVE`); en AIFF se conserva lo que diga el
/// archivo (AIFF/AIFC).
fn parse_chunks(
    data: &[u8],
    container: &[u8; 4],
    expect_form: Option<&[u8; 4]>,
    big_endian: bool,
) -> Result<Vec<Chunk>> {
    if data.len() < 12 || &data[..4] != container {
        bail!(
            "no es un contenedor {} válido",
            String::from_utf8_lossy(container)
        );
    }
    let form: [u8; 4] = data[8..12].try_into().expect("4 bytes");
    if let Some(f) = expect_form {
        if &form != f {
            bail!(
                "contenedor {}/{:?} inesperado",
                String::from_utf8_lossy(container),
                String::from_utf8_lossy(f)
            );
        }
    }
    let mut chunks = Vec::new();
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let id: [u8; 4] = data[pos..pos + 4].try_into().expect("4 bytes");
        let raw: [u8; 4] = data[pos + 4..pos + 8].try_into().expect("4 bytes");
        let size = if big_endian {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        } as usize;
        let payload = data
            .get(pos + 8..pos + 8 + size)
            .with_context(|| format!("chunk {:?} truncado", String::from_utf8_lossy(&id)))?;
        chunks.push(Chunk {
            id,
            payload: payload.to_vec(),
        });
        pos += 8 + size + (size & 1); // los impares llevan 1 byte de pad
    }
    Ok(chunks)
}

fn build_chunks(
    container: &[u8; 4],
    form_type: &[u8; 4],
    big_endian: bool,
    chunks: &[Chunk],
) -> Vec<u8> {
    let mut body = form_type.to_vec();
    for c in chunks {
        body.extend_from_slice(&c.id);
        let len = (c.payload.len() as u32).to_be_bytes();
        let len_le = (c.payload.len() as u32).to_le_bytes();
        body.extend_from_slice(if big_endian { &len } else { &len_le });
        body.extend_from_slice(&c.payload);
        if c.payload.len() & 1 == 1 {
            body.push(0);
        }
    }
    let mut out = container.to_vec();
    let blen_be = (body.len() as u32).to_be_bytes();
    let blen_le = (body.len() as u32).to_le_bytes();
    out.extend_from_slice(if big_endian {
        &blen_be
    } else {
        &blen_le
    });
    out.extend_from_slice(&body);
    out
}

/// ------------------------------------------------------------- escritura

#[derive(Debug, Clone, Copy, PartialEq)]
enum AudioFormat {
    Mp3,
    Aiff,
    Wav,
}

/// Formato por extensión (`.aif` cuenta como aiff). Err claro para todo
/// lo demás — el equivalente de `SeratoUnsupportedFormatError`.
fn sniff_format(path: &Path) -> Result<AudioFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" => Ok(AudioFormat::Mp3),
        "aiff" | "aif" => Ok(AudioFormat::Aiff),
        "wav" => Ok(AudioFormat::Wav),
        other => bail!(
            "los markers de Serato (GEOB '{GEOB_DESCRIPTION}') solo se escriben en \
             MP3/AIFF/WAV; '.{other}' queda fuera (FLAC usa Vorbis, p. ej.)"
        ),
    }
}

/// Frames ID3 actuales del archivo (`[]` si aún no hay tag).
fn existing_frames(data: &[u8], fmt: AudioFormat) -> Result<Vec<Id3Frame>> {
    match fmt {
        AudioFormat::Mp3 => parse_id3_tag(data),
        AudioFormat::Aiff => {
            let tag = parse_chunks(data, b"FORM", None, true)?
                .into_iter()
                .find(|c| c.id == *b"ID3 ");
            match tag {
                Some(c) => parse_id3_tag(&c.payload),
                None => Ok(vec![]),
            }
        }
        AudioFormat::Wav => {
            let tag = parse_chunks(data, b"RIFF", Some(b"WAVE"), false)?
                .into_iter()
                .find(|c| c.id == *b"id3 ");
            match tag {
                Some(c) => parse_id3_tag(&c.payload),
                None => Ok(vec![]),
            }
        }
    }
}

/// Escritura atómica: tmp junto al destino + rename (reemplaza al
/// destino, igual que `os.replace`).
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .with_context(|| format!("ruta sin nombre de archivo: {}", path.display()))?;
    let mut tmp = path.to_path_buf();
    tmp.set_file_name(format!("{}.sm-mk2.tmp", name.to_string_lossy()));
    if let Err(e) = fs::write(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("no se pudo escribir {}", tmp.display()));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("no se pudo reemplazar {}", path.display()));
    }
    Ok(())
}

/// Añade/reemplaza el GEOB `Serato Markers2` con los hot cues dados.
///
/// Conserva todos los frames existentes byte a byte EXCEPTO un GEOB
/// `Serato Markers2` previo (se sustituye, nunca se duplica); los chunks
/// de audio no se tocan. MP3 → tag ID3 de cabecera; AIFF → chunk
/// `ID3 `; WAV → chunk `id3 ` (se crea si falta). Escritura atómica.
/// Otros formatos → error claro.
///
/// Devuelve el número de cues escritos (≤ [`MAX_CUES`]).
pub fn write_serato_markers(
    path: impl AsRef<Path>,
    cues: &[CueInput],
    bpmlock: bool,
) -> Result<usize> {
    let path = path.as_ref();
    let fmt = sniff_format(path)?;
    if cues.is_empty() {
        bail!("serato markers: no hay cues que escribir");
    }
    let data =
        fs::read(path).with_context(|| format!("no se pudo leer {}", path.display()))?;
    let payload = build_markers2(cues, bpmlock)?;
    let mut frames = existing_frames(&data, fmt)?;
    frames.retain(|f| geob_description(f) != Some(GEOB_DESCRIPTION.as_bytes()));
    frames.push(Id3Frame {
        id: *b"GEOB",
        payload: geob_frame(&payload)[10..].to_vec(),
    });
    let tag = build_id3_tag(&frames, TAG_MIN_PADDING, TAG_BLOCK)?;
    let out = match fmt {
        AudioFormat::Mp3 => {
            let old = id3_tag_size(&data);
            let mut out = tag;
            out.extend_from_slice(data.get(old..).unwrap_or(&[]));
            out
        }
        AudioFormat::Aiff => {
            let form: [u8; 4] = data[8..12].try_into().expect("4 bytes");
            let mut chunks = parse_chunks(&data, b"FORM", None, true)?;
            chunks.retain(|c| c.id != *b"ID3 ");
            chunks.push(Chunk {
                id: *b"ID3 ",
                payload: tag,
            });
            build_chunks(b"FORM", &form, true, &chunks)
        }
        AudioFormat::Wav => {
            let mut chunks = parse_chunks(&data, b"RIFF", Some(b"WAVE"), false)?;
            chunks.retain(|c| c.id != *b"id3 ");
            chunks.push(Chunk {
                id: *b"id3 ",
                payload: tag,
            });
            build_chunks(b"RIFF", b"WAVE", false, &chunks)
        }
    };
    atomic_write(path, &out)?;
    Ok(cues.len().min(MAX_CUES))
}

/// Lee el GEOB `Serato Markers2` del archivo (si existe) → cues +
/// bpmlock. Es el verificador natural de [`write_serato_markers`].
pub fn read_serato_markers(path: impl AsRef<Path>) -> Result<Option<Markers2>> {
    let path = path.as_ref();
    let fmt = sniff_format(path)?;
    let data =
        fs::read(path).with_context(|| format!("no se pudo leer {}", path.display()))?;
    let frames = existing_frames(&data, fmt)?;
    let Some(frame) = frames
        .iter()
        .find(|f| geob_description(f) == Some(GEOB_DESCRIPTION.as_bytes()))
    else {
        return Ok(None);
    };
    let payload = geob_payload(frame).context("frame GEOB Markers2 sin payload")?;
    parse_markers2(payload).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VEC_A: &[CueInput] = &[
        (0.0, Some(1)),
        (12.345, Some(6)),
        (45.678, None),
        (61.5, Some(10)),
        (90.123, Some(3)),
        (120.0, None),
        (150.75, Some(8)),
        (180.002, Some(2)),
    ];

    #[test]
    fn payload_layout_470() {
        let p = build_markers2(VEC_A, true).unwrap();
        assert_eq!(p.len(), ALLOCATED_SIZE);
        assert_eq!(&p[..2], &MARKERS2_VERSION);
        // tras la región base64, todo NUL
        let last = p.iter().rposition(|&b| b != 0).expect("hay contenido");
        assert!(p[last + 1..].iter().all(|&b| b == 0));
        // líneas base64: solo \n como separador, sin \n final, ≤ 72 chars
        let b64 = &p[2..=last];
        assert!(!b64.ends_with(b"\n"));
        let lines: Vec<&[u8]> = b64.split(|&b| b == b'\n').collect();
        assert!(lines.iter().all(|l| l.len() <= BASE64_LINE));
        assert!(
            lines[..lines.len() - 1]
                .iter()
                .all(|l| l.len() == BASE64_LINE)
        );
        // sin padding '=' en el base64
        assert!(!b64.contains(&b'='));
    }

    #[test]
    fn colores_paleta_y_fallback() {
        assert_eq!(energy_color(1), Some([0x00, 0x00, 0xCC]));
        assert_eq!(energy_color(10), Some([0xCC, 0x00, 0x00]));
        assert_eq!(energy_color(0), None); // fuera de paleta
        assert_eq!(energy_color(11), None);
        // sin energía / no mapeable → color por defecto del ÍNDICE
        let cues: &[CueInput] = &[(1.0, None), (2.0, Some(0)), (3.0, Some(99)), (4.0, Some(2))];
        let m = parse_markers2(&build_markers2(cues, false).unwrap()).unwrap();
        assert_eq!(m.cues[0].color, DEFAULT_CUE_COLORS[0]);
        assert_eq!(m.cues[1].color, DEFAULT_CUE_COLORS[1]);
        assert_eq!(m.cues[2].color, DEFAULT_CUE_COLORS[2]);
        assert_eq!(m.cues[3].color, ENERGY_COLORS[1].1);
    }

    #[test]
    fn roundtrip_posiciones_y_bpmlock() {
        for lock in [false, true] {
            let p = build_markers2(VEC_A, lock).unwrap();
            let m = parse_markers2(&p).unwrap();
            assert_eq!(m.cues.len(), VEC_A.len());
            assert_eq!(m.bpmlock, lock);
            for (c, (t, _)) in m.cues.iter().zip(VEC_A) {
                assert!(
                    (c.time_s - t).abs() <= 0.010,
                    "posición {:+} ms fuera de tolerancia",
                    (c.time_s - t) * 1000.0
                );
            }
        }
    }

    #[test]
    fn max_cues_y_orden_de_indices() {
        let many: Vec<CueInput> = (0..12).map(|i| (i as f64, None)).collect();
        let m = parse_markers2(&build_markers2(&many, false).unwrap()).unwrap();
        assert_eq!(m.cues.len(), MAX_CUES);
        assert_eq!(
            m.cues.iter().map(|c| c.index).collect::<Vec<_>>(),
            (0..MAX_CUES as u8).collect::<Vec<_>>()
        );
    }

    #[test]
    fn errores_de_entrada() {
        let e = build_markers2(&[], false).unwrap_err();
        assert!(e.to_string().contains("no hay cues"), "{e}");
        let e = build_markers2(&[(-0.5, None)], false).unwrap_err();
        assert!(e.to_string().contains("fuera de rango"), "{e}");
        let e = build_markers2(&[(5_000_000.0, None)], false).unwrap_err(); // ≥ 2^32 ms
        assert!(e.to_string().contains("fuera de rango"), "{e}");
        // payload corrupto
        let e = parse_markers2(&[0x00, 0x01, 0x41]).unwrap_err();
        assert!(e.to_string().contains("cabecera"), "{e}");
    }

    #[test]
    fn redondeo_estilo_python() {
        // empates exactos a .5 → par (round() de Python, no el de Rust)
        assert_eq!(py_round(2.5), 2.0);
        assert_eq!(py_round(3.5), 4.0);
        assert_eq!(py_round(-0.5), 0.0);
        assert_eq!(py_round(-1.5), -2.0);
        assert_eq!(py_round(2.4), 2.0);
        assert_eq!(py_round(2.6), 3.0);
        assert_eq!(time_to_ms(12.345), 12_345);
    }

    #[test]
    fn geob_frame_layout() {
        let payload = build_markers2(&[(1.0, Some(5))], false).unwrap();
        let frame = geob_frame(&payload);
        assert_eq!(&frame[..4], b"GEOB");
        let size = decode_syncsafe(&[frame[4], frame[5], frame[6], frame[7]]);
        assert_eq!(&frame[8..10], &[0, 0]);
        assert_eq!(frame.len(), 10 + size);
        let body = &frame[10..];
        let expected_prefix: Vec<u8> = {
            let mut v = vec![0u8];
            v.extend_from_slice(GEOB_MIME.as_bytes());
            v.push(0);
            v.push(0);
            v.extend_from_slice(GEOB_DESCRIPTION.as_bytes());
            v.push(0);
            v
        };
        assert_eq!(&body[..expected_prefix.len()], &expected_prefix[..]);
        assert_eq!(&body[expected_prefix.len()..], &payload[..]);
    }

    #[test]
    fn id3_build_parse_roundtrip() {
        let payload = build_markers2(&[(1.0, None)], false).unwrap();
        let frames = vec![
            Id3Frame {
                id: *b"TIT2",
                payload: b"\x03Titulo".to_vec(),
            },
            Id3Frame {
                id: *b"GEOB",
                payload: geob_frame(&payload)[10..].to_vec(),
            },
        ];
        let tag = build_id3_tag(&frames, TAG_MIN_PADDING, TAG_BLOCK).unwrap();
        assert_eq!(&tag[..3], TAG_MAGIC);
        assert_eq!(tag[3], 4); // v2.4
        // el tamaño declarado cabe exacto y el resto es relleno NUL
        let size = decode_syncsafe(&[tag[6], tag[7], tag[8], tag[9]]);
        assert_eq!(tag.len(), 10 + size);
        assert!(tag[10 + size - TAG_MIN_PADDING..].iter().all(|&b| b == 0));
        let parsed = parse_id3_tag(&tag).unwrap();
        assert_eq!(parsed, frames);
        // sin tag → []; el GEOB parseado se reconoce por description
        assert!(parse_id3_tag(b"sin tag").unwrap().is_empty());
        assert_eq!(geob_description(&parsed[1]), Some(GEOB_DESCRIPTION.as_bytes()));
    }

    #[test]
    fn id3_parse_tolerante_v23() {
        // tag v2.3 artesanal: tamaños de frame BE32 planos
        let body = b"TPE1\x00\x00\x00\x04\x00\x00\x03Art";
        let mut tag = Vec::new();
        tag.extend_from_slice(TAG_MAGIC);
        tag.extend_from_slice(&[3, 0, 0]);
        tag.extend_from_slice(&encode_syncsafe(body.len()).unwrap());
        tag.extend_from_slice(body);
        let frames = parse_id3_tag(&tag).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id, *b"TPE1");
        assert_eq!(frames[0].payload, b"\x03Art");
    }

    #[test]
    fn formato_no_soportado_da_error_claro() {
        let dir = std::env::temp_dir().join(format!("sm-mk2-fmt-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for name in ["cancion.flac", "cancion.ogg", "cancion.m4a", "sintxt"] {
            let p = dir.join(name);
            fs::write(&p, b"x").unwrap();
            let err = write_serato_markers(&p, VEC_A, false).unwrap_err();
            assert!(
                err.to_string().contains("solo se escriben en"),
                "{name}: {err}"
            );
        }
    }
}
