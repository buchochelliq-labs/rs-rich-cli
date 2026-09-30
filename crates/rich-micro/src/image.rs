//! Image headers read without decoding (#580): the format, the canvas size
//! and the frame count of a PNG, APNG, GIF or WebP, from its container
//! structure alone. Limits are checked on these numbers before anything is
//! ever decoded, so a decompression bomb is refused by its header.
//!
//! Frames larger than their canvas count at their own size: a decoder
//! allocates them whole.

use crate::error::MicroError;
use crate::model::{ImageFormat, ImageInfo};

/// Read `bytes`' header. `frame_cap` stops counting frames past it (the
/// caller rejects the file anyway), so a hostile file cannot make this walk
/// for long.
pub fn sniff(bytes: &[u8], frame_cap: u32) -> Result<ImageInfo, MicroError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        gif(bytes, frame_cap)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        webp(bytes, frame_cap)
    } else {
        Err(MicroError::Image(
            "not a PNG, APNG, GIF or WebP image".to_string(),
        ))
    }
}

fn truncated(format: &str) -> MicroError {
    MicroError::Image(format!("truncated or malformed {format} header"))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32)
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 3)
        .map(|b| b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16)
}

fn png(bytes: &[u8]) -> Result<ImageInfo, MicroError> {
    let bad = || truncated("PNG");
    // The first chunk must be IHDR.
    if bytes.get(12..16) != Some(b"IHDR") {
        return Err(bad());
    }
    let width = be32(bytes, 16).ok_or_else(bad)?;
    let height = be32(bytes, 20).ok_or_else(bad)?;
    if width == 0 || height == 0 {
        return Err(MicroError::Image("PNG with a zero dimension".to_string()));
    }
    // acTL, if any, comes before the first IDAT.
    let mut at = 8usize;
    let mut frames = 1;
    let mut format = ImageFormat::Png;
    while let Some(length) = be32(bytes, at) {
        let kind = bytes.get(at + 4..at + 8).ok_or_else(bad)?;
        match kind {
            b"acTL" => {
                frames = be32(bytes, at + 8).ok_or_else(bad)?;
                if frames == 0 {
                    return Err(MicroError::Image("APNG with no frames".to_string()));
                }
                format = ImageFormat::Apng;
                break;
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        at = at
            .checked_add(12)
            .and_then(|a| a.checked_add(length as usize))
            .ok_or_else(bad)?;
    }
    Ok(ImageInfo {
        format,
        width,
        height,
        frames,
    })
}

fn gif(bytes: &[u8], frame_cap: u32) -> Result<ImageInfo, MicroError> {
    let bad = || truncated("GIF");
    let mut width = le16(bytes, 6).ok_or_else(bad)?;
    let mut height = le16(bytes, 8).ok_or_else(bad)?;
    let packed = *bytes.get(10).ok_or_else(bad)?;
    let mut at = 13usize;
    if packed & 0x80 != 0 {
        at += 3 << ((packed & 7) + 1);
    }
    // Skip data sub-blocks: a length byte, that many bytes, until a zero.
    let skip_blocks = |mut at: usize| -> Result<usize, MicroError> {
        loop {
            let length = *bytes.get(at).ok_or_else(bad)? as usize;
            at += 1;
            if length == 0 {
                return Ok(at);
            }
            at += length;
        }
    };
    let mut frames = 0u32;
    loop {
        match *bytes.get(at).ok_or_else(bad)? {
            0x3B => break,
            0x21 => at = skip_blocks(at + 2)?,
            0x2C => {
                let frame_width = le16(bytes, at + 5).ok_or_else(bad)?;
                let frame_height = le16(bytes, at + 7).ok_or_else(bad)?;
                width = width.max(frame_width);
                height = height.max(frame_height);
                let local = *bytes.get(at + 9).ok_or_else(bad)?;
                at += 10;
                if local & 0x80 != 0 {
                    at += 3 << ((local & 7) + 1);
                }
                // The LZW minimum code size, then the image data.
                at = skip_blocks(at + 1)?;
                frames += 1;
                if frames > frame_cap {
                    break;
                }
            }
            _ => return Err(bad()),
        }
    }
    if frames == 0 {
        return Err(MicroError::Image("GIF with no frames".to_string()));
    }
    if width == 0 || height == 0 {
        return Err(MicroError::Image("GIF with a zero dimension".to_string()));
    }
    Ok(ImageInfo {
        format: ImageFormat::Gif,
        width,
        height,
        frames,
    })
}

fn webp(bytes: &[u8], frame_cap: u32) -> Result<ImageInfo, MicroError> {
    let bad = || truncated("WebP");
    let mut at = 12usize;
    let mut size: Option<(u32, u32)> = None;
    let mut animated = false;
    let mut frames = 0u32;
    let (mut max_w, mut max_h) = (0u32, 0u32);
    while at + 8 <= bytes.len() {
        let kind = &bytes[at..at + 4];
        let length = le32(bytes, at + 4).ok_or_else(bad)? as usize;
        let data = at + 8;
        match kind {
            b"VP8X" => {
                let flags = *bytes.get(data).ok_or_else(bad)?;
                animated = flags & 0x02 != 0;
                let w = le24(bytes, data + 4).ok_or_else(bad)? + 1;
                let h = le24(bytes, data + 7).ok_or_else(bad)? + 1;
                size = Some((w, h));
            }
            b"VP8 " if size.is_none() => {
                if bytes.get(data + 3..data + 6) != Some(&[0x9d, 0x01, 0x2a]) {
                    return Err(bad());
                }
                let w = le16(bytes, data + 6).ok_or_else(bad)? & 0x3fff;
                let h = le16(bytes, data + 8).ok_or_else(bad)? & 0x3fff;
                size = Some((w, h));
            }
            b"VP8L" if size.is_none() => {
                if bytes.get(data) != Some(&0x2f) {
                    return Err(bad());
                }
                let bits = le32(bytes, data + 1).ok_or_else(bad)?;
                size = Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
            }
            b"ANMF" => {
                let w = le24(bytes, data + 6).ok_or_else(bad)? + 1;
                let h = le24(bytes, data + 9).ok_or_else(bad)? + 1;
                max_w = max_w.max(w);
                max_h = max_h.max(h);
                frames += 1;
                if frames > frame_cap {
                    break;
                }
            }
            _ => {}
        }
        at = data
            .checked_add(length)
            .and_then(|a| a.checked_add(length & 1))
            .ok_or_else(bad)?;
    }
    let (width, height) = size.ok_or_else(bad)?;
    if animated && frames == 0 {
        return Err(MicroError::Image(
            "animated WebP with no frames".to_string(),
        ));
    }
    let (width, height) = (width.max(max_w), height.max(max_h));
    if width == 0 || height == 0 {
        return Err(MicroError::Image("WebP with a zero dimension".to_string()));
    }
    Ok(ImageInfo {
        format: ImageFormat::WebP,
        width,
        height,
        frames: frames.max(1),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG header: signature and IHDR, then `extra` chunks. Enough for
    /// [`sniff`], not a decodable image.
    pub(crate) fn png_header(width: u32, height: u32, frames: Option<u32>) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
            out.extend((data.len() as u32).to_be_bytes());
            out.extend(kind);
            out.extend(data);
            out.extend([0; 4]);
        };
        let mut ihdr = width.to_be_bytes().to_vec();
        ihdr.extend(height.to_be_bytes());
        ihdr.extend([8, 6, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        if let Some(frames) = frames {
            let mut actl = frames.to_be_bytes().to_vec();
            actl.extend(0u32.to_be_bytes());
            chunk(&mut out, b"acTL", &actl);
        }
        chunk(&mut out, b"IDAT", &[]);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    /// A GIF with `frames` 1-pixel-of-data frames at `width`×`height`.
    pub(crate) fn gif_bytes(width: u16, height: u16, frames: u32) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend(width.to_le_bytes());
        out.extend(height.to_le_bytes());
        out.extend([0x80, 0, 0]); // a 2-colour global table
        out.extend([0, 0, 0, 255, 255, 255]);
        for _ in 0..frames {
            out.extend([0x21, 0xF9, 4, 0, 10, 0, 0, 0]); // graphic control
            out.push(0x2C);
            out.extend([0, 0, 0, 0]);
            out.extend(width.to_le_bytes());
            out.extend(height.to_le_bytes());
            out.push(0);
            out.extend([2, 2, 0x4C, 0x01, 0]); // LZW data
        }
        out.push(0x3B);
        out
    }

    #[test]
    fn png_and_apng() {
        let info = sniff(&png_header(16, 8, None), 10).unwrap();
        assert_eq!((info.format, info.width, info.height, info.frames), (ImageFormat::Png, 16, 8, 1));
        let info = sniff(&png_header(16, 8, Some(4)), 10).unwrap();
        assert_eq!((info.format, info.frames), (ImageFormat::Apng, 4));
        assert!(sniff(&png_header(0, 8, None), 10).is_err());
        assert!(sniff(&png_header(16, 8, None)[..20], 10).is_err());
    }

    #[test]
    fn gif_frames_and_truncation() {
        let bytes = gif_bytes(8, 4, 3);
        let info = sniff(&bytes, 10).unwrap();
        assert_eq!((info.format, info.width, info.height, info.frames), (ImageFormat::Gif, 8, 4, 3));
        assert_eq!(sniff(&gif_bytes(8, 4, 50), 5).unwrap().frames, 6);
        assert!(sniff(&bytes[..bytes.len() - 5], 10).is_err());
    }

    #[test]
    fn webp_simple_and_animated() {
        // VP8L: 0x2f, then (w-1) | (h-1) << 14.
        let bits: u32 = 15 | (7 << 14);
        let mut data = vec![0x2f];
        data.extend(bits.to_le_bytes());
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(&data);
        bytes.push(0);
        let info = sniff(&bytes, 10).unwrap();
        assert_eq!((info.width, info.height, info.frames), (16, 8, 1));

        let mut anim = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        anim.extend(10u32.to_le_bytes());
        anim.extend([0x02, 0, 0, 0, 15, 0, 0, 7, 0, 0]);
        for _ in 0..3 {
            anim.extend(b"ANMF");
            anim.extend(16u32.to_le_bytes());
            anim.extend([0, 0, 0, 0, 0, 0, 15, 0, 0, 7, 0, 0, 0, 0, 0, 0]);
        }
        let info = sniff(&anim, 10).unwrap();
        assert_eq!((info.width, info.height, info.frames), (16, 8, 3));
    }

    #[test]
    fn unknown_formats() {
        assert!(sniff(b"hello", 1).is_err());
        assert!(sniff(b"", 1).is_err());
    }
}
