//! Tests de sm-library: WAV PCM generado a mano + read_metadata (lofty),
//! escaneo de directorios, importación M3U y duplicados por metadatos.

use std::fs;
use std::path::{Path, PathBuf};

use sm_library::{find_duplicates, import_m3u, read_metadata, scan_root};
use sm_model::TrackMeta;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sm-lib-test-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// WAV PCM canónico de 44 bytes de cabecera + `seconds` de silencio.
fn write_wav(path: &Path, seconds: u32, sample_rate: u32, channels: u16, bits: u16) {
    let n_samples = seconds as u64 * sample_rate as u64 * channels as u64;
    let data_len = (n_samples * (bits as u64 / 8)) as u32;
    let mut b: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * bits as u32 / 8;
    b.extend_from_slice(&byte_rate.to_le_bytes());
    b.extend_from_slice(&(channels * bits / 8).to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    b.extend(vec![0u8; data_len as usize]);
    fs::write(path, b).unwrap();
}

#[test]
fn read_metadata_wav_generado() {
    let dir = temp_dir("wav");
    let wav = dir.join("silence-1s.wav");
    write_wav(&wav, 1, 44_100, 1, 16);

    let scanned = read_metadata(&wav).expect("read_metadata sobre WAV válido");
    let m = &scanned.meta;
    assert!((m.duration_s - 1.0).abs() < 0.05, "duration_s = {}", m.duration_s);
    assert_eq!(m.channels, Some(1));
    assert_eq!(m.sample_rate, Some(44_100));
    assert_eq!(m.format, "wav");
    assert_eq!(m.file_size, 44 + 44_100 * 2);
    assert!(m.file_modified_unix > 0);
    assert_eq!(m.path, wav.to_string_lossy());
    // WAV sin chunk LIST/INFO: lofty no da bitrate calculable
    println!("read_metadata wav: {m:?}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_metadata_wav_estereo_2s() {
    let dir = temp_dir("wav2");
    let wav = dir.join("stereo.wav");
    write_wav(&wav, 2, 48_000, 2, 24);

    let m = &read_metadata(&wav).unwrap().meta;
    assert!((m.duration_s - 2.0).abs() < 0.05, "duration_s = {}", m.duration_s);
    assert_eq!(m.channels, Some(2));
    assert_eq!(m.sample_rate, Some(48_000));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scan_root_filtra_extensiones_y_ocultos() {
    let dir = temp_dir("scan");
    fs::write(dir.join("a.mp3"), b"x").unwrap();
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("b.WAV"), b"x").unwrap();
    fs::create_dir_all(dir.join(".hidden")).unwrap();
    fs::write(dir.join(".hidden").join("c.mp3"), b"x").unwrap();
    fs::create_dir_all(dir.join(".git")).unwrap();
    fs::write(dir.join(".git").join("d.flac"), b"x").unwrap();
    fs::write(dir.join("readme.txt"), b"x").unwrap();
    fs::write(dir.join("noext"), b"x").unwrap();

    let found = scan_root(&dir);
    let names: Vec<String> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["a.mp3", "b.WAV"], "encontrados: {found:?}");
    // ordenados y como rutas absolutas del archivo
    assert!(found[0].ends_with("a.mp3"));

    // raíz inexistente → vacío, sin pánico
    assert!(scan_root(&dir.join("no-existe")).is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn import_m3u_con_rutas_absolutas_y_relativas() {
    let dir = temp_dir("m3u");
    let abs1 = dir.join("one.mp3");
    let abs2 = dir.join("two.mp3");
    let content = format!(
        "#EXTM3U\n\
         #EXTINF:123,Artist - Title\n\
         {}\n\
         sub/three.mp3\n\
         \n\
         # comentario\n\
         {}\n",
        abs1.display(),
        abs2.display()
    );
    let m3u = dir.join("list.m3u8");
    fs::write(&m3u, content).unwrap();

    let entries = import_m3u(&m3u).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(Path::new(&entries[0]), abs1);
    assert_eq!(Path::new(&entries[2]), abs2);
    // relativa resuelta contra el dir del m3u
    assert_eq!(Path::new(&entries[1]), dir.join("sub").join("three.mp3"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn find_duplicates_duplicado_y_unico() {
    let mk = |id: i64, path: &str, title: &str, artist: &str, dur: f64| {
        (
            id,
            TrackMeta {
                path: path.into(),
                title: title.into(),
                artist: artist.into(),
                duration_s: dur,
                ..Default::default()
            },
        )
    };
    let metas = vec![
        mk(1, "C:/a/summer jam.mp3", "Summer Jam", "DJ Luna", 180.0),
        mk(2, "C:/b/SUMMER JAM!!.mp3", "summer  jam", "dj  luna", 181.5), // ±1.5 s → dup
        mk(3, "C:/c/anthem.mp3", "Anthem", "Solo", 150.0),                // único
        mk(4, "C:/d/summer jam live.mp3", "Summer Jam", "DJ Luna", 400.0), // misma key, dur lejana → no
    ];

    let groups = find_duplicates(&metas);
    assert_eq!(groups.len(), 1, "grupos: {groups:?}");
    let g = &groups[0];
    assert_eq!(g.reason, "artist+title");
    assert_eq!(g.tracks.len(), 2);
    let ids: Vec<i64> = g.tracks.iter().map(|t| t.0).collect();
    assert_eq!(ids, vec![1, 2]);
    assert!((g.tracks[0].2 - 180.0).abs() < f64::EPSILON);

    // sin tags → sin grupos
    let untagged = vec![mk(10, "C:/x.mp3", "", "", 100.0), mk(11, "C:/y.mp3", "", "", 100.0)];
    assert!(find_duplicates(&untagged).is_empty());
}
