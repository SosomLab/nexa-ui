//! ★ 기저(baseline · SOF0/SOF1 · 8비트 · 허프만) JPEG 디코더 — 외부 crate 0(DR-3). LOB 이미지 **미리보기**용(docs/87 §5-1 · T-237).
//! 지원 = 회색 1성분 · YCbCr 3성분(서브샘플링 h/v 1~2 · 최근접 업샘플) · 재시작 간격(DRI/RSTn) · APPn/COM 건너뜀.
//! 미지원 = 프로그레시브(SOF2) · 산술 부호(SOF9~) · 12비트 · CMYK/YCCK 4성분 · 계층/무손실 — 오류 문구로 안내.
//! 정확도 = 부동소수 분리 IDCT(미리보기 품질 · 표준 참조 IDCT와 ±1 이내).

use crate::surface::IconImage;

/// SOF에서 (가로, 세로)만 읽는다(전체 풀이 없이).
pub fn dimensions(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            if i + 9 > b.len() {
                return None;
            }
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]);
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]);
            return Some((u32::from(w), u32::from(h)));
        }
        i += 2 + len;
    }
    None
}

#[derive(Clone, Default)]
struct Huff {
    /// 길이별 최대 코드(없으면 -1) · 첫 코드 · 값 표 시작.
    maxcode: [i32; 17],
    mincode: [i32; 17],
    valptr: [usize; 17],
    vals: Vec<u8>,
}

impl Huff {
    fn build(counts: &[u8; 16], vals: Vec<u8>) -> Huff {
        let mut h = Huff {
            vals,
            ..Huff::default()
        };
        let mut code = 0i32;
        let mut k = 0usize;
        for l in 1..=16 {
            let n = counts[l - 1] as usize;
            if n == 0 {
                h.maxcode[l] = -1;
            } else {
                h.valptr[l] = k;
                h.mincode[l] = code;
                code += n as i32;
                k += n;
                h.maxcode[l] = code - 1;
            }
            code <<= 1;
        }
        h
    }
}

#[derive(Clone, Copy, Default)]
struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    /// SOS에서.
    td: usize,
    ta: usize,
}

/// 엔트로피 구간 비트 읽기(0xFF00 채움 제거 · 마커를 만나면 0비트를 공급하고 멈춘다).
struct Bits<'a> {
    b: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
    /// 마커에 닿았다(RSTn 등) — 그 위치.
    marker: Option<usize>,
}

impl<'a> Bits<'a> {
    fn new(b: &'a [u8], pos: usize) -> Self {
        Bits {
            b,
            pos,
            buf: 0,
            cnt: 0,
            marker: None,
        }
    }
    fn fill(&mut self) {
        while self.cnt <= 24 {
            let byte = if self.marker.is_some() || self.pos >= self.b.len() {
                0
            } else {
                let c = self.b[self.pos];
                if c == 0xFF {
                    let next = self.b.get(self.pos + 1).copied().unwrap_or(0xD9);
                    if next == 0x00 {
                        self.pos += 2;
                        c
                    } else if next == 0xFF {
                        // 채움 0xFF — 건너뛴다.
                        self.pos += 1;
                        continue;
                    } else {
                        self.marker = Some(self.pos);
                        0
                    }
                } else {
                    self.pos += 1;
                    c
                }
            };
            self.buf |= u32::from(byte) << (24 - self.cnt);
            self.cnt += 8;
        }
    }
    fn bit(&mut self) -> u32 {
        if self.cnt == 0 {
            self.fill();
        }
        let v = self.buf >> 31;
        self.buf <<= 1;
        self.cnt -= 1;
        v
    }
    fn bits(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        if self.cnt < n {
            self.fill();
        }
        let v = self.buf >> (32 - n);
        self.buf <<= n;
        self.cnt -= n;
        v
    }
    /// 재시작: 바이트 정렬 · RSTn 마커를 지나간다.
    fn restart(&mut self) -> Result<(), String> {
        self.buf = 0;
        self.cnt = 0;
        let at = match self.marker.take() {
            Some(p) => p,
            None => {
                // 마커까지 바이트를 찾는다(패딩 뒤).
                let mut p = self.pos;
                while p + 1 < self.b.len() && !(self.b[p] == 0xFF && self.b[p + 1] != 0x00) {
                    p += 1;
                }
                p
            }
        };
        let m = self.b.get(at + 1).copied().unwrap_or(0);
        if !(0xD0..=0xD7).contains(&m) {
            return Err(format!("jpeg: expected RST marker, found {m:02X}"));
        }
        self.pos = at + 2;
        Ok(())
    }
    fn decode(&mut self, h: &Huff) -> Result<u8, String> {
        let mut code = 0i32;
        for l in 1..=16 {
            code = (code << 1) | self.bit() as i32;
            if h.maxcode[l] >= 0 && code <= h.maxcode[l] {
                let idx = h.valptr[l] + (code - h.mincode[l]) as usize;
                return h
                    .vals
                    .get(idx)
                    .copied()
                    .ok_or_else(|| "jpeg: huffman table".to_string());
            }
        }
        Err("jpeg: bad huffman code".into())
    }
    /// 크기 s의 값 받기 + 부호 확장(F.2.2.1 EXTEND).
    fn receive_extend(&mut self, s: u32) -> i32 {
        if s == 0 {
            return 0;
        }
        let v = self.bits(s) as i32;
        if v < (1 << (s - 1)) {
            v - (1 << s) + 1
        } else {
            v
        }
    }
}

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// 분리 IDCT의 코사인 표 c[u][x] = C(u)/2 · cos((2x+1)uπ/16).
fn idct_table() -> [[f32; 8]; 8] {
    let mut t = [[0f32; 8]; 8];
    for (u, row) in t.iter_mut().enumerate() {
        let cu = if u == 0 {
            std::f32::consts::FRAC_1_SQRT_2
        } else {
            1.0
        };
        for (x, v) in row.iter_mut().enumerate() {
            *v = cu / 2.0 * (((2 * x + 1) as f32) * (u as f32) * std::f32::consts::PI / 16.0).cos();
        }
    }
    t
}

/// 8×8 블록 IDCT(입력 = 역양자화된 자연 순서 계수) → 0..255.
fn idct8x8(coef: &[f32; 64], t: &[[f32; 8]; 8], out: &mut [u8; 64]) {
    let mut tmp = [0f32; 64];
    // 행 방향(u → x).
    for y in 0..8 {
        for x in 0..8 {
            let mut s = 0f32;
            for u in 0..8 {
                s += t[u][x] * coef[y * 8 + u];
            }
            tmp[y * 8 + x] = s;
        }
    }
    // 열 방향(v → y).
    for x in 0..8 {
        for y in 0..8 {
            let mut s = 0f32;
            for v in 0..8 {
                s += t[v][y] * tmp[v * 8 + x];
            }
            out[y * 8 + x] = (s + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// 기저 JPEG → RGBA. `max_pixels`는 호출자가 SOF 뒤 검사한다(`check` 콜백).
pub fn decode(
    b: &[u8],
    check: &dyn Fn(u32, u32) -> Result<(), String>,
) -> Result<IconImage, String> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return Err("jpeg: not a JPEG (no SOI)".into());
    }
    let mut qt: [[u16; 64]; 4] = [[0; 64]; 4];
    let mut dc: [Huff; 4] = Default::default();
    let mut ac: [Huff; 4] = Default::default();
    let mut comps: Vec<Comp> = Vec::new();
    let (mut w, mut h) = (0u32, 0u32);
    let mut restart_interval = 0usize;
    let mut i = 2usize;
    let mut sos: Option<usize> = None;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xD8 || (0xD0..=0xD7).contains(&m) || m == 0x01 {
            i += 2;
            continue;
        }
        if m == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let seg = b
            .get(i + 4..i + 2 + len)
            .ok_or_else(|| "jpeg: truncated segment".to_string())?;
        match m {
            0xC0 | 0xC1 => {
                if seg.len() < 6 || seg[0] != 8 {
                    return Err("jpeg: only 8-bit precision is supported".into());
                }
                h = u32::from(u16::from_be_bytes([seg[1], seg[2]]));
                w = u32::from(u16::from_be_bytes([seg[3], seg[4]]));
                let n = seg[5] as usize;
                if seg.len() < 6 + 3 * n {
                    return Err("jpeg: SOF".into());
                }
                comps = (0..n)
                    .map(|k| Comp {
                        id: seg[6 + 3 * k],
                        h: (seg[7 + 3 * k] >> 4) as usize,
                        v: (seg[7 + 3 * k] & 15) as usize,
                        tq: (seg[8 + 3 * k] & 3) as usize,
                        td: 0,
                        ta: 0,
                    })
                    .collect();
                if w == 0 || h == 0 {
                    return Err("jpeg: empty image".into());
                }
                check(w, h)?;
            }
            0xC2 => {
                return Err("jpeg: progressive JPEG is not supported (save to file to view)".into())
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err("jpeg: lossless/hierarchical/arithmetic JPEG is not supported".into())
            }
            0xC4 => {
                let mut p = 0;
                while p + 17 <= seg.len() {
                    let tc = seg[p] >> 4;
                    let th = (seg[p] & 15) as usize;
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&seg[p + 1..p + 17]);
                    let total: usize = counts.iter().map(|&c| c as usize).sum();
                    let vals = seg
                        .get(p + 17..p + 17 + total)
                        .ok_or_else(|| "jpeg: DHT".to_string())?
                        .to_vec();
                    if th > 3 {
                        return Err("jpeg: DHT id".into());
                    }
                    if tc == 0 {
                        dc[th] = Huff::build(&counts, vals);
                    } else {
                        ac[th] = Huff::build(&counts, vals);
                    }
                    p += 17 + total;
                }
            }
            0xDB => {
                let mut p = 0;
                while p < seg.len() {
                    let pq = seg[p] >> 4;
                    let tq = (seg[p] & 3) as usize;
                    p += 1;
                    for &z in ZIGZAG.iter() {
                        let v = if pq == 0 {
                            let v = *seg.get(p).ok_or("jpeg: DQT")?;
                            p += 1;
                            u16::from(v)
                        } else {
                            let v = u16::from_be_bytes([
                                *seg.get(p).ok_or("jpeg: DQT")?,
                                *seg.get(p + 1).ok_or("jpeg: DQT")?,
                            ]);
                            p += 2;
                            v
                        };
                        qt[tq][z] = v;
                    }
                }
            }
            0xDD => {
                if seg.len() >= 2 {
                    restart_interval = u16::from_be_bytes([seg[0], seg[1]]) as usize;
                }
            }
            0xDA => {
                let n = seg[0] as usize;
                if n != comps.len() || comps.is_empty() {
                    return Err("jpeg: only a single interleaved scan is supported".into());
                }
                for k in 0..n {
                    let id = seg[1 + 2 * k];
                    let t = seg[2 + 2 * k];
                    let c = comps
                        .iter_mut()
                        .find(|c| c.id == id)
                        .ok_or_else(|| "jpeg: SOS component".to_string())?;
                    c.td = (t >> 4) as usize & 3;
                    c.ta = (t & 15) as usize & 3;
                }
                sos = Some(i + 2 + len);
                break;
            }
            _ => {}
        }
        i += 2 + len;
    }
    let start = sos.ok_or_else(|| "jpeg: no scan (SOS)".to_string())?;
    if comps.len() != 1 && comps.len() != 3 {
        return Err(format!(
            "jpeg: {} components (CMYK/YCCK) are not supported",
            comps.len()
        ));
    }
    let hmax = comps.iter().map(|c| c.h).max().unwrap_or(1).clamp(1, 4);
    let vmax = comps.iter().map(|c| c.v).max().unwrap_or(1).clamp(1, 4);
    if comps
        .iter()
        .any(|c| c.h == 0 || c.v == 0 || c.h > 4 || c.v > 4)
    {
        return Err("jpeg: sampling factors".into());
    }
    let mcu_w = 8 * hmax;
    let mcu_h = 8 * vmax;
    let mcus_x = (w as usize).div_ceil(mcu_w);
    let mcus_y = (h as usize).div_ceil(mcu_h);
    // 성분 평면(MCU 격자 크기).
    let mut planes: Vec<Vec<u8>> = comps
        .iter()
        .map(|c| vec![0u8; mcus_x * c.h * 8 * mcus_y * c.v * 8])
        .collect();
    let plane_w: Vec<usize> = comps.iter().map(|c| mcus_x * c.h * 8).collect();
    let t = idct_table();
    let mut br = Bits::new(b, start);
    let mut preds = vec![0i32; comps.len()];
    let mut coef = [0f32; 64];
    let mut block = [0u8; 64];
    let total_mcus = mcus_x * mcus_y;
    for mcu in 0..total_mcus {
        if restart_interval > 0 && mcu > 0 && mcu % restart_interval == 0 {
            br.restart()?;
            preds.iter_mut().for_each(|p| *p = 0);
        }
        let (mx, my) = (mcu % mcus_x, mcu / mcus_x);
        for (ci, c) in comps.iter().enumerate() {
            let q = &qt[c.tq];
            for by in 0..c.v {
                for bx in 0..c.h {
                    coef.iter_mut().for_each(|v| *v = 0.0);
                    // DC.
                    let s = br.decode(&dc[c.td])? as u32;
                    let diff = br.receive_extend(s);
                    preds[ci] += diff;
                    coef[0] = (preds[ci] * i32::from(q[0])) as f32;
                    // AC.
                    let mut k = 1usize;
                    while k < 64 {
                        let rs = br.decode(&ac[c.ta])?;
                        let r = (rs >> 4) as usize;
                        let s = (rs & 15) as u32;
                        if s == 0 {
                            if r == 15 {
                                k += 16;
                                continue;
                            }
                            break; // EOB
                        }
                        k += r;
                        if k > 63 {
                            return Err("jpeg: AC run past block".into());
                        }
                        let z = ZIGZAG[k];
                        coef[z] = (br.receive_extend(s) * i32::from(q[z])) as f32;
                        k += 1;
                    }
                    idct8x8(&coef, &t, &mut block);
                    // 평면에 쓰기.
                    let px0 = (mx * c.h + bx) * 8;
                    let py0 = (my * c.v + by) * 8;
                    let pw = plane_w[ci];
                    let plane = &mut planes[ci];
                    for yy in 0..8 {
                        let row = (py0 + yy) * pw + px0;
                        plane[row..row + 8].copy_from_slice(&block[yy * 8..yy * 8 + 8]);
                    }
                }
            }
        }
    }
    // 성분 평면 → RGBA(최근접 업샘플 · YCbCr → RGB).
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    let samp = |ci: usize, x: usize, y: usize| -> f32 {
        let c = &comps[ci];
        let sx = x * c.h / hmax;
        let sy = y * c.v / vmax;
        f32::from(planes[ci][sy * plane_w[ci] + sx])
    };
    for y in 0..h as usize {
        for x in 0..w as usize {
            if comps.len() == 1 {
                let g = samp(0, x, y) as u8;
                rgba.extend_from_slice(&[g, g, g, 255]);
            } else {
                let yy = samp(0, x, y);
                let cb = samp(1, x, y) - 128.0;
                let cr = samp(2, x, y) - 128.0;
                let r = yy + 1.402 * cr;
                let g = yy - 0.344_136 * cb - 0.714_136 * cr;
                let bl = yy + 1.772 * cb;
                rgba.extend_from_slice(&[
                    r.round().clamp(0.0, 255.0) as u8,
                    g.round().clamp(0.0, 255.0) as u8,
                    bl.round().clamp(0.0, 255.0) as u8,
                    255,
                ]);
            }
        }
    }
    Ok(IconImage { w, h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s = s.trim();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    fn px(img: &IconImage, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * img.w + x) * 4) as usize;
        [img.rgba[o], img.rgba[o + 1], img.rgba[o + 2]]
    }

    fn near(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
        (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= tol)
    }

    #[test]
    fn baseline_420_color_quadrants_and_dimensions() {
        // sips로 만든 16×16 4분면(빨강·초록·파랑·흰색) · 4:2:0 · DRI 있음.
        let b = hex(include_str!("../tests/quad.hex"));
        assert_eq!(dimensions(&b), Some((16, 16)));
        let img = decode(&b, &|_, _| Ok(())).expect("decode");
        assert_eq!((img.w, img.h), (16, 16));
        // 4:2:0 크로마 경계에서 번지므로 각 분면의 안쪽 픽셀로 본다.
        assert!(
            near(px(&img, 2, 2), [255, 0, 0], 40),
            "{:?}",
            px(&img, 2, 2)
        );
        assert!(
            near(px(&img, 13, 2), [0, 255, 0], 40),
            "{:?}",
            px(&img, 13, 2)
        );
        assert!(
            near(px(&img, 2, 13), [0, 0, 255], 40),
            "{:?}",
            px(&img, 2, 13)
        );
        assert!(
            near(px(&img, 13, 13), [255, 255, 255], 20),
            "{:?}",
            px(&img, 13, 13)
        );
    }

    #[test]
    fn baseline_gray_gradient() {
        // 회색 16×16 · 값 = x*16 + y(품질 95).
        let b = hex(include_str!("../tests/gray.hex"));
        let img = decode(&b, &|_, _| Ok(())).expect("decode");
        for (x, y) in [(0u32, 0u32), (8, 8), (15, 15), (3, 12), (12, 3)] {
            let want = (x * 16 + y).min(255) as u8;
            let got = px(&img, x, y);
            assert_eq!(got[0], got[1]);
            assert!(
                (i32::from(got[0]) - i32::from(want)).abs() <= 6,
                "({x},{y}) got {} want {want}",
                got[0]
            );
        }
    }

    #[test]
    fn size_check_and_unsupported_kinds() {
        let b = hex(include_str!("../tests/quad.hex"));
        let e = decode(&b, &|w, h| Err(format!("too big {w}x{h}"))).expect_err("check");
        assert!(e.contains("too big 16x16"));
        // SOF2로 바꿔 프로그레시브 판정.
        let mut p = b;
        let sof = p.windows(2).position(|w| w == [0xFF, 0xC0]).expect("SOF0");
        p[sof + 1] = 0xC2;
        let e = decode(&p, &|_, _| Ok(())).expect_err("progressive");
        assert!(e.contains("progressive"));
        assert!(decode(b"hello", &|_, _| Ok(())).is_err());
    }
}
