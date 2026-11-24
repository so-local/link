//! A screenshot as PNG: 8-bit RGB, each row unfiltered, deflated.

use std::io::Write;

/// `rgb`, `width` by `height` pixels of three bytes each, as a PNG.
pub fn encode(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let row = width as usize * 3;
    assert_eq!(rgb.len(), row * height as usize, "pixels for the size");
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    // 8 bits a sample, RGB, deflate, no filter method, not interlaced.
    header.extend([8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    let mut deflate = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    for line in rgb.chunks(row) {
        // Each row's filter: none.
        deflate.write_all(&[0]).expect("writing to memory");
        deflate.write_all(line).expect("writing to memory");
    }
    chunk(
        &mut out,
        b"IDAT",
        &deflate.finish().expect("writing to memory"),
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend(kind);
    out.extend(data);
    out.extend(crc.finalize().to_be_bytes());
}

/// A PNG of [`encode`]'s back to its size and pixels: for tests.
pub fn decode(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    use std::io::Read;
    let png = png.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
    let (mut at, mut size, mut data) = (0, None, Vec::new());
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().ok()?) as usize;
        let kind = &png[at + 4..at + 8];
        let body = png.get(at + 8..at + 8 + len)?;
        match kind {
            b"IHDR" => {
                let width = u32::from_be_bytes(body[0..4].try_into().ok()?);
                let height = u32::from_be_bytes(body[4..8].try_into().ok()?);
                size = Some((width, height));
            }
            b"IDAT" => data.extend(body),
            _ => {}
        }
        at += 12 + len;
    }
    let (width, height) = size?;
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&data[..])
        .read_to_end(&mut raw)
        .ok()?;
    let row = width as usize * 3;
    let rgb = raw
        .chunks(row + 1)
        .flat_map(|line| line[1..].to_vec())
        .collect();
    Some((width, height, rgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_pixels_come_back() {
        let rgb: Vec<u8> = (0..4 * 3 * 3).map(|n| n as u8).collect();
        let png = encode(4, 3, &rgb);
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(decode(&png), Some((4, 3, rgb)));
    }
}
