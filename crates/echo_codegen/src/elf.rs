//! ELF helpers for AOT link policy (static vs dynamic).

/// True when `bytes` is an ELF that needs a dynamic loader (`PT_INTERP`).
///
/// Non-ELF input is treated as dynamic so `--static` cannot ship a mystery
/// object as a scratch-image binary.
#[must_use]
pub fn elf_needs_interpreter(bytes: &[u8]) -> bool {
    match elf_pt_interp(bytes) {
        Some(has) => has,
        None => true,
    }
}

/// `Some(true)` if `PT_INTERP` is present, `Some(false)` if ELF has no
/// interpreter, `None` if the bytes are not a recognizable ELF.
#[must_use]
pub fn elf_pt_interp(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 64 || bytes[0..4] != *b"\x7fELF" {
        return None;
    }
    let class = bytes[4];
    let data = bytes[5];
    if data != 1 {
        // Big-endian ELF is out of v0 (Linux baseline is little-endian).
        return None;
    }
    let (phoff, phentsize, phnum) = if class == 2 {
        let phoff = u64::from_le_bytes(bytes[32..40].try_into().ok()?) as usize;
        let phentsize = u16::from_le_bytes(bytes[54..56].try_into().ok()?) as usize;
        let phnum = u16::from_le_bytes(bytes[56..58].try_into().ok()?) as usize;
        (phoff, phentsize, phnum)
    } else if class == 1 {
        let phoff = u32::from_le_bytes(bytes[28..32].try_into().ok()?) as usize;
        let phentsize = u16::from_le_bytes(bytes[42..44].try_into().ok()?) as usize;
        let phnum = u16::from_le_bytes(bytes[44..46].try_into().ok()?) as usize;
        (phoff, phentsize, phnum)
    } else {
        return None;
    };
    if phentsize < 4 || phnum == 0 {
        return Some(false);
    }
    const PT_INTERP: u32 = 3;
    for i in 0..phnum {
        let off = phoff.checked_add(i.checked_mul(phentsize)?)?;
        let typ = bytes.get(off..off + 4)?;
        let p_type = u32::from_le_bytes(typ.try_into().ok()?);
        if p_type == PT_INTERP {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le16(n: u16) -> [u8; 2] {
        n.to_le_bytes()
    }
    fn le32(n: u32) -> [u8; 4] {
        n.to_le_bytes()
    }
    fn le64(n: u64) -> [u8; 8] {
        n.to_le_bytes()
    }

    fn elf64(phoff: u64, phentsize: u16, phnum: u16, phdrs: &[[u8; 56]]) -> Vec<u8> {
        let mut b = vec![0u8; 64 + phdrs.len() * 56];
        b[0..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        b[32..40].copy_from_slice(&le64(phoff));
        b[54..56].copy_from_slice(&le16(phentsize));
        b[56..58].copy_from_slice(&le16(phnum));
        for (i, ph) in phdrs.iter().enumerate() {
            let o = phoff as usize + i * 56;
            b[o..o + 56].copy_from_slice(ph);
        }
        b
    }

    fn phdr(p_type: u32) -> [u8; 56] {
        let mut p = [0u8; 56];
        p[0..4].copy_from_slice(&le32(p_type));
        p
    }

    #[test]
    fn rejects_non_elf() {
        assert!(elf_pt_interp(b"not elf").is_none());
        assert!(elf_needs_interpreter(b"not elf"));
    }

    #[test]
    fn static_elf_has_no_interp() {
        let bytes = elf64(64, 56, 1, &[phdr(1)]);
        assert_eq!(elf_pt_interp(&bytes), Some(false));
        assert!(!elf_needs_interpreter(&bytes));
    }

    #[test]
    fn dynamic_elf_has_interp() {
        let bytes = elf64(64, 56, 2, &[phdr(1), phdr(3)]);
        assert_eq!(elf_pt_interp(&bytes), Some(true));
        assert!(elf_needs_interpreter(&bytes));
    }
}
