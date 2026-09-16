//! 글꼴 `name` 테이블에서 **패밀리 이름 전부**(nameID 1 · 16 — 영문·현지어 · 09-16)를 읽는다.
//! OS 래스터라이저(GDI)는 로케일에 따라 영문 이름만 찾거나 현지어 이름만 돌려주므로(영문 Windows에서 "맑은 고딕"
//! 요청은 대체 글꼴이 된다 — CI 실패) 후보 전부를 알아야 한다. 의존 0 · 실패는 빈 목록.

fn u16be(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

fn u32be(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(o)?,
        *d.get(o + 1)?,
        *d.get(o + 2)?,
        *d.get(o + 3)?,
    ]))
}

/// 컬렉션(`ttcf`)이면 `index`번째 sfnt 오프셋 · 아니면 0.
fn sfnt_offset(data: &[u8], index: u32) -> Option<usize> {
    if data.get(0..4)? == b"ttcf" {
        let n = u32be(data, 8)?;
        if index >= n {
            return None;
        }
        return Some(u32be(data, 12 + 4 * index as usize)? as usize);
    }
    Some(0)
}

/// `name` 테이블 바이트.
fn name_table(data: &[u8], index: u32) -> Option<&[u8]> {
    let base = sfnt_offset(data, index)?;
    let num = u16be(data, base + 4)? as usize;
    for i in 0..num {
        let rec = base + 12 + i * 16;
        if data.get(rec..rec + 4)? == b"name" {
            let off = u32be(data, rec + 8)? as usize;
            let len = u32be(data, rec + 12)? as usize;
            return data.get(off..off + len);
        }
    }
    None
}

/// 패밀리 이름들(중복 제거 · 테이블 순서). nameID 1(패밀리) · 16(타이포그래픽 패밀리) · 플랫폼 0/3 = UTF-16BE ·
/// 플랫폼 1 = 단일 바이트(ASCII 범위만 신뢰).
#[must_use]
pub(crate) fn family_names(data: &[u8], index: u32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(t) = name_table(data, index) else {
        return out;
    };
    let (Some(count), Some(str_off)) = (u16be(t, 2), u16be(t, 4)) else {
        return out;
    };
    for i in 0..count as usize {
        let r = 6 + i * 12;
        let (Some(plat), Some(name_id), Some(len), Some(off)) = (
            u16be(t, r),
            u16be(t, r + 6),
            u16be(t, r + 8),
            u16be(t, r + 10),
        ) else {
            break;
        };
        if name_id != 1 && name_id != 16 {
            continue;
        }
        let s = str_off as usize + off as usize;
        let Some(bytes) = t.get(s..s + len as usize) else {
            continue;
        };
        let name = match plat {
            0 | 3 => {
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            1 => {
                if !bytes.is_ascii() {
                    continue;
                }
                String::from_utf8_lossy(bytes).into_owned()
            }
            _ => continue,
        };
        let name = name.trim().to_string();
        if !name.is_empty() && !out.iter().any(|n| n == &name) {
            out.push(name);
        }
    }
    out
}
