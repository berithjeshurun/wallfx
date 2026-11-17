//! Appended self-extract overlay: thin PE + zstd(mpv|ffmpeg|thin) + footer.

use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"WFX1";
pub const VERSION: u32 = 1;
pub const FOOTER_LEN: u64 = 48;

#[derive(Debug, Clone)]
pub struct Footer {
    pub pe_size: u64,
    pub compressed_size: u64,
    pub thin_size: u64,
    pub mpv_size: u64,
    pub ffmpeg_size: u64,
}

impl Footer {
    pub fn encode(&self) -> [u8; FOOTER_LEN as usize] {
        let mut buf = [0u8; FOOTER_LEN as usize];
        buf[0..4].copy_from_slice(MAGIC);
        buf[4..8].copy_from_slice(&VERSION.to_le_bytes());
        buf[8..16].copy_from_slice(&self.pe_size.to_le_bytes());
        buf[16..24].copy_from_slice(&self.compressed_size.to_le_bytes());
        buf[24..32].copy_from_slice(&self.thin_size.to_le_bytes());
        buf[32..40].copy_from_slice(&self.mpv_size.to_le_bytes());
        buf[40..48].copy_from_slice(&self.ffmpeg_size.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < FOOTER_LEN as usize {
            bail!("footer too short");
        }
        if &buf[0..4] != MAGIC {
            bail!("overlay magic mismatch");
        }
        let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        if version != VERSION {
            bail!("unsupported overlay version {version}");
        }
        Ok(Self {
            pe_size: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            compressed_size: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            thin_size: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            mpv_size: u64::from_le_bytes(buf[32..40].try_into().unwrap()),
            ffmpeg_size: u64::from_le_bytes(buf[40..48].try_into().unwrap()),
        })
    }
}

#[derive(Debug)]
pub struct Payload {
    pub thin_pe: Vec<u8>,
    pub mpv: Vec<u8>,
    pub ffmpeg: Vec<u8>,
}

pub fn read_footer(path: &Path) -> Result<Option<Footer>> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.len() < FOOTER_LEN {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    file.seek(SeekFrom::End(-(FOOTER_LEN as i64)))?;
    let mut buf = [0u8; FOOTER_LEN as usize];
    file.read_exact(&mut buf)?;
    if &buf[0..4] != MAGIC {
        return Ok(None);
    }
    Ok(Some(Footer::decode(&buf)?))
}

pub fn read_payload(path: &Path) -> Result<Option<(Footer, Payload)>> {
    let Some(footer) = read_footer(path)? else {
        return Ok(None);
    };
    let meta = fs::metadata(path)?;
    let expected = footer
        .pe_size
        .checked_add(footer.compressed_size)
        .and_then(|v| v.checked_add(FOOTER_LEN))
        .context("overlay size overflow")?;
    if meta.len() != expected {
        bail!(
            "overlay size mismatch: file {} bytes, expected {}",
            meta.len(),
            expected
        );
    }

    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(footer.pe_size))?;
    let mut compressed = vec![0u8; footer.compressed_size as usize];
    file.read_exact(&mut compressed)
        .context("read compressed overlay")?;

    let raw = zstd::decode_all(compressed.as_slice()).context("zstd decompress overlay")?;
    let need = (footer.thin_size + footer.mpv_size + footer.ffmpeg_size) as usize;
    if raw.len() != need {
        bail!(
            "decompressed payload size mismatch: got {}, want {}",
            raw.len(),
            need
        );
    }

    let thin_end = footer.thin_size as usize;
    let mpv_end = thin_end + footer.mpv_size as usize;
    let payload = Payload {
        thin_pe: raw[..thin_end].to_vec(),
        mpv: raw[thin_end..mpv_end].to_vec(),
        ffmpeg: raw[mpv_end..].to_vec(),
    };
    Ok(Some((footer, payload)))
}

/// Build raw payload bytes: thin PE || mpv || ffmpeg.
pub fn build_raw_payload(thin_pe: &[u8], mpv: &[u8], ffmpeg: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(thin_pe.len() + mpv.len() + ffmpeg.len());
    raw.extend_from_slice(thin_pe);
    raw.extend_from_slice(mpv);
    raw.extend_from_slice(ffmpeg);
    raw
}

/// Write fat exe = thin PE + zstd(payload) + footer. Overwrites `out`.
pub fn pack_to(out: &Path, thin_pe: &[u8], mpv: &[u8], ffmpeg: &[u8]) -> Result<()> {
    let raw = build_raw_payload(thin_pe, mpv, ffmpeg);
    let compressed = zstd::encode_all(raw.as_slice(), 3).context("zstd compress overlay")?;

    let footer = Footer {
        pe_size: thin_pe.len() as u64,
        compressed_size: compressed.len() as u64,
        thin_size: thin_pe.len() as u64,
        mpv_size: mpv.len() as u64,
        ffmpeg_size: ffmpeg.len() as u64,
    };

    let mut file = File::create(out).with_context(|| format!("create {}", out.display()))?;
    file.write_all(thin_pe)?;
    file.write_all(&compressed)?;
    file.write_all(&footer.encode())?;
    file.flush()?;
    Ok(())
}

/// If `path` already has an overlay, return thin PE bytes; else read whole file as thin.
pub fn load_thin_pe(path: &Path) -> Result<Vec<u8>> {
    if let Some(footer) = read_footer(path)? {
        let mut file = File::open(path)?;
        let mut thin = vec![0u8; footer.pe_size as usize];
        file.read_exact(&mut thin)?;
        return Ok(thin);
    }
    fs::read(path).with_context(|| format!("read {}", path.display()))
}
