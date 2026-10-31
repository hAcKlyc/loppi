use ogg::{PacketWriteEndInfo, PacketWriter};
use opus2::{Application, Bitrate, Channels, Decoder, Encoder};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const RATE: u32 = 48_000;
const FRAME: usize = 960; // 20 ms, one Opus packet.

/// A sealed file is immutable. Its sample count excludes encoder padding.
pub struct SealedAudio {
    pub path: PathBuf,
    pub samples: u64,
    pub bytes: u64,
}

/// Decodes a single private block for local analysis. The caller obtains the
/// path from the core's manifest, never from an untrusted client request.
pub fn decode_block(path: &Path, expected_frames: usize) -> Result<Vec<f32>, String> {
    if expected_frames == 0 || expected_frames > RATE as usize * 11 {
        return Err("invalid block sample count".to_owned());
    }
    let file = File::open(path).map_err(|error| format!("audio block unavailable: {error}"))?;
    if file
        .metadata()
        .map_err(|error| format!("audio block unavailable: {error}"))?
        .len()
        > 10_000_000
    {
        return Err("audio block exceeded size limit".to_owned());
    }
    decode_reader(file, expected_frames)
}

pub(crate) fn decode_reader(
    file: impl std::io::Read + std::io::Seek,
    expected_frames: usize,
) -> Result<Vec<f32>, String> {
    if expected_frames == 0 || expected_frames > RATE as usize * 11 {
        return Err("invalid block sample count".into());
    }
    let mut reader = ogg::PacketReader::new(file);
    let head = reader
        .read_packet()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "audio block lacked Opus header".to_owned())?;
    if head.data.len() < 19 || &head.data[..8] != b"OpusHead" || head.data[9] != 1 {
        return Err("unsupported audio block format".to_owned());
    }
    let skip = u16::from_le_bytes([head.data[10], head.data[11]]) as usize;
    let tags = reader
        .read_packet()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "audio block lacked Opus tags".to_owned())?;
    if tags.data.len() < 8 || &tags.data[..8] != b"OpusTags" {
        return Err("invalid audio block tags".to_owned());
    }
    let mut decoder = Decoder::new(RATE, Channels::Mono).map_err(|error| error.to_string())?;
    let mut decoded = Vec::with_capacity(expected_frames + skip + FRAME);
    let mut frame = [0.0f32; FRAME * 6];
    let mut last_granule = 0;
    while let Some(packet) = reader.read_packet().map_err(|error| error.to_string())? {
        let len = decoder
            .decode_float(&packet.data, &mut frame, false)
            .map_err(|error| error.to_string())?;
        decoded.extend_from_slice(&frame[..len]);
        last_granule = packet.absgp_page();
        if decoded.len() > RATE as usize * 12 {
            return Err("audio block decoded beyond limit".to_owned());
        }
    }
    if last_granule != (expected_frames + skip) as u64 || decoded.len() < expected_frames + skip {
        return Err("audio block duration did not match manifest".to_owned());
    }
    Ok(decoded[skip..skip + expected_frames].to_vec())
}

/// Writes one independently recoverable mono Ogg Opus block. The caller owns
/// the source/time map and only publishes the file after `seal` succeeds.
pub struct OpusBlock {
    pending: PathBuf,
    final_path: PathBuf,
    writer: PacketWriter<'static, File>,
    encoder: Encoder,
    serial: u32,
    lookahead: u16,
    samples: u64,
    pending_pcm: Vec<f32>,
}

impl OpusBlock {
    pub fn create(directory: &Path, id: Uuid) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let pending = directory.join(format!("{id}.pending"));
        let final_path = directory.join(format!("{id}.opus"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(|error| error.to_string())?;
        let mut encoder = Encoder::new(RATE, Channels::Mono, Application::Audio)
            .map_err(|error| error.to_string())?;
        encoder
            .set_bitrate(Bitrate::Bits(48_000))
            .map_err(|error| error.to_string())?;
        let lookahead = u16::try_from(encoder.get_lookahead().map_err(|error| error.to_string())?)
            .map_err(|_| "invalid Opus lookahead".to_owned())?;
        let serial = u32::from_le_bytes(id.as_bytes()[..4].try_into().unwrap());
        let mut writer = PacketWriter::new(file);
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.extend_from_slice(&[1, 1]); // version and mono channel count
        head.extend_from_slice(&lookahead.to_le_bytes());
        head.extend_from_slice(&RATE.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // channel mapping family
        writer
            .write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)
            .map_err(|error| error.to_string())?;
        let vendor = b"loppi";
        let mut tags = Vec::with_capacity(16 + vendor.len());
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        tags.extend_from_slice(vendor);
        tags.extend_from_slice(&0u32.to_le_bytes());
        writer
            .write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            pending,
            final_path,
            writer,
            encoder,
            serial,
            lookahead,
            samples: 0,
            pending_pcm: Vec::with_capacity(FRAME * 2),
        })
    }

    pub fn push(&mut self, mut mono_48k: &[f32]) -> Result<(), String> {
        if !self.pending_pcm.is_empty() {
            let take = (FRAME - self.pending_pcm.len()).min(mono_48k.len());
            self.pending_pcm.extend_from_slice(&mono_48k[..take]);
            mono_48k = &mono_48k[take..];
            if self.pending_pcm.len() == FRAME {
                let mut frame = [0.0; FRAME];
                frame.copy_from_slice(&self.pending_pcm);
                self.write_frame(&frame, FRAME as u64, PacketWriteEndInfo::NormalPacket)?;
                self.pending_pcm.clear();
            }
        }
        let mut frames = mono_48k.chunks_exact(FRAME);
        for frame in &mut frames {
            self.write_frame(frame, FRAME as u64, PacketWriteEndInfo::NormalPacket)?;
        }
        self.pending_pcm.extend_from_slice(frames.remainder());
        Ok(())
    }

    fn write_frame(
        &mut self,
        frame: &[f32],
        real_samples: u64,
        end: PacketWriteEndInfo,
    ) -> Result<(), String> {
        let mut encoded = vec![0u8; 4096];
        let len = self
            .encoder
            .encode_float(frame, &mut encoded)
            .map_err(|error| error.to_string())?;
        encoded.truncate(len);
        self.samples += real_samples;
        self.writer
            .write_packet(
                encoded,
                self.serial,
                end,
                self.lookahead as u64 + self.samples,
            )
            .map_err(|error| error.to_string())
    }

    pub fn seal(mut self) -> Result<SealedAudio, String> {
        let remaining = self.pending_pcm.len();
        let mut last = [0.0f32; FRAME];
        last[..remaining].copy_from_slice(&self.pending_pcm);
        // Opus decoders discard `lookahead` samples at the beginning. When the
        // final real frame nearly fills a packet, one padding packet cannot
        // cover that preskip. Encode another packet while keeping the Ogg end
        // granule at the exact number of real samples plus preskip.
        let encoded_after_last = ((self.samples as usize / FRAME) + 1) * FRAME;
        let needs_more =
            encoded_after_last < self.samples as usize + remaining + self.lookahead as usize;
        self.write_frame(
            &last,
            remaining as u64,
            if needs_more {
                PacketWriteEndInfo::NormalPacket
            } else {
                PacketWriteEndInfo::EndStream
            },
        )?;
        if needs_more {
            self.write_frame(&[0.0; FRAME], 0, PacketWriteEndInfo::EndStream)?;
        }
        self.writer
            .inner_mut()
            .sync_all()
            .map_err(|error| error.to_string())?;
        let samples = self.samples;
        let pending = self.pending.clone();
        let final_path = self.final_path.clone();
        drop(self);
        fs::rename(&pending, &final_path).map_err(|error| error.to_string())?;
        if let Some(parent) = final_path.parent() {
            File::open(parent)
                .and_then(|dir| dir.sync_all())
                .map_err(|error| error.to_string())?;
        }
        let bytes = fs::metadata(&final_path)
            .map_err(|error| error.to_string())?
            .len();
        Ok(SealedAudio {
            path: final_path,
            samples,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogg::PacketReader;

    #[test]
    fn input_chunking_preserves_exact_audio_and_bounds_pending_pcm() {
        let directory = tempfile::tempdir().unwrap();
        let pcm: Vec<f32> = (0..240_557)
            .map(|i| (i as f32 * 0.05).sin() * 0.2)
            .collect();
        let mut whole = OpusBlock::create(directory.path(), Uuid::new_v4()).unwrap();
        whole.push(&pcm).unwrap();
        assert!(whole.pending_pcm.len() < FRAME);
        let whole = whole.seal().unwrap();
        for size in [1, 123, 960, 1007, 48_000] {
            let mut block = OpusBlock::create(directory.path(), Uuid::new_v4()).unwrap();
            for chunk in pcm.chunks(size) {
                block.push(chunk).unwrap();
                assert!(block.pending_pcm.len() < FRAME);
                assert!(block.pending_pcm.capacity() <= FRAME * 2);
            }
            let sealed = block.seal().unwrap();
            assert_eq!(sealed.samples, whole.samples);
            assert_eq!(
                decode_block(&sealed.path, pcm.len()).unwrap(),
                decode_block(&whole.path, pcm.len()).unwrap()
            );
        }
    }

    #[test]
    fn ogg_block_seals_with_playable_headers_and_exact_duration() {
        let temporary = tempfile::tempdir().unwrap();
        let mut block = OpusBlock::create(temporary.path(), Uuid::new_v4()).unwrap();
        block.push(&vec![0.25; 48_555]).unwrap();
        let sealed = block.seal().unwrap();
        assert_eq!(sealed.samples, 48_555);
        assert!(sealed.bytes > 100);
        let mut reader = PacketReader::new(File::open(&sealed.path).unwrap());
        let first = reader.read_packet().unwrap().unwrap();
        assert_eq!(&first.data[..8], b"OpusHead");
        let tags = reader.read_packet().unwrap().unwrap();
        assert_eq!(&tags.data[..8], b"OpusTags");
        let mut last_granule = 0;
        while let Some(packet) = reader.read_packet().unwrap() {
            last_granule = packet.absgp_page();
        }
        assert_eq!(
            last_granule,
            sealed.samples + u16::from_le_bytes([first.data[10], first.data[11]]) as u64
        );
        assert_eq!(
            decode_block(&sealed.path, sealed.samples as usize)
                .unwrap()
                .len(),
            48_555
        );
    }

    #[test]
    fn partial_tail_with_nearly_full_packet_survives_preskip() {
        let temporary = tempfile::tempdir().unwrap();
        let mut block = OpusBlock::create(temporary.path(), Uuid::new_v4()).unwrap();
        block.push(&vec![0.25; 225_536]).unwrap();
        let sealed = block.seal().unwrap();
        assert_eq!(decode_block(&sealed.path, 225_536).unwrap().len(), 225_536);
    }
}
