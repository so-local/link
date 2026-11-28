/// The copy's progress counts the files of the stream as it passes,
/// in pieces of any size, and its bytes.
#[test]
fn the_copy_counts_its_files_as_they_pass() {
    let mut tar = tar::Builder::new(Vec::new());
    for (name, size) in [("a", 10usize), ("b", 0), ("c", 1000)] {
        let mut header = tar::Header::new_gnu();
        header.set_size(size as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, vec![7u8; size].as_slice())
            .unwrap();
    }
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(0);
    header.set_cksum();
    tar.append_data(&mut header, "d", std::io::empty()).unwrap();
    let bytes = tar.into_inner().unwrap();
    for piece in [1, 7, 512, 4096] {
        let mut count = super::Count::default();
        for chunk in bytes.chunks(piece) {
            count.add(chunk);
        }
        assert_eq!(count.files, 3, "in pieces of {piece}");
        assert_eq!(count.bytes, bytes.len() as u64);
    }
    assert_eq!(super::human(999), "999 bytes");
    assert_eq!(super::human(4_200_000), "4.2 MB");
}
