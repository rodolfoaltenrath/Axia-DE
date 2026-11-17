//! Build-time-only ELF packer: accepts self-contained BPF programs and map relocations.
//! No ELF parsing happens in the privileged service.
#[derive(Debug)]
pub struct Packed {
    pub section: String,
    pub name: String,
    pub insns: Vec<u64>,
    pub relocs: Vec<(usize, String)>,
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn at(&self, o: u64, n: u64) -> Result<&'a [u8], String> {
        let end = o.checked_add(n).ok_or("offset overflow")?;
        self.0
            .get(
                usize::try_from(o).map_err(|_| "offset overflow")?
                    ..usize::try_from(end).map_err(|_| "offset overflow")?,
            )
            .ok_or_else(|| "outside ELF file".into())
    }
    fn u16(&self, o: u64) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.at(o, 2)?.try_into().unwrap()))
    }
    fn u32(&self, o: u64) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.at(o, 4)?.try_into().unwrap()))
    }
    fn u64(&self, o: u64) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.at(o, 8)?.try_into().unwrap()))
    }
}
#[derive(Clone)]
struct Section {
    name: String,
    ty: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: usize,
    info: usize,
    ent: u64,
}
fn string(b: &[u8], o: usize) -> Result<String, String> {
    let b = b.get(o..).ok_or("string outside table")?;
    let n = b
        .iter()
        .position(|&c| c == 0)
        .ok_or("unterminated string")?;
    std::str::from_utf8(&b[..n])
        .map(str::to_owned)
        .map_err(|_| "non-UTF8 symbol".into())
}
pub fn pack(bytes: &[u8]) -> Result<Vec<Packed>, String> {
    let r = Reader(bytes);
    if bytes.len() > 64 << 20
        || r.at(0, 7)? != b"\x7fELF\x02\x01\x01"
        || r.u16(16)? != 1
        || r.u16(18)? != 247
        || r.u16(58)? != 64
    {
        return Err("not a 64-bit little-endian BPF relocatable object".into());
    }
    let off = r.u64(40)?;
    let count = r.u16(60)? as usize;
    let si = r.u16(62)? as usize;
    if count == 0 || si >= count {
        return Err("no section names".into());
    }
    r.at(off, count as u64 * 64)?;
    let p = off + si as u64 * 64;
    let names = r.at(r.u64(p + 24)?, r.u64(p + 32)?)?;
    let mut sections = vec![];
    for i in 0..count {
        let o = off + i as u64 * 64;
        let s = Section {
            name: string(names, r.u32(o)? as usize)?,
            ty: r.u32(o + 4)?,
            flags: r.u64(o + 8)?,
            offset: r.u64(o + 24)?,
            size: r.u64(o + 32)?,
            link: r.u32(o + 40)? as usize,
            info: r.u32(o + 44)? as usize,
            ent: r.u64(o + 56)?,
        };
        if s.size != 0
            && (s.name == ".text"
                || s.name.starts_with(".data")
                || s.name.starts_with(".rodata")
                || s.name == ".bss")
        {
            return Err(format!("unsupported runtime section {}", s.name));
        }
        if s.ty != 8 {
            r.at(s.offset, s.size)?;
        }
        sections.push(s);
    }
    let sy = sections
        .iter()
        .position(|s| s.ty == 2)
        .ok_or("missing symbols")?;
    let maps = sections
        .iter()
        .position(|s| s.name == "maps")
        .ok_or("missing maps")?;
    let sym = &sections[sy];
    if sym.ent != 24 || sym.size % 24 != 0 {
        return Err("invalid symbols".into());
    }
    let strsec = sections.get(sym.link).ok_or("invalid strings")?;
    let strings = r.at(strsec.offset, strsec.size)?;
    let ns = sym.size / 24;
    let mut out = vec![];
    for (i, s) in sections.iter().enumerate() {
        if s.ty != 1 || s.flags & 4 == 0 || s.size == 0 {
            continue;
        }
        if !s.name.starts_with("cgroup/")
            || s.name.len() > 63
            || s.size % 8 != 0
            || s.size / 8 > 1_000_000
            || out.len() >= 32
        {
            return Err("invalid program section".into());
        }
        let mut function = None;
        for k in 0..ns {
            let o = sym.offset + k * 24;
            if r.u16(o + 6)? as usize == i && r.at(o + 4, 1)?[0] & 15 == 2 {
                if function.is_some() || r.u64(o + 8)? != 0 || r.u64(o + 16)? != s.size {
                    return Err("program must be one inlined function".into());
                }
                function = Some(string(strings, r.u32(o)? as usize)?);
            }
        }
        let name = function.ok_or("unnamed program")?;
        let code = r.at(s.offset, s.size)?;
        let insns: Vec<u64> = code
            .chunks_exact(8)
            .map(|v| u64::from_le_bytes(v.try_into().unwrap()))
            .collect();
        let mut relocs = vec![];
        for rs in &sections {
            if rs.info != i || !matches!(rs.ty, 4 | 9) {
                continue;
            }
            if rs.ty != 9 || rs.ent != 16 || rs.size % 16 != 0 || rs.link != sy {
                return Err("unsupported relocation table".into());
            }
            for k in 0..rs.size / 16 {
                let o = rs.offset + k * 16;
                let offset = r.u64(o)?;
                let info = r.u64(o + 8)?;
                let symidx = info >> 32;
                if info as u32 != 1
                    || symidx >= ns
                    || offset % 8 != 0
                    || offset / 8 + 1 >= insns.len() as u64
                {
                    return Err("unsupported relocation".into());
                }
                let sp = sym.offset + symidx * 24;
                if r.u16(sp + 6)? as usize != maps {
                    return Err("relocation is not a map".into());
                }
                let n = string(strings, r.u32(sp)? as usize)?;
                if n.is_empty()
                    || !n
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                {
                    return Err("invalid map name".into());
                }
                let pc = (offset / 8) as usize;
                if insns[pc] & 255 != 0x18
                    || insns[pc + 1] != 0
                    || !relocs.iter().all(|(i, _)| *i != pc)
                {
                    return Err("invalid map instruction".into());
                }
                relocs.push((pc, n));
            }
        }
        // No unresolved function calls or global addresses can reach the loader.
        for (pc, &ins) in insns.iter().enumerate() {
            if ins & 255 == 0x85 && (ins >> 12) & 15 != 0 {
                return Err("non-inlined BPF call".into());
            }
            if ins & 255 == 0x18 && !relocs.iter().any(|(i, _)| *i == pc) && (ins >> 12) & 15 != 0 {
                return Err("unsupported global address".into());
            }
        }
        out.push(Packed {
            section: s.name.clone(),
            name,
            insns,
            relocs,
        });
    }
    if out.is_empty() {
        Err("no programs".into())
    } else {
        Ok(out)
    }
}
pub fn emit(programs: &[Packed]) -> String {
    let mut s = String::from("pub static PROGRAMS: &[Program] = &[\n");
    for p in programs {
        s.push_str(&format!(
            "Program{{section:{:?},name:{:?},insns:&{:?},relocs:&{:?}}},\n",
            p.section, p.name, p.insns, p.relocs
        ));
    }
    s.push_str("];\n");
    s
}
