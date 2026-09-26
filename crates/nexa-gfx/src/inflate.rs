//! DEFLATE(RFC 1951) · zlib(RFC 1950) 풀기 — 외부 crate 0(DR-3) · PNG 디코더([`crate::image`])의 바탕.
//!
//! puff(zlib 참조 구현)식 단순 디코더: 정적/동적 허프만 · 저장 블록 · 비트 단위 읽기. 속도보다 정확·작음을 택했다(미리보기용 ≤ 수십 MB).
//! 실패는 전부 `Err(&'static str)`(패닉 없음 · 손상 입력 = 오류).

/// zlib 스트림(2바이트 머리 + DEFLATE + Adler-32) → 원문. Adler-32는 검증한다.
pub fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 6 {
        return Err("zlib: too short");
    }
    let cmf = data[0];
    let flg = data[1];
    if cmf & 0x0f != 8 || (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err("zlib: bad header");
    }
    if flg & 0x20 != 0 {
        return Err("zlib: preset dictionary not supported");
    }
    let (out, used) = inflate_raw(&data[2..])?;
    let tail = &data[2 + used..];
    if tail.len() >= 4 {
        let want = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
        if adler32(&out) != want {
            return Err("zlib: adler32 mismatch");
        }
    }
    Ok(out)
}

/// 원시 DEFLATE → (원문 · 소비한 바이트 수).
pub fn inflate_raw(data: &[u8]) -> Result<(Vec<u8>, usize), &'static str> {
    let mut st = State {
        data,
        pos: 0,
        bitbuf: 0,
        bitcnt: 0,
        out: Vec::with_capacity(data.len() * 3),
    };
    loop {
        let last = st.bits(1)? == 1;
        match st.bits(2)? {
            0 => st.stored()?,
            1 => {
                let (lit, dist) = fixed_tables();
                st.codes(&lit, &dist)?;
            }
            2 => {
                let (lit, dist) = st.dynamic_tables()?;
                st.codes(&lit, &dist)?;
            }
            _ => return Err("deflate: bad block type"),
        }
        if last {
            break;
        }
    }
    Ok((st.out, st.pos))
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

const MAXBITS: usize = 15;

/// 정규 허프만 표(길이별 개수 + 심볼 순서) — puff 방식.
struct Huffman {
    count: [u16; MAXBITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, &'static str> {
        let mut count = [0u16; MAXBITS + 1];
        for &l in lengths {
            count[l as usize] += 1;
        }
        if count[0] as usize == lengths.len() {
            // 코드가 하나도 없다(허용 · 디코드 시 오류).
            return Ok(Huffman {
                count,
                symbol: Vec::new(),
            });
        }
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left <<= 1;
            left -= i32::from(c);
            if left < 0 {
                return Err("deflate: over-subscribed code");
            }
        }
        let mut offs = [0u16; MAXBITS + 1];
        for len in 1..MAXBITS {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }
}

struct State<'a> {
    data: &'a [u8],
    pos: usize,
    bitbuf: u32,
    bitcnt: u32,
    out: Vec<u8>,
}

impl State<'_> {
    fn bits(&mut self, need: u32) -> Result<u32, &'static str> {
        let mut val = self.bitbuf;
        while self.bitcnt < need {
            let Some(&b) = self.data.get(self.pos) else {
                return Err("deflate: unexpected end");
            };
            self.pos += 1;
            val |= u32::from(b) << self.bitcnt;
            self.bitcnt += 8;
        }
        self.bitbuf = val >> need;
        self.bitcnt -= need;
        Ok(val & ((1u32 << need) - 1))
    }

    fn decode(&mut self, h: &Huffman) -> Result<u16, &'static str> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAXBITS {
            code |= self.bits(1)? as i32;
            let count = i32::from(h.count[len]);
            if code - count < first {
                return h
                    .symbol
                    .get((index + (code - first)) as usize)
                    .copied()
                    .ok_or("deflate: bad code");
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("deflate: bad code")
    }

    fn stored(&mut self) -> Result<(), &'static str> {
        self.bitbuf = 0;
        self.bitcnt = 0;
        if self.pos + 4 > self.data.len() {
            return Err("deflate: stored header");
        }
        let len = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]) as usize;
        let nlen = u16::from_le_bytes([self.data[self.pos + 2], self.data[self.pos + 3]]) as usize;
        if len != (!nlen & 0xffff) {
            return Err("deflate: stored length check");
        }
        self.pos += 4;
        if self.pos + len > self.data.len() {
            return Err("deflate: stored data");
        }
        self.out
            .extend_from_slice(&self.data[self.pos..self.pos + len]);
        self.pos += len;
        Ok(())
    }

    fn codes(&mut self, lit: &Huffman, dist: &Huffman) -> Result<(), &'static str> {
        const LBASE: [u16; 29] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227, 258,
        ];
        const LEXT: [u8; 29] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
        ];
        const DBASE: [u16; 30] = [
            1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
            1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
        ];
        const DEXT: [u8; 30] = [
            0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
            12, 13, 13,
        ];
        loop {
            let sym = self.decode(lit)?;
            if sym < 256 {
                self.out.push(sym as u8);
            } else if sym == 256 {
                return Ok(());
            } else {
                let i = (sym - 257) as usize;
                if i >= 29 {
                    return Err("deflate: bad length code");
                }
                let len = LBASE[i] as usize + self.bits(u32::from(LEXT[i]))? as usize;
                let d = self.decode(dist)? as usize;
                if d >= 30 {
                    return Err("deflate: bad distance code");
                }
                let dist_v = DBASE[d] as usize + self.bits(u32::from(DEXT[d]))? as usize;
                if dist_v > self.out.len() {
                    return Err("deflate: distance too far");
                }
                let start = self.out.len() - dist_v;
                for k in 0..len {
                    let b = self.out[start + k];
                    self.out.push(b);
                }
            }
        }
    }

    fn dynamic_tables(&mut self) -> Result<(Huffman, Huffman), &'static str> {
        const ORDER: [usize; 19] = [
            16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
        ];
        let nlen = self.bits(5)? as usize + 257;
        let ndist = self.bits(5)? as usize + 1;
        let ncode = self.bits(4)? as usize + 4;
        if nlen > 286 || ndist > 30 {
            return Err("deflate: bad counts");
        }
        let mut lengths = [0u8; 19];
        for &o in ORDER.iter().take(ncode) {
            lengths[o] = self.bits(3)? as u8;
        }
        let lencode = Huffman::new(&lengths)?;
        let mut all = vec![0u8; nlen + ndist];
        let mut i = 0;
        while i < nlen + ndist {
            let sym = self.decode(&lencode)?;
            if sym < 16 {
                all[i] = sym as u8;
                i += 1;
            } else {
                let (rep_len, count) = match sym {
                    16 => {
                        if i == 0 {
                            return Err("deflate: repeat with no first");
                        }
                        (all[i - 1], 3 + self.bits(2)? as usize)
                    }
                    17 => (0, 3 + self.bits(3)? as usize),
                    _ => (0, 11 + self.bits(7)? as usize),
                };
                if i + count > nlen + ndist {
                    return Err("deflate: too many lengths");
                }
                for _ in 0..count {
                    all[i] = rep_len;
                    i += 1;
                }
            }
        }
        if all[256] == 0 {
            return Err("deflate: no end-of-block code");
        }
        let lit = Huffman::new(&all[..nlen])?;
        let dist = Huffman::new(&all[nlen..])?;
        Ok((lit, dist))
    }
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lengths = [0u8; 288];
    for (i, l) in lengths.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let lit = Huffman::new(&lengths).unwrap_or(Huffman {
        count: [0; MAXBITS + 1],
        symbol: Vec::new(),
    });
    let dist = Huffman::new(&[5u8; 30]).unwrap_or(Huffman {
        count: [0; MAXBITS + 1],
        symbol: Vec::new(),
    });
    (lit, dist)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 정적 허프만(작은 반복 문자열) · Adler 검증.
    #[test]
    fn fixed_huffman_small() {
        let z = [
            0x78, 0xda, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x22, 0xcb, 0xf3, 0x8b,
            0x72, 0x52, 0x00, 0x68, 0x7d, 0x08, 0xc5,
        ];
        assert_eq!(inflate_zlib(&z).expect("ok"), b"hello hello hello world");
    }

    /// 저장 블록(압축 없음).
    #[test]
    fn stored_block() {
        // BFINAL=1 BTYPE=00 → 0x01, LEN=5, NLEN=!5, "abcde"
        let raw = [0x01, 0x05, 0x00, 0xfa, 0xff, b'a', b'b', b'c', b'd', b'e'];
        let (out, used) = inflate_raw(&raw).expect("ok");
        assert_eq!(out, b"abcde");
        assert_eq!(used, raw.len());
    }

    /// 동적 허프만(긴 반복 + 256바이트 전 값 · Python zlib level 9 산출) — 원문 1,052바이트 복원.
    #[test]
    fn dynamic_huffman_long() {
        let z: Vec<u8> = include_str!("../tests/z2.hex")
            .split(',')
            .map(|t| u8::from_str_radix(t.trim().trim_start_matches("0x"), 16).expect("hex"))
            .collect();
        let out = inflate_zlib(&z).expect("ok");
        let mut want = Vec::new();
        for _ in 0..12 {
            want.extend_from_slice(b"The quick brown fox jumps over the lazy dog. ");
        }
        for _ in 0..2 {
            want.extend((0..=255u8).collect::<Vec<_>>());
        }
        assert_eq!(out.len(), 1052);
        assert_eq!(out, want);
    }

    #[test]
    fn bad_input_is_error_not_panic() {
        assert!(inflate_zlib(&[0x78, 0x9c, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(inflate_zlib(&[0, 0, 0]).is_err());
        assert!(inflate_raw(&[0x07]).is_err());
    }
}
