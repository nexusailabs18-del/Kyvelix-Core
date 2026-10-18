//! Arena parser: read a Loop from bincode bytes directly into an Arena.
//! No intermediate allocations. All strings and vectors live in the arena.

use crate::arena::Arena;
use crate::ir::*;

/// Read a LoopRef from bincode bytes, allocating everything from `arena`.
///
/// Format matches the current bincode layout of `Loop`, `IV`, `Access`:
///   depth: u64 (bincode encodes usize as u64)
///   ivs: Vec<IV>     (u64 length, then each IV)
///   body: Vec<Access> (u64 length, then each Access)
///
/// Each IV: name (String = u64 len + bytes), lower i64, upper i64, step i64
/// Each Access: kind (u8), base (String), index (Vec<i64>), elem_size i64, offset i64
pub fn parse_loop<'a>(arena: &'a Arena, bytes: &'a [u8]) -> Option<LoopRef<'a>> {
    let mut cursor = Cursor { pos: 0, data: bytes };

    let depth = cursor.read_u64()? as usize;

    // IVs
    let n_ivs = cursor.read_u64()? as usize;
    if n_ivs > 16 { return None; }
    let ivs_raw: &mut [IVRef] = arena.alloc_uninit_slice(n_ivs);
    for i in 0..n_ivs {
        let name_len = cursor.read_u64()? as usize;
        if name_len > 1024 { return None; }
        let name_bytes = cursor.read_bytes(name_len)?;
        let name: &str = std::str::from_utf8(name_bytes).ok()?;
        let lower = cursor.read_i64()?;
        let upper = cursor.read_i64()?;
        let step = cursor.read_i64()?;
        ivs_raw[i] = IVRef { name, lower, upper, step };
    }

    // Body
    let n_acc = cursor.read_u64()? as usize;
    if n_acc > 32 { return None; }
    let body_raw: &mut [AccessRef] = arena.alloc_uninit_slice(n_acc);
    for i in 0..n_acc {
        let kind_byte = cursor.read_u8()?;
        let kind = match kind_byte {
            0 => AccessKind::Read,
            1 => AccessKind::Write,
            _ => return None,
        };
        let base_len = cursor.read_u64()? as usize;
        if base_len > 256 { return None; }
        let base_bytes = cursor.read_bytes(base_len)?;
        let base: &str = std::str::from_utf8(base_bytes).ok()?;

        let idx_len = cursor.read_u64()? as usize;
        if idx_len > 16 { return None; }
        let mut idx_tmp: [i64; 16] = [0; 16];
        for k in 0..idx_len {
            idx_tmp[k] = cursor.read_i64()?;
        }
        let index: &[i64] = arena.alloc_slice(&idx_tmp[..idx_len]);

        let elem_size = cursor.read_i64()?;
        let offset = cursor.read_i64()?;

        body_raw[i] = AccessRef { kind, base, index, elem_size, offset };
    }

    let loop_ref = LoopRef {
        depth,
        ivs: ivs_raw,
        body: body_raw,
    };

    // Validate before returning
    if loop_ref.validate().is_err() {
        return None;
    }

    Some(loop_ref)
}

struct Cursor<'a> {
    pos: usize,
    data: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn read_u8(&mut self) -> Option<u8> {
        if self.pos >= self.data.len() { return None; }
        let v = self.data[self.pos];
        self.pos += 1;
        Some(v)
    }
    fn read_u64(&mut self) -> Option<u64> {
        if self.pos + 8 > self.data.len() { return None; }
        let v = u64::from_le_bytes(self.data[self.pos..self.pos+8].try_into().ok()?);
        self.pos += 8;
        Some(v)
    }
    fn read_i64(&mut self) -> Option<i64> {
        if self.pos + 8 > self.data.len() { return None; }
        let v = i64::from_le_bytes(self.data[self.pos..self.pos+8].try_into().ok()?);
        self.pos += 8;
        Some(v)
    }
    fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n > self.data.len() { return None; }
        let v = &self.data[self.pos..self.pos+n];
        self.pos += n;
        Some(v)
    }
}
