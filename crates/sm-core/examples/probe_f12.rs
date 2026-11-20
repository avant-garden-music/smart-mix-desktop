fn main() {
    let p = std::path::Path::new("C:/Users/Crist/AppData/Local/hermes/cache/scratch/test_f12.wma");
    match sm_core::decode::decode_file(&p) {
        Ok(d) => println!("OK: {:.2}s, {} muestras, sr={} ch={}", d.duration_s, d.interleaved.len(), d.sample_rate, d.channels),
        Err(e) => println!("ERR: {e}"),
    }
}
