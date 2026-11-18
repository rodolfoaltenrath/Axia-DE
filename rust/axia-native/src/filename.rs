pub fn stem_len(name: &str, folder: bool) -> usize {
    let n = name.chars().count();
    if folder {
        return n;
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].chars().count(),
        _ => n,
    }
}
