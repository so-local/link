//! A page sent compressed though nobody asked. The tool sends no
//! `Accept-Encoding`, so a server that compresses anyway, with gzip or
//! deflate (the two HTTP has always had), is undone here, up to the
//! page's most; any other is refused by name.

use super::*;

/// `body`, sent in `encoding` (its `Content-Encoding`), as it was before
/// it was compressed: at most `most` bytes. A stream cut short, as the
/// page's most cuts one, gives what came before the cut.
pub(super) fn inflated(
    encoding: Option<&str>,
    body: Vec<u8>,
    most: u64,
) -> anyhow::Result<Vec<u8>> {
    let encoding = encoding.map(|e| e.trim().to_ascii_lowercase());
    let reader: Box<dyn Read + '_> = match encoding.as_deref() {
        None | Some("" | "identity") => return Ok(body),
        Some("gzip" | "x-gzip") => Box::new(flate2::read::MultiGzDecoder::new(&body[..])),
        Some("deflate") => Box::new(flate2::read::ZlibDecoder::new(&body[..])),
        Some(other) => bail!(
            "the page came compressed as {}, which this tool didn't ask for and doesn't read",
            link_format::clean(other)
        ),
    };
    let mut out = Vec::new();
    match reader.take(most).read_to_end(&mut out) {
        Ok(_) => Ok(out),
        Err(_) if !out.is_empty() => Ok(out),
        Err(err) => Err(err).context("undoing the page's compression"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        out.write_all(bytes).unwrap();
        out.finish().unwrap()
    }

    /// gzip and deflate are undone, a body sent as it is stays so, and
    /// an encoding it can't undo is refused by its name.
    #[test]
    fn a_page_sent_compressed_unasked_is_undone() {
        let page = b"<html><p>plain words</p></html>".to_vec();
        assert_eq!(inflated(Some("gzip"), gzip(&page), 1000).unwrap(), page);
        assert_eq!(inflated(Some(" X-Gzip "), gzip(&page), 1000).unwrap(), page);
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zlib.write_all(&page).unwrap();
        assert_eq!(
            inflated(Some("deflate"), zlib.finish().unwrap(), 1000).unwrap(),
            page
        );
        assert_eq!(inflated(None, page.clone(), 1000).unwrap(), page);
        assert_eq!(
            inflated(Some("identity"), page.clone(), 1000).unwrap(),
            page
        );
        let err = inflated(Some("br"), page.clone(), 1000).unwrap_err();
        assert!(err.to_string().contains("compressed as br"), "{err}");
        assert!(inflated(Some("gzip"), page, 1000).is_err());
    }

    /// What it undoes stops at the most, however far the stream would
    /// grow, and a stream cut short gives what came before the cut.
    #[test]
    fn an_undone_page_stops_at_its_most() {
        let zeros = gzip(&vec![b'a'; 1 << 20]);
        assert_eq!(inflated(Some("gzip"), zeros, 4096).unwrap().len(), 4096);
        let words = b"word ".repeat(4000);
        let mut cut = gzip(&words);
        cut.truncate(cut.len() / 2);
        let start = inflated(Some("gzip"), cut, 1 << 20).unwrap();
        assert!(!start.is_empty() && words.starts_with(&start));
    }
}
