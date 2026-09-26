//! 이미지 디코더(PNG · BMP · GIF 첫 프레임) → [`IconImage`](crate::surface::IconImage)(RGBA straight) — 외부 crate 0(DR-3).
//! 용도 = 결과 그리드 BLOB 값 미리보기(nexa-sql docs/87 §5) · 작은 자산. JPEG/WebP는 **판별만**(`sniff`) — 미리보기는 후속
//! (기저 JPEG 디코더는 크기 대비 가치가 낮아 보류 · 사용자는 "파일로 저장"으로 본다).
//!
//! 규칙: 손상 입력 = `Err(String)`(패닉 없음) · 픽셀 상한 `max_pixels`(호출자 설정 · 메모리 보호) · 16비트는 상위 바이트 ·
//! PNG 인터레이스(Adam7) = 7패스 풀이(T-237) · JPEG 기저 = `jpeg` 모듈(프로그레시브는 안내).

use crate::inflate::inflate_zlib;
use crate::surface::IconImage;

/// 시그니처로 판별한 형식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Webp,
    Unknown,
}

impl ImageKind {
    pub fn name(self) -> &'static str {
        match self {
            ImageKind::Png => "PNG",
            ImageKind::Jpeg => "JPEG",
            ImageKind::Gif => "GIF",
            ImageKind::Bmp => "BMP",
            ImageKind::Webp => "WebP",
            ImageKind::Unknown => "",
        }
    }

    /// 이 크레이트가 픽셀로 풀 수 있는가.
    pub fn decodable(self) -> bool {
        matches!(
            self,
            ImageKind::Png | ImageKind::Gif | ImageKind::Bmp | ImageKind::Jpeg
        )
    }

    /// 파일 확장자(저장 기본 이름용).
    pub fn ext(self) -> &'static str {
        match self {
            ImageKind::Png => "png",
            ImageKind::Jpeg => "jpg",
            ImageKind::Gif => "gif",
            ImageKind::Bmp => "bmp",
            ImageKind::Webp => "webp",
            ImageKind::Unknown => "bin",
        }
    }
}

/// 앞 바이트로 형식 판별.
pub fn sniff(b: &[u8]) -> ImageKind {
    if b.len() >= 8 && b[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        ImageKind::Png
    } else if b.len() >= 3 && b[..3] == [0xff, 0xd8, 0xff] {
        ImageKind::Jpeg
    } else if b.len() >= 6 && (&b[..6] == b"GIF87a" || &b[..6] == b"GIF89a") {
        ImageKind::Gif
    } else if b.len() >= 2 && &b[..2] == b"BM" {
        ImageKind::Bmp
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        ImageKind::Webp
    } else {
        ImageKind::Unknown
    }
}

/// 형식을 판별해 푼다. `max_pixels` = 폭×높이 상한(넘으면 오류 · 메모리 보호).
pub fn decode(b: &[u8], max_pixels: usize) -> Result<IconImage, String> {
    match sniff(b) {
        ImageKind::Png => decode_png(b, max_pixels),
        ImageKind::Bmp => decode_bmp(b, max_pixels),
        ImageKind::Gif => decode_gif(b, max_pixels),
        ImageKind::Jpeg => crate::jpeg::decode(b, &|w, h| check_size(w, h, max_pixels)),
        ImageKind::Webp => Err("WebP preview is not supported (save to file to view)".into()),
        ImageKind::Unknown => Err("not an image".into()),
    }
}

/// (폭, 높이)만 빨리 — 판별 가능한 형식만.
pub fn dimensions(b: &[u8]) -> Option<(u32, u32)> {
    match sniff(b) {
        ImageKind::Png if b.len() >= 24 => Some((be32(&b[16..20]), be32(&b[20..24]))),
        ImageKind::Gif if b.len() >= 10 => Some((
            u32::from(u16::from_le_bytes([b[6], b[7]])),
            u32::from(u16::from_le_bytes([b[8], b[9]])),
        )),
        ImageKind::Bmp if b.len() >= 26 => {
            let w = i32::from_le_bytes([b[18], b[19], b[20], b[21]]);
            let h = i32::from_le_bytes([b[22], b[23], b[24], b[25]]);
            Some((w.unsigned_abs(), h.unsigned_abs()))
        }
        ImageKind::Jpeg => crate::jpeg::dimensions(b),
        _ => None,
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn check_size(w: u32, h: u32, max_pixels: usize) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    let n = (w as usize)
        .checked_mul(h as usize)
        .ok_or("image too large")?;
    if n > max_pixels {
        return Err(format!("image too large: {w}×{h} (limit {max_pixels} px)"));
    }
    Ok(())
}

// ───────────────────────────── PNG

fn decode_png(b: &[u8], max_pixels: usize) -> Result<IconImage, String> {
    let mut pos = 8;
    let mut w = 0u32;
    let mut h = 0u32;
    let mut depth = 0u8;
    let mut ctype = 0u8;
    let mut interlace = 0u8;
    let mut plte: Vec<u8> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_ihdr = false;
    while pos + 8 <= b.len() {
        let len = be32(&b[pos..pos + 4]) as usize;
        let ty = &b[pos + 4..pos + 8];
        let start = pos + 8;
        let end = start.checked_add(len).ok_or("png: chunk length")?;
        if end + 4 > b.len() {
            return Err("png: truncated chunk".into());
        }
        let data = &b[start..end];
        match ty {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err("png: IHDR".into());
                }
                w = be32(&data[0..4]);
                h = be32(&data[4..8]);
                depth = data[8];
                ctype = data[9];
                interlace = data[12];
                seen_ihdr = true;
                check_size(w, h, max_pixels)?;
            }
            b"PLTE" => plte = data.to_vec(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos = end + 4; // CRC는 검증하지 않는다(미리보기).
    }
    if !seen_ihdr {
        return Err("png: no IHDR".into());
    }
    let channels: usize = match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("png: color type".into()),
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) || (ctype != 0 && ctype != 3 && depth < 8) {
        return Err("png: bit depth".into());
    }
    let raw = inflate_zlib(&idat).map_err(|e| format!("png: {e}"))?;
    let bits_pp = channels * depth as usize;
    let bpp = bits_pp.div_ceil(8).max(1);
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    // 패스 목록: 비인터레이스 = 전체 한 패스 · Adam7 = 7패스(시작 x·y · 간격 x·y) — 빈 패스는 건너뛴다.
    let passes: Vec<(usize, usize, usize, usize)> = if interlace == 0 {
        vec![(0, 0, 1, 1)]
    } else {
        vec![
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    };
    let mut off = 0usize;
    for (sx, sy, dx, dy) in passes {
        let pw = (w as usize).saturating_sub(sx).div_ceil(dx);
        let ph = (h as usize).saturating_sub(sy).div_ceil(dy);
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = (pw * bits_pp).div_ceil(8);
        if raw.len() < off + (stride + 1) * ph {
            return Err("png: image data too short".into());
        }
        // 필터 되돌리기(줄마다 · 패스 안에서 이전 줄 기준).
        let mut cur = vec![0u8; stride];
        let mut prev = vec![0u8; stride];
        for j in 0..ph {
            let base = off + j * (stride + 1);
            let filter = raw[base];
            cur.copy_from_slice(&raw[base + 1..base + 1 + stride]);
            for i in 0..stride {
                let a = if i >= bpp { cur[i - bpp] } else { 0 };
                let bb = prev[i];
                let c = if i >= bpp { prev[i - bpp] } else { 0 };
                let pred = match filter {
                    0 => 0,
                    1 => a,
                    2 => bb,
                    3 => ((u16::from(a) + u16::from(bb)) / 2) as u8,
                    4 => paeth(a, bb, c),
                    _ => return Err("png: filter type".into()),
                };
                cur[i] = cur[i].wrapping_add(pred);
            }
            // 픽셀 → RGBA(패스 좌표 → 전체 좌표).
            for x in 0..pw {
                let px = match (ctype, depth) {
                    (0, 8) => gray(cur[x], &trns, 0, u16::from(cur[x])),
                    (0, 16) => gray(
                        cur[x * 2],
                        &trns,
                        0,
                        u16::from_be_bytes([cur[x * 2], cur[x * 2 + 1]]),
                    ),
                    (0, d) => {
                        let v = sample(&cur, x, d);
                        let max = (1u16 << d) - 1;
                        let g = (u16::from(v) * 255 / max) as u8;
                        gray(g, &trns, 0, u16::from(v))
                    }
                    (2, 8) => {
                        let (r, g, bl) = (cur[x * 3], cur[x * 3 + 1], cur[x * 3 + 2]);
                        let a = if trns.len() >= 6
                            && trns[1] == r
                            && trns[3] == g
                            && trns[5] == bl
                            && trns[0] == 0
                            && trns[2] == 0
                            && trns[4] == 0
                        {
                            0
                        } else {
                            255
                        };
                        [r, g, bl, a]
                    }
                    (2, 16) => [cur[x * 6], cur[x * 6 + 2], cur[x * 6 + 4], 255],
                    (3, d) => {
                        let idx = if d == 8 { cur[x] } else { sample(&cur, x, d) } as usize;
                        let (r, g, bl) = (
                            *plte.get(idx * 3).unwrap_or(&0),
                            *plte.get(idx * 3 + 1).unwrap_or(&0),
                            *plte.get(idx * 3 + 2).unwrap_or(&0),
                        );
                        [r, g, bl, *trns.get(idx).unwrap_or(&255)]
                    }
                    (4, 8) => [cur[x * 2], cur[x * 2], cur[x * 2], cur[x * 2 + 1]],
                    (4, 16) => [cur[x * 4], cur[x * 4], cur[x * 4], cur[x * 4 + 2]],
                    (6, 8) => [cur[x * 4], cur[x * 4 + 1], cur[x * 4 + 2], cur[x * 4 + 3]],
                    (6, 16) => [cur[x * 8], cur[x * 8 + 2], cur[x * 8 + 4], cur[x * 8 + 6]],
                    _ => return Err("png: unsupported layout".into()),
                };
                let (gx, gy) = (sx + x * dx, sy + j * dy);
                let o = (gy * w as usize + gx) * 4;
                rgba[o..o + 4].copy_from_slice(&px);
            }
            std::mem::swap(&mut cur, &mut prev);
        }
        off += (stride + 1) * ph;
    }
    Ok(IconImage { w, h, rgba })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// 1/2/4비트 샘플(MSB 먼저).
fn sample(row: &[u8], x: usize, depth: u8) -> u8 {
    let d = depth as usize;
    let bit = x * d;
    let byte = row[bit / 8];
    let shift = 8 - d - (bit % 8);
    (byte >> shift) & ((1u8 << d) - 1)
}

fn gray(g: u8, trns: &[u8], _: u8, raw: u16) -> [u8; 4] {
    let a = if trns.len() >= 2 && u16::from_be_bytes([trns[0], trns[1]]) == raw {
        0
    } else {
        255
    };
    [g, g, g, a]
}

// ───────────────────────────── BMP

fn decode_bmp(b: &[u8], max_pixels: usize) -> Result<IconImage, String> {
    if b.len() < 54 {
        return Err("bmp: too short".into());
    }
    let off = u32::from_le_bytes([b[10], b[11], b[12], b[13]]) as usize;
    let hsize = u32::from_le_bytes([b[14], b[15], b[16], b[17]]) as usize;
    if hsize < 40 {
        return Err("bmp: only BITMAPINFOHEADER and later are supported".into());
    }
    let w = i32::from_le_bytes([b[18], b[19], b[20], b[21]]);
    let h = i32::from_le_bytes([b[22], b[23], b[24], b[25]]);
    let bpp = u16::from_le_bytes([b[28], b[29]]);
    let comp = u32::from_le_bytes([b[30], b[31], b[32], b[33]]);
    let top_down = h < 0;
    let (wu, hu) = (w.unsigned_abs(), h.unsigned_abs());
    check_size(wu, hu, max_pixels)?;
    if comp != 0 && !(comp == 3 && (bpp == 32 || bpp == 16)) {
        return Err("bmp: compressed BMP is not supported".into());
    }
    // 팔레트(≤ 8비트).
    let colors_used = u32::from_le_bytes([b[46], b[47], b[48], b[49]]) as usize;
    let pal_start = 14 + hsize;
    let pal_n = if bpp <= 8 {
        if colors_used == 0 {
            1usize << bpp
        } else {
            colors_used
        }
    } else {
        0
    };
    let stride = (wu as usize * bpp as usize).div_ceil(32) * 4;
    if off + stride * hu as usize > b.len() {
        return Err("bmp: pixel data too short".into());
    }
    let mut rgba = Vec::with_capacity(wu as usize * hu as usize * 4);
    for row in 0..hu as usize {
        let src_row = if top_down { row } else { hu as usize - 1 - row };
        let line = &b[off + src_row * stride..off + (src_row + 1) * stride];
        for x in 0..wu as usize {
            let px = match bpp {
                32 => [line[x * 4 + 2], line[x * 4 + 1], line[x * 4], 255],
                24 => [line[x * 3 + 2], line[x * 3 + 1], line[x * 3], 255],
                16 => {
                    let v = u16::from_le_bytes([line[x * 2], line[x * 2 + 1]]);
                    let r = ((v >> 10) & 31) as u8 * 255 / 31;
                    let g = ((v >> 5) & 31) as u8 * 255 / 31;
                    let bl = (v & 31) as u8 * 255 / 31;
                    [r, g, bl, 255]
                }
                8 | 4 | 1 => {
                    let idx = match bpp {
                        8 => line[x],
                        4 => (line[x / 2] >> (if x % 2 == 0 { 4 } else { 0 })) & 0x0f,
                        _ => (line[x / 8] >> (7 - x % 8)) & 1,
                    } as usize;
                    if idx >= pal_n {
                        return Err("bmp: palette index".into());
                    }
                    let p = pal_start + idx * 4;
                    if p + 3 > b.len() {
                        return Err("bmp: palette".into());
                    }
                    [b[p + 2], b[p + 1], b[p], 255]
                }
                _ => return Err("bmp: bit depth".into()),
            };
            rgba.extend_from_slice(&px);
        }
    }
    Ok(IconImage { w: wu, h: hu, rgba })
}

// ───────────────────────────── GIF(첫 프레임)

fn decode_gif(b: &[u8], max_pixels: usize) -> Result<IconImage, String> {
    if b.len() < 13 {
        return Err("gif: too short".into());
    }
    let sw = u16::from_le_bytes([b[6], b[7]]) as usize;
    let sh = u16::from_le_bytes([b[8], b[9]]) as usize;
    let flags = b[10];
    let bg = b[11] as usize;
    let mut pos = 13;
    let mut global: Vec<u8> = Vec::new();
    if flags & 0x80 != 0 {
        let n = 3 * (1usize << ((flags & 7) + 1));
        if pos + n > b.len() {
            return Err("gif: global color table".into());
        }
        global = b[pos..pos + n].to_vec();
        pos += n;
    }
    check_size(sw as u32, sh as u32, max_pixels)?;
    let mut transparent: Option<usize> = None;
    loop {
        let Some(&tag) = b.get(pos) else {
            return Err("gif: no image".into());
        };
        pos += 1;
        match tag {
            0x21 => {
                // 확장: 그래픽 제어(투명 인덱스)만 읽고 나머지는 건너뜀.
                let label = *b.get(pos).ok_or("gif: ext")?;
                pos += 1;
                if label == 0xf9 && b.len() > pos + 5 && b[pos] == 4 && b[pos + 1] & 1 != 0 {
                    transparent = Some(b[pos + 4] as usize);
                }
                loop {
                    let n = *b.get(pos).ok_or("gif: ext block")? as usize;
                    pos += 1;
                    if n == 0 {
                        break;
                    }
                    pos += n;
                }
            }
            0x2c => {
                if pos + 9 > b.len() {
                    return Err("gif: image descriptor".into());
                }
                let ix = u16::from_le_bytes([b[pos], b[pos + 1]]) as usize;
                let iy = u16::from_le_bytes([b[pos + 2], b[pos + 3]]) as usize;
                let iw = u16::from_le_bytes([b[pos + 4], b[pos + 5]]) as usize;
                let ih = u16::from_le_bytes([b[pos + 6], b[pos + 7]]) as usize;
                let iflags = b[pos + 8];
                pos += 9;
                let mut table = global.clone();
                if iflags & 0x80 != 0 {
                    let n = 3 * (1usize << ((iflags & 7) + 1));
                    if pos + n > b.len() {
                        return Err("gif: local color table".into());
                    }
                    table = b[pos..pos + n].to_vec();
                    pos += n;
                }
                let interlaced = iflags & 0x40 != 0;
                let min_code = *b.get(pos).ok_or("gif: lzw min code")?;
                pos += 1;
                let mut data = Vec::new();
                loop {
                    let n = *b.get(pos).ok_or("gif: data block")? as usize;
                    pos += 1;
                    if n == 0 {
                        break;
                    }
                    if pos + n > b.len() {
                        return Err("gif: data block short".into());
                    }
                    data.extend_from_slice(&b[pos..pos + n]);
                    pos += n;
                }
                let idx = lzw_decode(&data, min_code, iw * ih)?;
                // 화면 크기 캔버스에 배경(투명) 깔고 프레임 배치.
                let mut rgba = vec![0u8; sw * sh * 4];
                if let Some(bgc) = global.get(bg * 3..bg * 3 + 3) {
                    if transparent != Some(bg) {
                        for p in rgba.chunks_mut(4) {
                            p[0] = bgc[0];
                            p[1] = bgc[1];
                            p[2] = bgc[2];
                            p[3] = 255;
                        }
                    }
                }
                let rows: Vec<usize> = if interlaced {
                    let mut v = Vec::with_capacity(ih);
                    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                        v.extend((start..ih).step_by(step));
                    }
                    v
                } else {
                    (0..ih).collect()
                };
                for (src_row, &dst_row) in rows.iter().enumerate() {
                    for x in 0..iw {
                        let i = idx[src_row * iw + x] as usize;
                        let (dx, dy) = (ix + x, iy + dst_row);
                        if dx >= sw || dy >= sh || Some(i) == transparent {
                            continue;
                        }
                        let o = (dy * sw + dx) * 4;
                        let c = table.get(i * 3..i * 3 + 3).ok_or("gif: color index")?;
                        rgba[o] = c[0];
                        rgba[o + 1] = c[1];
                        rgba[o + 2] = c[2];
                        rgba[o + 3] = 255;
                    }
                }
                return Ok(IconImage {
                    w: sw as u32,
                    h: sh as u32,
                    rgba,
                });
            }
            0x3b => return Err("gif: no image".into()),
            _ => return Err("gif: unknown block".into()),
        }
    }
}

/// GIF LZW(가변 코드 길이 · 하위 비트 먼저) → 인덱스 `count`개.
fn lzw_decode(data: &[u8], min_code: u8, count: usize) -> Result<Vec<u8>, String> {
    if !(1..=11).contains(&min_code) {
        return Err("gif: lzw min code size".into());
    }
    let clear = 1usize << min_code;
    let eoi = clear + 1;
    let mut code_size = min_code as usize + 1;
    let mut dict: Vec<(Option<usize>, u8)> = (0..clear).map(|i| (None, i as u8)).collect();
    dict.push((None, 0));
    dict.push((None, 0));
    let mut out: Vec<u8> = Vec::with_capacity(count);
    let mut prev: Option<usize> = None;
    let mut bitpos = 0usize;
    let total_bits = data.len() * 8;
    let mut stack: Vec<u8> = Vec::new();
    while out.len() < count {
        if bitpos + code_size > total_bits {
            break;
        }
        let mut code = 0usize;
        for i in 0..code_size {
            let bp = bitpos + i;
            if (data[bp / 8] >> (bp % 8)) & 1 == 1 {
                code |= 1 << i;
            }
        }
        bitpos += code_size;
        if code == clear {
            dict.truncate(clear + 2);
            code_size = min_code as usize + 1;
            prev = None;
            continue;
        }
        if code == eoi {
            break;
        }
        let entry_first: u8;
        if code < dict.len() {
            // 사전에 있는 코드 → 문자열 펼치기.
            stack.clear();
            let mut c = code;
            loop {
                let (p, ch) = dict[c];
                stack.push(ch);
                match p {
                    Some(pp) => c = pp,
                    None => break,
                }
            }
            entry_first = *stack.last().ok_or("gif: lzw")?;
            while let Some(ch) = stack.pop() {
                out.push(ch);
            }
        } else if code == dict.len() && prev.is_some() {
            // KwKwK 경우.
            let mut c = prev.ok_or("gif: lzw")?;
            stack.clear();
            loop {
                let (p, ch) = dict[c];
                stack.push(ch);
                match p {
                    Some(pp) => c = pp,
                    None => break,
                }
            }
            entry_first = *stack.last().ok_or("gif: lzw")?;
            while let Some(ch) = stack.pop() {
                out.push(ch);
            }
            out.push(entry_first);
        } else {
            return Err("gif: bad lzw code".into());
        }
        if let Some(p) = prev {
            if dict.len() < 4096 {
                dict.push((Some(p), entry_first));
                if dict.len() == (1 << code_size) && code_size < 12 {
                    code_size += 1;
                }
            }
        }
        prev = Some(code);
    }
    out.resize(count, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_RGB: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12,
        0x16, 0xf1, 0x4d, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xe4,
        0x12, 0x91, 0x83, 0x00, 0x26, 0x46, 0x18, 0x00, 0x00, 0x0e, 0x0b, 0x00, 0xfd, 0x03, 0x25,
        0xe5, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const PNG_PAL: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x03, 0x00, 0x00, 0x00, 0x45,
        0x68, 0xfd, 0x16, 0x00, 0x00, 0x00, 0x0c, 0x50, 0x4c, 0x54, 0x45, 0xff, 0x00, 0x00, 0x00,
        0xff, 0x00, 0x00, 0x00, 0xff, 0x09, 0x09, 0x09, 0x5c, 0x71, 0x7e, 0x86, 0x00, 0x00, 0x00,
        0x04, 0x74, 0x52, 0x4e, 0x53, 0xff, 0x80, 0x00, 0xff, 0xa1, 0xa1, 0x94, 0x66, 0x00, 0x00,
        0x00, 0x0e, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x60, 0x64, 0x61, 0x62, 0x04,
        0x00, 0x00, 0x1b, 0x00, 0x09, 0xf5, 0xef, 0x23, 0x56, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
        0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const PNG_GRAY: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0xd1,
        0x49, 0x20, 0x56, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x4e,
        0x99, 0x06, 0x00, 0x01, 0x6a, 0x00, 0xfe, 0xa1, 0x56, 0xae, 0x57, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    const BMP: &[u8] = &[
        0x42, 0x4d, 0x46, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x28,
        0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x18, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x13, 0x0b, 0x00, 0x00, 0x13, 0x0b, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0xff, 0xff, 0xff,
        0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0x00, 0x00,
    ];
    const GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x02, 0x00, 0x02, 0x00, 0x81, 0x00, 0x00, 0xff, 0x00,
        0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
        0x02, 0x00, 0x02, 0x00, 0x00, 0x02, 0x03, 0x44, 0x02, 0x05, 0x00, 0x3b,
    ];

    fn px(img: &IconImage, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * img.w + x) * 4) as usize;
        [
            img.rgba[o],
            img.rgba[o + 1],
            img.rgba[o + 2],
            img.rgba[o + 3],
        ]
    }

    #[test]
    fn sniff_kinds() {
        assert_eq!(sniff(PNG_RGB), ImageKind::Png);
        assert_eq!(sniff(BMP), ImageKind::Bmp);
        assert_eq!(sniff(GIF), ImageKind::Gif);
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), ImageKind::Jpeg);
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), ImageKind::Webp);
        assert_eq!(sniff(b"hello"), ImageKind::Unknown);
        assert_eq!(dimensions(PNG_RGB), Some((3, 2)));
        assert_eq!(dimensions(BMP), Some((2, 2)));
        assert_eq!(dimensions(GIF), Some((2, 2)));
    }

    /// PNG RGB(Sub·Up 필터) · 팔레트+tRNS(Paeth) · 회색(Average).
    #[test]
    fn png_filters_and_types() {
        let i = decode(PNG_RGB, 1 << 20).expect("rgb");
        assert_eq!((i.w, i.h), (3, 2));
        assert_eq!(px(&i, 0, 0), [10, 20, 30, 255]);
        assert_eq!(px(&i, 2, 0), [70, 80, 90, 255]);
        assert_eq!(px(&i, 1, 1), [41, 51, 61, 255]);
        let p = decode(PNG_PAL, 1 << 20).expect("pal");
        assert_eq!(px(&p, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&p, 1, 0), [0, 255, 0, 128]);
        assert_eq!(px(&p, 0, 1), [0, 0, 255, 0]);
        assert_eq!(px(&p, 1, 1), [9, 9, 9, 255]);
        let g = decode(PNG_GRAY, 1 << 20).expect("gray");
        assert_eq!(px(&g, 0, 0), [100, 100, 100, 255]);
        assert_eq!(px(&g, 1, 0), [200, 200, 200, 255]);
    }

    #[test]
    fn bmp_bottom_up_24() {
        let i = decode(BMP, 1 << 20).expect("bmp");
        assert_eq!(px(&i, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&i, 1, 0), [0, 255, 0, 255]);
        assert_eq!(px(&i, 0, 1), [0, 0, 255, 255]);
        assert_eq!(px(&i, 1, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn gif_first_frame_lzw() {
        let i = decode(GIF, 1 << 20).expect("gif");
        assert_eq!((i.w, i.h), (2, 2));
        assert_eq!(px(&i, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&i, 1, 0), [0, 0, 255, 255]);
        assert_eq!(px(&i, 1, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn limits_and_errors() {
        assert!(decode(PNG_RGB, 5).expect_err("limit").contains("too large"));
        assert!(decode(&PNG_RGB[..30], 1 << 20).is_err());
        assert!(decode(&[0xff, 0xd8, 0xff, 0xe0], 1 << 20)
            .expect_err("jpeg")
            .to_ascii_lowercase()
            .contains("jpeg"));
        assert!(decode(b"nope", 1 << 20).is_err());
        assert!(decode(&GIF[..20], 1 << 20).is_err());
    }

    #[test]
    fn png_adam7_interlaced_matches_reference() {
        let hex = |t: &str| -> Vec<u8> {
            let t = t.trim();
            (0..t.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&t[i..i + 2], 16).expect("hex"))
                .collect()
        };
        let b = hex(include_str!("../tests/adam7.hex"));
        let want = hex(include_str!("../tests/adam7-ref.hex"));
        let img = decode(&b, 1 << 20).expect("adam7");
        assert_eq!((img.w, img.h), (9, 7));
        for i in 0..(9 * 7) {
            assert_eq!(
                &img.rgba[i * 4..i * 4 + 3],
                &want[i * 3..i * 3 + 3],
                "pixel {i}"
            );
            assert_eq!(img.rgba[i * 4 + 3], 255);
        }
    }
}
