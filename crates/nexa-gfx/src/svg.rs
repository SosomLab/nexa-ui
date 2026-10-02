//! `svg` — **최소 SVG 서브셋 파서 + CPU 래스터**(외부 crate 0 · 3-OS 동일 · nexa-dir3 T-62 C-2 / T-30 · 출처: nexa-dir2 `svg.rs` 파서 +
//! `gdipctx::render_svg_at`의 GDI+ 규약을 CPU 스캔라인으로 옮김).
//!
//! 용도 = 신뢰된 자산(툴바 아이콘 · 플러그인이 만든 다이어그램) 전용 — 일반 SVG 호환을 목표로 하지 않는다.
//!
//! ## 지원 서브셋(dir2와 같다 + `polygon`)
//! - 루트 `<svg>`: `viewBox`(필수) · `stroke-width`(기본 1) · 루트 `fill`/`stroke` 색 상속.
//! - 요소: `rect`(x/y/width/height/rx) · `circle` · `line` · `polyline` · `polygon` · `path`(`d` = M/L/H/V/C/A[원형]/Z, 상대 포함) ·
//!   `text`(x/y[베이스라인]/font-size/font-weight/text-anchor=middle — 글꼴은 호출자가 준 [`Font`]).
//! - 채움/스트로크: 요소 `fill`(`none` = 스트로크 · 색 = 채움 · 부재 = 루트 모드) · 색은 `#RRGGBB`(그 외 = 잉크) ·
//!   스트로크 = 라운드 캡/조인 · 최소 1px.
//!
//! ## 래스터
//! 도형 → 디바이스 좌표 폴리곤(곡선·호·라운드 모서리 평탄화) → **nonzero 스캔라인 커버리지**(행당 4 서브샘플 · 가로 분수 커버리지) →
//! [`Surface::blend_px`]. 스트로크 = 세그먼트 사각형 + 꼭짓점 원(라운드 조인/캡)의 nonzero 합집합. 텍스트 = [`Font::draw_styled`].
//! 결과는 불투명 RGB([`IconImage`] · 알파 255) — 배경색은 호출자가 준다(Surface는 알파를 들고 있지 않다).

use crate::surface::{Color, IconImage, Surface};
use crate::text::{Font, TextStyle};

/// 경로 세그먼트(절대 좌표로 정규화).
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// 3차 베지어(제어 1·제어 2·끝).
    CurveTo([(f32, f32); 3]),
    /// 원호(파싱 시 중심 매개변수로 해석 완료 — SVG `A`는 **원형만**: rx=ry·회전 0). 각도 = 도 · 양수 = 시계방향(화면 좌표).
    Arc {
        cx: f32,
        cy: f32,
        r: f32,
        start: f32,
        sweep: f32,
    },
    Close,
}

/// 드로 op — 좌표는 viewBox 기준(렌더러가 스케일).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// (라운드) 사각형(`rx` 0 = 직각).
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    /// 꺾은선(`closed` = `polygon`).
    Polyline(Vec<(f32, f32)>, bool),
    Path(Vec<Seg>),
    /// 텍스트(`y` = 베이스라인). `middle` = 수평 중앙 앵커.
    Text {
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        middle: bool,
        content: String,
    },
}

/// 문서 내 한 요소 — op + 색/채움/굵기 오버라이드(부재 = 루트 상속 · 색 부재 = 렌더러 잉크).
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub op: Op,
    pub color: Option<u32>,
    /// `fill` 속성: `none` = `Some(false)`(스트로크) · 색 = `Some(true)`(채움) · 부재 = `None`(루트 모드).
    pub fill: Option<bool>,
    /// 요소별 `stroke-width`.
    pub width: Option<f32>,
}

/// 파싱된 문서 — viewBox `(x, y, w, h)` + 루트 스트로크 폭 + 요소 목록.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub viewbox: (f32, f32, f32, f32),
    pub stroke_width: f32,
    /// 루트 `fill` 채움 모드(true = 도형 채움 · false = 스트로크).
    pub fill: bool,
    /// 루트 `stroke`/`fill` 색 — 요소가 자체 색을 안 주면 상속. 없으면 잉크.
    pub root_stroke: Option<u32>,
    pub root_fill: Option<u32>,
    pub ops: Vec<Element>,
}

impl Doc {
    /// viewBox 크기를 올림한 자연 픽셀 크기.
    #[must_use]
    pub fn natural_size(&self) -> (u32, u32) {
        (
            self.viewbox.2.ceil().max(1.0) as u32,
            self.viewbox.3.ceil().max(1.0) as u32,
        )
    }
}

// ─────────────────────────────────────────────────────────────── 파서

/// SVG 텍스트 파싱. viewBox 없음/형식 오류/요소 0 = `None`(오류 격리).
#[must_use]
pub fn parse(svg: &str) -> Option<Doc> {
    let mut doc = Doc {
        viewbox: (0.0, 0.0, 0.0, 0.0),
        stroke_width: 1.0,
        fill: false,
        root_stroke: None,
        root_fill: None,
        ops: Vec::new(),
    };
    let mut seen_root = false;
    for chunk in svg.split('<').skip(1) {
        let Some(gt) = chunk.find('>') else {
            continue;
        };
        let tag = chunk[..gt].trim_end_matches('/').trim();
        let inner = &chunk[gt + 1..];
        let (name, attrs) = match tag.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a),
            None => (tag, ""),
        };
        let colors = |attrs: &str| resolve_color(attrs, doc.root_stroke, doc.root_fill, doc.fill);
        match name {
            "svg" => {
                let vb = attr(attrs, "viewBox")?;
                let v: Vec<f32> = vb
                    .split([' ', ','])
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if v.len() != 4 || v[2] <= 0.0 || v[3] <= 0.0 {
                    return None;
                }
                doc.viewbox = (v[0], v[1], v[2], v[3]);
                if let Some(sw) = attr(attrs, "stroke-width").and_then(|s| s.parse().ok()) {
                    doc.stroke_width = sw;
                }
                doc.fill = attr(attrs, "fill").is_some_and(|f| f != "none");
                doc.root_stroke = hex_color(attr(attrs, "stroke").as_deref());
                doc.root_fill = hex_color(attr(attrs, "fill").as_deref());
                seen_root = true;
            }
            "rect" => {
                let el = Element {
                    op: Op::Rect {
                        x: num(attrs, "x"),
                        y: num(attrs, "y"),
                        w: num(attrs, "width"),
                        h: num(attrs, "height"),
                        rx: num(attrs, "rx"),
                    },
                    color: colors(attrs),
                    fill: elem_fill(attrs),
                    width: elem_width(attrs),
                };
                doc.ops.push(el);
            }
            "circle" => {
                let el = Element {
                    op: Op::Circle {
                        cx: num(attrs, "cx"),
                        cy: num(attrs, "cy"),
                        r: num(attrs, "r"),
                    },
                    color: colors(attrs),
                    fill: elem_fill(attrs),
                    width: elem_width(attrs),
                };
                doc.ops.push(el);
            }
            "line" => {
                let el = Element {
                    op: Op::Line {
                        x1: num(attrs, "x1"),
                        y1: num(attrs, "y1"),
                        x2: num(attrs, "x2"),
                        y2: num(attrs, "y2"),
                    },
                    color: colors(attrs),
                    fill: elem_fill(attrs),
                    width: elem_width(attrs),
                };
                doc.ops.push(el);
            }
            "polyline" | "polygon" => {
                let pts: Vec<f32> = attr(attrs, "points")
                    .unwrap_or_default()
                    .split([' ', ','])
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let pairs: Vec<(f32, f32)> = pts.chunks_exact(2).map(|p| (p[0], p[1])).collect();
                if pairs.len() >= 2 {
                    let el = Element {
                        op: Op::Polyline(pairs, name == "polygon"),
                        color: colors(attrs),
                        fill: elem_fill(attrs),
                        width: elem_width(attrs),
                    };
                    doc.ops.push(el);
                }
            }
            "path" => {
                if let Some(d) = attr(attrs, "d") {
                    let segs = parse_path(&d)?;
                    if !segs.is_empty() {
                        let el = Element {
                            op: Op::Path(segs),
                            color: colors(attrs),
                            fill: elem_fill(attrs),
                            width: elem_width(attrs),
                        };
                        doc.ops.push(el);
                    }
                }
            }
            "text" => {
                let content = unescape(inner.split('<').next().unwrap_or("").trim());
                if !content.is_empty() {
                    let el = Element {
                        op: Op::Text {
                            x: num(attrs, "x"),
                            y: num(attrs, "y"),
                            size: attr(attrs, "font-size")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(10.0),
                            bold: attr(attrs, "font-weight").is_some_and(|w| {
                                w == "bold" || w.parse::<i32>().is_ok_and(|n| n >= 600)
                            }),
                            middle: attr(attrs, "text-anchor").is_some_and(|a| a == "middle"),
                            content,
                        },
                        color: colors(attrs),
                        fill: elem_fill(attrs),
                        width: elem_width(attrs),
                    };
                    doc.ops.push(el);
                }
            }
            _ => {}
        }
    }
    (seen_root && !doc.ops.is_empty()).then_some(doc)
}

/// XML 엔티티 5종 풀기(텍스트 본문).
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 속성값 추출 — `k="v"` 형태(단순 스캔 · 키 경계 확인: `x`가 `rx`에 매칭되지 않게).
fn attr(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    while let Some(i) = rest.find(key) {
        let before_ok = i == 0
            || !rest[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '-');
        let after = &rest[i + key.len()..];
        if before_ok {
            if let Some(v) = after.strip_prefix('=') {
                let v = v.trim_start();
                let quote = v.chars().next()?;
                if quote == '"' || quote == '\'' {
                    return v[1..].split(quote).next().map(str::to_string);
                }
            }
        }
        rest = &rest[i + key.len()..];
    }
    None
}

fn elem_fill(attrs: &str) -> Option<bool> {
    attr(attrs, "fill").map(|v| v != "none")
}

fn elem_width(attrs: &str) -> Option<f32> {
    attr(attrs, "stroke-width").and_then(|v| v.parse().ok())
}

/// `#RRGGBB`(6자리) → `Some(rgb)`. `currentColor`·`none`·부재·형식 오류 = `None`.
fn hex_color(v: Option<&str>) -> Option<u32> {
    let hex = v?.strip_prefix('#')?;
    if hex.len() == 3 {
        let mut out = 0u32;
        for c in hex.chars() {
            let d = c.to_digit(16)?;
            out = (out << 8) | (d * 17);
        }
        return Some(out);
    }
    (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

fn elem_color(attrs: &str) -> Option<u32> {
    hex_color(attr(attrs, "stroke").as_deref())
        .or_else(|| hex_color(attr(attrs, "fill").as_deref()))
}

/// 요소 색 — 자체 색 → 루트 상속(채움 요소 = 루트 `fill` · 스트로크 요소 = 루트 `stroke`) → `None`(잉크).
fn resolve_color(
    attrs: &str,
    root_stroke: Option<u32>,
    root_fill: Option<u32>,
    root_fill_mode: bool,
) -> Option<u32> {
    if let Some(c) = elem_color(attrs) {
        return Some(c);
    }
    let filled = elem_fill(attrs).unwrap_or(root_fill_mode);
    if filled {
        root_fill
    } else {
        root_stroke
    }
}

fn num(attrs: &str, key: &str) -> f32 {
    attr(attrs, key).and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

/// path `d` 파싱 — 상대 명령은 절대 좌표로 정규화. 미지 명령 = `None`(전체 무효).
fn parse_path(d: &str) -> Option<Vec<Seg>> {
    let mut segs = Vec::new();
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut nums: Vec<f32> = Vec::new();
    let mut cmd = ' ';
    let mut it = d.chars().peekable();
    loop {
        nums.clear();
        while let Some(&c) = it.peek() {
            if c.is_ascii_alphabetic() {
                break;
            }
            it.next();
            if c == ',' || c.is_whitespace() {
                continue;
            }
            let mut s = String::new();
            s.push(c);
            while let Some(&n) = it.peek() {
                if n.is_ascii_digit() || n == '.' {
                    s.push(n);
                    it.next();
                } else {
                    break;
                }
            }
            nums.push(s.parse().ok()?);
        }
        if cmd != ' ' {
            apply_cmd(cmd, &nums, &mut segs, &mut cx, &mut cy, &mut sx, &mut sy)?;
        } else if !nums.is_empty() {
            return None;
        }
        match it.next() {
            Some(c) => cmd = c,
            None => break,
        }
    }
    Some(segs)
}

/// 한 명령 적용(반복 인자 허용 — 예: `L x1 y1 x2 y2`).
#[allow(clippy::too_many_arguments)]
fn apply_cmd(
    cmd: char,
    n: &[f32],
    segs: &mut Vec<Seg>,
    cx: &mut f32,
    cy: &mut f32,
    sx: &mut f32,
    sy: &mut f32,
) -> Option<()> {
    let rel = cmd.is_ascii_lowercase();
    match cmd.to_ascii_uppercase() {
        'M' => {
            for (i, p) in n.chunks_exact(2).enumerate() {
                let (x, y) = if rel {
                    (*cx + p[0], *cy + p[1])
                } else {
                    (p[0], p[1])
                };
                *cx = x;
                *cy = y;
                if i == 0 {
                    *sx = x;
                    *sy = y;
                    segs.push(Seg::MoveTo(x, y));
                } else {
                    segs.push(Seg::LineTo(x, y));
                }
            }
            (n.len() >= 2 && n.len() % 2 == 0).then_some(())
        }
        'L' => {
            for p in n.chunks_exact(2) {
                let (x, y) = if rel {
                    (*cx + p[0], *cy + p[1])
                } else {
                    (p[0], p[1])
                };
                *cx = x;
                *cy = y;
                segs.push(Seg::LineTo(x, y));
            }
            (n.len() >= 2 && n.len() % 2 == 0).then_some(())
        }
        'H' => {
            for &v in n {
                *cx = if rel { *cx + v } else { v };
                segs.push(Seg::LineTo(*cx, *cy));
            }
            (!n.is_empty()).then_some(())
        }
        'V' => {
            for &v in n {
                *cy = if rel { *cy + v } else { v };
                segs.push(Seg::LineTo(*cx, *cy));
            }
            (!n.is_empty()).then_some(())
        }
        'C' => {
            for p in n.chunks_exact(6) {
                let f = |i: usize| {
                    if rel {
                        (*cx + p[i], *cy + p[i + 1])
                    } else {
                        (p[i], p[i + 1])
                    }
                };
                let pts = [f(0), f(2), f(4)];
                *cx = pts[2].0;
                *cy = pts[2].1;
                segs.push(Seg::CurveTo(pts));
            }
            (n.len() >= 6 && n.len() % 6 == 0).then_some(())
        }
        'A' => {
            for p in n.chunks_exact(7) {
                let (rx, ry) = (p[0].abs(), p[1].abs());
                if (rx - ry).abs() > 0.01 || rx <= 0.0 {
                    return None;
                }
                let (fa, fs) = (p[3] != 0.0, p[4] != 0.0);
                let (ex, ey) = if rel {
                    (*cx + p[5], *cy + p[6])
                } else {
                    (p[5], p[6])
                };
                let (dx, dy) = ((*cx - ex) / 2.0, (*cy - ey) / 2.0);
                let d2 = dx * dx + dy * dy;
                if d2 <= 0.0 {
                    return None;
                }
                let r = rx.max(d2.sqrt());
                let sign = if fa != fs { 1.0 } else { -1.0 };
                let k = sign * ((r * r - d2) / d2).max(0.0).sqrt();
                let (ccx, ccy) = ((*cx + ex) / 2.0 + k * dy, (*cy + ey) / 2.0 - k * dx);
                let th1 = (*cy - ccy).atan2(*cx - ccx).to_degrees();
                let th2 = (ey - ccy).atan2(ex - ccx).to_degrees();
                let mut sweep = th2 - th1;
                if fs && sweep < 0.0 {
                    sweep += 360.0;
                } else if !fs && sweep > 0.0 {
                    sweep -= 360.0;
                }
                segs.push(Seg::Arc {
                    cx: ccx,
                    cy: ccy,
                    r,
                    start: th1,
                    sweep,
                });
                *cx = ex;
                *cy = ey;
            }
            (n.len() >= 7 && n.len() % 7 == 0).then_some(())
        }
        'Z' => {
            *cx = *sx;
            *cy = *sy;
            segs.push(Seg::Close);
            Some(())
        }
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────── 래스터

type Pt = (f32, f32);

/// 서브패스(디바이스 좌표 · `closed` = Z 또는 닫힌 도형).
struct Sub {
    pts: Vec<Pt>,
    closed: bool,
}

/// 요소 기하 → 서브패스 목록(곡선·호·라운드 모서리 평탄화). 텍스트는 빈 목록.
fn subpaths(op: &Op, sx: &dyn Fn(f32) -> f32, sy: &dyn Fn(f32) -> f32, scale: f32) -> Vec<Sub> {
    let mut out = Vec::new();
    match op {
        Op::Rect { x, y, w, h, rx } => {
            let (l, t, w2, h2) = (sx(*x), sy(*y), w * scale, h * scale);
            if w2 <= 0.0 || h2 <= 0.0 {
                return out;
            }
            let r = (rx * scale).clamp(0.0, w2.min(h2) / 2.0);
            let mut pts = Vec::new();
            if r <= 0.01 {
                pts.extend([(l, t), (l + w2, t), (l + w2, t + h2), (l, t + h2)]);
            } else {
                // 네 모서리 호(시계방향 · 좌상 → 우상 → 우하 → 좌하).
                for (ccx, ccy, a0) in [
                    (l + r, t + r, 180.0),
                    (l + w2 - r, t + r, 270.0),
                    (l + w2 - r, t + h2 - r, 0.0),
                    (l + r, t + h2 - r, 90.0),
                ] {
                    arc_points(&mut pts, ccx, ccy, r, a0, 90.0);
                }
            }
            out.push(Sub { pts, closed: true });
        }
        Op::Circle { cx, cy, r } => {
            let mut pts = Vec::new();
            arc_points(&mut pts, sx(*cx), sy(*cy), r * scale, 0.0, 360.0);
            out.push(Sub { pts, closed: true });
        }
        Op::Line { x1, y1, x2, y2 } => out.push(Sub {
            pts: vec![(sx(*x1), sy(*y1)), (sx(*x2), sy(*y2))],
            closed: false,
        }),
        Op::Polyline(pts, closed) => out.push(Sub {
            pts: pts.iter().map(|(x, y)| (sx(*x), sy(*y))).collect(),
            closed: *closed,
        }),
        Op::Path(segs) => {
            let mut cur: Vec<Pt> = Vec::new();
            let (mut cx, mut cy) = (0.0f32, 0.0f32);
            for seg in segs {
                match *seg {
                    Seg::MoveTo(x, y) => {
                        if cur.len() >= 2 {
                            out.push(Sub {
                                pts: std::mem::take(&mut cur),
                                closed: false,
                            });
                        }
                        cur.clear();
                        cur.push((sx(x), sy(y)));
                        (cx, cy) = (x, y);
                    }
                    Seg::LineTo(x, y) => {
                        if cur.is_empty() {
                            cur.push((sx(cx), sy(cy)));
                        }
                        cur.push((sx(x), sy(y)));
                        (cx, cy) = (x, y);
                    }
                    Seg::CurveTo([c1, c2, e]) => {
                        if cur.is_empty() {
                            cur.push((sx(cx), sy(cy)));
                        }
                        let p0 = (sx(cx), sy(cy));
                        let (p1, p2, p3) = (
                            (sx(c1.0), sy(c1.1)),
                            (sx(c2.0), sy(c2.1)),
                            (sx(e.0), sy(e.1)),
                        );
                        for i in 1..=16 {
                            let t = i as f32 / 16.0;
                            let u = 1.0 - t;
                            let x = u * u * u * p0.0
                                + 3.0 * u * u * t * p1.0
                                + 3.0 * u * t * t * p2.0
                                + t * t * t * p3.0;
                            let y = u * u * u * p0.1
                                + 3.0 * u * u * t * p1.1
                                + 3.0 * u * t * t * p2.1
                                + t * t * t * p3.1;
                            cur.push((x, y));
                        }
                        (cx, cy) = (e.0, e.1);
                    }
                    Seg::Arc {
                        cx: acx,
                        cy: acy,
                        r,
                        start,
                        sweep,
                    } => {
                        let mut pts = Vec::new();
                        arc_points(&mut pts, sx(acx), sy(acy), r * scale, start, sweep);
                        if cur.is_empty() {
                            cur.push((sx(cx), sy(cy)));
                        }
                        cur.extend(pts.into_iter().skip(1));
                        let end = (start + sweep).to_radians();
                        (cx, cy) = (acx + r * end.cos(), acy + r * end.sin());
                    }
                    Seg::Close => {
                        if cur.len() >= 2 {
                            out.push(Sub {
                                pts: std::mem::take(&mut cur),
                                closed: true,
                            });
                        }
                        cur.clear();
                    }
                }
            }
            if cur.len() >= 2 {
                out.push(Sub {
                    pts: cur,
                    closed: false,
                });
            }
        }
        Op::Text { .. } => {}
    }
    out
}

/// 원호 점열(시작점 포함 · `sweep` 양수 = 시계방향[화면 좌표] · 6°당 1점 이상).
fn arc_points(pts: &mut Vec<Pt>, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32) {
    let n = ((sweep_deg.abs() / 6.0).ceil() as usize).clamp(2, 128);
    for i in 0..=n {
        let a = (start_deg + sweep_deg * i as f32 / n as f32).to_radians();
        pts.push((cx + r * a.cos(), cy + r * a.sin()));
    }
}

/// 꼭짓점 원(라운드 캡/조인).
fn disc(cx: f32, cy: f32, r: f32) -> Vec<Pt> {
    let mut pts = Vec::with_capacity(13);
    arc_points(&mut pts, cx, cy, r, 0.0, 360.0);
    pts.pop();
    pts
}

/// 폴리곤 방향을 양(+) 면적으로 통일(nonzero 합집합이 상쇄되지 않게).
fn orient(mut poly: Vec<Pt>) -> Vec<Pt> {
    let mut area = 0.0f32;
    for i in 0..poly.len() {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % poly.len()];
        area += x0 * y1 - x1 * y0;
    }
    if area < 0.0 {
        poly.reverse();
    }
    poly
}

/// 스트로크 폴리곤(세그먼트 사각형 + 꼭짓점 원).
fn stroke_polys(sub: &Sub, width: f32) -> Vec<Vec<Pt>> {
    let hw = width / 2.0;
    let mut out = Vec::new();
    let n = sub.pts.len();
    if n == 0 {
        return out;
    }
    let seg_count = if sub.closed && n > 2 { n } else { n - 1 };
    for i in 0..seg_count {
        let (x0, y0) = sub.pts[i];
        let (x1, y1) = sub.pts[(i + 1) % n];
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-4 {
            continue;
        }
        let (nx, ny) = (-dy / len * hw, dx / len * hw);
        out.push(orient(vec![
            (x0 + nx, y0 + ny),
            (x1 + nx, y1 + ny),
            (x1 - nx, y1 - ny),
            (x0 - nx, y0 - ny),
        ]));
    }
    if hw > 0.6 {
        for &(x, y) in &sub.pts {
            out.push(orient(disc(x, y, hw)));
        }
    }
    out
}

/// nonzero 스캔라인 커버리지(행당 4 서브샘플 · 가로 분수 커버리지) → `mask[y*w+x]`(0..1).
fn coverage(polys: &[Vec<Pt>], w: usize, h: usize, mask: &mut [f32]) {
    const SUB: usize = 4;
    struct Edge {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        dir: i32,
    }
    let mut edges: Vec<Edge> = Vec::new();
    let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
    for poly in polys {
        let n = poly.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let (ax, ay) = poly[i];
            let (bx, by) = poly[(i + 1) % n];
            if ay == by || !(ax.is_finite() && ay.is_finite() && bx.is_finite() && by.is_finite()) {
                continue;
            }
            let (x0, y0, x1, y1, dir) = if ay < by {
                (ax, ay, bx, by, 1)
            } else {
                (bx, by, ax, ay, -1)
            };
            ymin = ymin.min(y0);
            ymax = ymax.max(y1);
            edges.push(Edge {
                x0,
                y0,
                x1,
                y1,
                dir,
            });
        }
    }
    if edges.is_empty() {
        return;
    }
    let row_lo = (ymin.floor().max(0.0)) as usize;
    let row_hi = (ymax.ceil().min(h as f32)) as usize;
    let mut xs: Vec<(f32, i32)> = Vec::new();
    let inc = 1.0 / SUB as f32;
    for row in row_lo..row_hi {
        for s in 0..SUB {
            let sy = row as f32 + (s as f32 + 0.5) * inc;
            xs.clear();
            for e in &edges {
                if sy >= e.y0 && sy < e.y1 {
                    let x = e.x0 + (sy - e.y0) * (e.x1 - e.x0) / (e.y1 - e.y0);
                    xs.push((x, e.dir));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut wind = 0;
            let mut span_start = 0.0f32;
            for &(x, dir) in &xs {
                let was = wind;
                wind += dir;
                if was == 0 && wind != 0 {
                    span_start = x;
                } else if was != 0 && wind == 0 {
                    let (a, b) = (span_start.max(0.0), x.min(w as f32));
                    if b <= a {
                        continue;
                    }
                    let px0 = a.floor() as usize;
                    let px1 = (b.ceil() as usize).min(w);
                    let base = row * w;
                    for px in px0..px1 {
                        let l = a.max(px as f32);
                        let r = b.min(px as f32 + 1.0);
                        if r > l {
                            mask[base + px] += (r - l) * inc;
                        }
                    }
                }
            }
        }
    }
}

/// 렌더 옵션.
#[derive(Debug, Clone, Copy)]
pub struct RenderOpts {
    /// 색 지정이 없는 요소의 잉크.
    pub ink: Color,
    /// 배경(불투명 결과).
    pub bg: Color,
}

/// 문서를 `w × h` 픽셀로 래스터(viewBox를 비율 유지로 맞춤 · 좌상 정렬). `font` = 텍스트 글꼴(없으면 텍스트 생략).
#[must_use]
pub fn render(doc: &Doc, w: u32, h: u32, opts: RenderOpts, font: Option<&Font>) -> IconImage {
    let (w, h) = (w.max(1), h.max(1));
    let mut buf = vec![0u32; (w * h) as usize];
    {
        let mut surface = Surface::new(&mut buf, w as usize, h as usize);
        surface.fill(opts.bg);
        let (vx, vy, vw, vh) = doc.viewbox;
        let scale = (w as f32 / vw).min(h as f32 / vh);
        let sx = |x: f32| (x - vx) * scale;
        let sy = |y: f32| (y - vy) * scale;
        let mut mask = vec![0f32; (w * h) as usize];
        for el in &doc.ops {
            let color = match el.color {
                Some(rgb) => Color::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8),
                None => opts.ink,
            };
            if let Op::Text {
                x,
                y,
                size,
                bold,
                middle,
                content,
            } = &el.op
            {
                let Some(font) = font else { continue };
                let px = (size * scale).max(1.0);
                let mut left = sx(*x);
                if *middle {
                    left -= font.measure_from_styled(content, px, 0.0, *bold) / 2.0;
                }
                let clip = (0, 0, w as i32, h as i32);
                let _ = font.draw_styled(
                    &mut surface,
                    left,
                    sy(*y),
                    px,
                    color,
                    content,
                    clip,
                    TextStyle {
                        bold: *bold,
                        italic: false,
                    },
                    left,
                );
                continue;
            }
            let subs = subpaths(&el.op, &sx, &sy, scale);
            if subs.is_empty() {
                continue;
            }
            let filled = el.fill.unwrap_or(doc.fill);
            let polys: Vec<Vec<Pt>> = if filled {
                subs.into_iter()
                    .filter(|s| s.pts.len() >= 3)
                    .map(|s| s.pts)
                    .collect()
            } else {
                let width = (el.width.unwrap_or(doc.stroke_width) * scale).max(1.0);
                subs.iter().flat_map(|s| stroke_polys(s, width)).collect()
            };
            if polys.is_empty() {
                continue;
            }
            mask.iter_mut().for_each(|m| *m = 0.0);
            coverage(&polys, w as usize, h as usize, &mut mask);
            for (i, m) in mask.iter().enumerate() {
                if *m > 0.002 {
                    surface.blend_px(
                        (i % w as usize) as i32,
                        (i / w as usize) as i32,
                        color,
                        m.min(1.0),
                    );
                }
            }
        }
    }
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for p in buf {
        rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
    }
    IconImage::from_rgba(w, h, rgba)
}

/// viewBox 크기 그대로(올림) 래스터 — 한 변 `max_side`를 넘으면 `None`(비정상 문서 보호).
#[must_use]
pub fn render_natural(
    doc: &Doc,
    opts: RenderOpts,
    font: Option<&Font>,
    max_side: u32,
) -> Option<IconImage> {
    let (w, h) = doc.natural_size();
    if w > max_side || h > max_side {
        return None;
    }
    Some(render(doc, w, h, opts, font))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &IconImage, x: u32, y: u32) -> (u8, u8, u8) {
        let i = ((y * img.w + x) * 4) as usize;
        (img.rgba[i], img.rgba[i + 1], img.rgba[i + 2])
    }

    fn opts() -> RenderOpts {
        RenderOpts {
            ink: Color::from_rgb(0, 0, 0),
            bg: Color::from_rgb(255, 255, 255),
        }
    }

    #[test]
    fn path_relative_and_close() {
        let svg = r##"<svg viewBox="0 0 10 10"><path d="M1 1 l2 0 v2 h-2 z"/></svg>"##;
        let doc = parse(svg).expect("parse");
        assert_eq!(
            doc.ops[0].op,
            Op::Path(vec![
                Seg::MoveTo(1.0, 1.0),
                Seg::LineTo(3.0, 1.0),
                Seg::LineTo(3.0, 3.0),
                Seg::LineTo(1.0, 3.0),
                Seg::Close,
            ])
        );
        assert_eq!(doc.ops[0].color, None, "currentColor = 잉크");
    }

    #[test]
    fn curve_negative_numbers_colors_and_text() {
        let svg = r##"<svg viewBox="0 0 10 10" fill="#112233"><path d="M0 5C1 -1,4 -1,5 5" fill="none" stroke="#abc"/><text x="2" y="8" font-size="4" font-weight="bold" text-anchor="middle">a &amp; b</text><polygon points="0,0 2,0 1,2"/></svg>"##;
        let doc = parse(svg).expect("parse");
        assert_eq!(doc.ops.len(), 3);
        assert_eq!(
            doc.ops[0].op,
            Op::Path(vec![
                Seg::MoveTo(0.0, 5.0),
                Seg::CurveTo([(1.0, -1.0), (4.0, -1.0), (5.0, 5.0)])
            ])
        );
        assert_eq!(doc.ops[0].color, Some(0xAABBCC), "3자리 색 확장");
        assert_eq!(doc.ops[0].fill, Some(false));
        assert!(
            matches!(&doc.ops[1].op, Op::Text { bold: true, middle: true, content, .. } if content == "a & b")
        );
        assert_eq!(doc.ops[1].color, Some(0x112233), "루트 fill 상속");
        assert!(matches!(doc.ops[2].op, Op::Polyline(_, true)));
        assert!(parse("<svg><rect/></svg>").is_none(), "viewBox 없음");
        assert!(
            parse(r##"<svg viewBox="0 0 1 1"></svg>"##).is_none(),
            "요소 0"
        );
        assert!(
            parse(r##"<svg viewBox="0 0 1 1"><path d="M0 0 Q 1 1 2 2"/></svg>"##).is_none(),
            "미지원 명령"
        );
    }

    /// 채움 사각형 = 안은 색 · 밖은 배경 · 경계 픽셀은 중간(AA).
    #[test]
    fn render_filled_rect_and_stroke_line() {
        let svg = r##"<svg viewBox="0 0 20 20"><rect x="2" y="2" width="10" height="10" fill="#ff0000"/><line x1="0" y1="17.5" x2="20" y2="17.5" stroke="#0000ff" stroke-width="1"/></svg>"##;
        let doc = parse(svg).expect("parse");
        let img = render(&doc, 20, 20, opts(), None);
        assert_eq!((img.w, img.h), (20, 20));
        assert_eq!(px(&img, 5, 5), (255, 0, 0), "채움 안");
        assert_eq!(px(&img, 15, 5), (255, 255, 255), "채움 밖");
        assert_eq!(px(&img, 10, 17), (0, 0, 255), "선 위(1px · 행 17)");
        assert_eq!(px(&img, 10, 14), (255, 255, 255), "선 밖");
        let (w, h) = doc.natural_size();
        assert_eq!((w, h), (20, 20));
        assert!(
            render_natural(&doc, opts(), None, 10).is_none(),
            "상한 초과"
        );
    }

    /// 라운드 사각형 스트로크: 모서리 밖은 배경 · 변 중앙은 잉크 · 2배 스케일.
    #[test]
    fn render_rounded_stroke_scales() {
        let svg = r##"<svg viewBox="0 0 10 10" stroke-width="1"><rect x="1" y="1" width="8" height="8" rx="3"/></svg>"##;
        let doc = parse(svg).expect("parse");
        let img = render(&doc, 20, 20, opts(), None);
        assert_eq!(px(&img, 2, 2), (255, 255, 255), "둥근 모서리 밖");
        let (r, g, b) = px(&img, 10, 2);
        assert!(
            r < 128 && g < 128 && b < 128,
            "위 변 중앙은 잉크(2px 폭): {:?}",
            (r, g, b)
        );
        assert_eq!(px(&img, 10, 10), (255, 255, 255), "스트로크 안쪽은 비었다");
    }

    /// nonzero: 같은 방향 두 삼각형이 겹쳐도 커버리지는 1을 넘지 않는다(블렌드 포화 없음).
    #[test]
    fn coverage_nonzero_union() {
        let a = orient(vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
        let b = orient(vec![(2.0, 2.0), (6.0, 2.0), (6.0, 6.0), (2.0, 6.0)]);
        let mut mask = vec![0f32; 64];
        coverage(&[a, b], 8, 8, &mut mask);
        assert!((mask[3 * 8 + 3] - 1.0).abs() < 1e-3, "겹침 = 1");
        assert!((mask[8 + 1] - 1.0).abs() < 1e-3);
        assert!((mask[5 * 8 + 5] - 1.0).abs() < 1e-3);
        assert_eq!(mask[7 * 8 + 7], 0.0);
    }
}
