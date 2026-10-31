// Standalone harness for the unmodified production media.rs.
// Compile in a temporary Cargo project with ogg=0.9.2,
// opus2=0.4.0 (bundled), uuid=1 (v4), and tempfile=3, in release mode.
// HEAR_AUDIT_MEDIA_RS must point to the audited media.rs at compile time.
#[allow(dead_code)]
mod media { include!(env!("HEAR_AUDIT_MEDIA_RS")); }
use std::time::Instant;

fn main() {
    let samples: Vec<f32> = (0..240_000).map(|i| {
        0.1 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin()
    }).collect();
    println!("mode,iteration,push_ms,seal_ms,decode_ms,bytes");
    // Alternate order; each variant encodes the same synthetic 5 s tone.
    // Both use the production writer; only caller input chunking differs.
    for iteration in 0..16 {
        for chunked in if iteration % 2 == 0 { [false, true] } else { [true, false] } {
            let dir = tempfile::tempdir().unwrap();
            let mut block = media::OpusBlock::create(dir.path(), uuid::Uuid::new_v4()).unwrap();
            let at = Instant::now();
            if chunked {
                for chunk in samples.chunks(960) { block.push(chunk).unwrap(); }
            } else { block.push(&samples).unwrap(); }
            let push_ms = at.elapsed().as_secs_f64() * 1000.0;
            let at = Instant::now();
            let sealed = block.seal().unwrap();
            let seal_ms = at.elapsed().as_secs_f64() * 1000.0;
            let at = Instant::now();
            let decoded = media::decode_block(&sealed.path, samples.len()).unwrap();
            let decode_ms = at.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(decoded.len(), samples.len());
            println!("{},{iteration},{push_ms:.4},{seal_ms:.4},{decode_ms:.4},{}",
                if chunked { "frame_input" } else { "whole_block_input" }, sealed.bytes);
        }
    }
}
