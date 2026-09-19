//! 편집기 페인트·캐럿 이동 벤치(nexa-sql 사용자 09-19 "커서 이동이 느린 것 같다" 실측용).
//! `cargo run --release -p nexa-ctl --example bench_editor [lines]` — 2 MB급 본문을 멀티라인 TextBox에 넣고
//! ① 오프스크린 페인트 30회 평균 ② ↓ 키 100회 평균 ③ Home/End 100회 평균 ④ text()+collect 1회를 ms로 찍는다.
use nexa_ctl::controls::TextBox;
use nexa_ctl::event::{InputEvent, Key};
use nexa_ctl::geom::Rect;
use nexa_ctl::raster::RasterCtx;
use nexa_ctl::theme::{FontPrefs, SlotFont, Theme};
use nexa_ctl::widget::{Invalidations, Widget};
use nexa_ctl::Control;
use nexa_gfx::Surface;
use std::time::Instant;

fn main() {
    let lines: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);
    let mut text = String::new();
    for i in 0..lines {
        text.push_str(&format!(
            "SELECT COL_{} AS C{i}, '한글 문자열 {i}' AS K, SYSDATE FROM DUAL WHERE ROWNUM <= {} AND 1=1;\n",
            i % 97,
            i % 50
        ));
    }
    println!("text: {} bytes · {} lines", text.len(), lines);
    let Some(mono) = nexa_font::mono_font(None) else {
        eprintln!("no mono font");
        return;
    };
    let (w, h) = (1600usize, 1000usize);
    let mut buf = vec![0u32; w * h];
    let theme = Theme::default();
    let mut tb = TextBox::new("").with_multiline();
    let mut inv = Invalidations::default();
    tb.set_bounds(Rect::new(0, 0, w as i32, h as i32), &mut inv);
    tb.set_focused(true);
    tb.set_text(&text);
    let prefs = FontPrefs {
        base: SlotFont {
            size: 14.0,
            bold: false,
            italic: false,
        },
        ..FontPrefs::default()
    };
    let paint = |tb: &TextBox, buf: &mut [u32]| {
        let mut s = Surface::new(buf, w, h);
        let mut dc = RasterCtx::new(&mut s, &mono.font, 1.0).with_fonts(prefs);
        tb.paint(&mut dc, &theme);
    };
    // 앱과 같은 구성(nexa-sql 09-19 "기능을 켠 실제 비용"): 둘째 인자 = 켤 기능(쉼표 · all = 전부).
    //   hl = SQL 구문 · ln = 줄번호+변경 띠 · base = 줄 변경 기준선 · mm = 미니맵 · occ = 선택어 강조 · br = 괄호 짝
    let feats = std::env::args().nth(2).unwrap_or_default();
    let on = |f: &str| feats == "all" || feats.split(',').any(|x| x == f);
    if on("hl") {
        tb.set_highlighter(Some(std::rc::Rc::new(nexa_ctl::SyntaxSpec::sql())));
    }
    if on("ln") {
        tb.set_line_numbers(true);
        tb.set_gutter_marks(true);
    }
    if on("base") {
        tb.set_baseline(Some(&text));
    }
    if on("mm") {
        tb.set_minimap(true);
    }
    if on("occ") {
        tb.set_occurrence_highlight(true);
    }
    if on("br") {
        tb.set_bracket_opts(nexa_ctl::BracketOpts::default());
    }
    println!(
        "features: {}",
        if feats.is_empty() { "(none)" } else { &feats }
    );
    // 워밍업(글리프 캐시).
    paint(&tb, &mut buf);
    let t = Instant::now();
    for _ in 0..30 {
        paint(&tb, &mut buf);
    }
    println!(
        "paint avg: {:.2} ms",
        t.elapsed().as_secs_f64() * 1000.0 / 30.0
    );
    let key = |k| InputEvent::Key {
        key: k,
        shift: false,
        primary: false,
    };
    tb.select_range(0, 0, &mut inv);
    let t = Instant::now();
    for _ in 0..100 {
        tb.on_event(&key(Key::Down), &mut inv);
    }
    println!(
        "Down ×100 avg: {:.3} ms",
        t.elapsed().as_secs_f64() * 1000.0 / 100.0
    );
    let t = Instant::now();
    for _ in 0..50 {
        tb.on_event(&key(Key::End), &mut inv);
        tb.on_event(&key(Key::Home), &mut inv);
    }
    println!(
        "Home/End ×100 avg: {:.3} ms",
        t.elapsed().as_secs_f64() * 1000.0 / 100.0
    );
    let t = Instant::now();
    let s = tb.text();
    let n = s.chars().count();
    println!(
        "text()+count once: {:.3} ms ({n} chars)",
        t.elapsed().as_secs_f64() * 1000.0
    );
    // 페인트 + Down 을 섞은 "키 하나당" 비용(실제 프레임 = on_event + paint).
    let t = Instant::now();
    for _ in 0..20 {
        tb.on_event(&key(Key::Down), &mut inv);
        paint(&tb, &mut buf);
    }
    println!(
        "Down+paint avg: {:.2} ms",
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    );
    // 글자 입력 + 페인트(세대가 바뀌는 프레임 = 캐시를 다시 만드는 비용) — 파일 끝 / 파일 처음.
    for (label, at) in [("end", usize::MAX), ("start", 0usize)] {
        let n = tb.text().chars().count();
        let pos = at.min(n);
        tb.select_range(pos, pos, &mut inv);
        paint(&tb, &mut buf);
        let (mut ev_ms, mut paint_ms) = (0.0f64, 0.0f64);
        let t = Instant::now();
        for _ in 0..20 {
            let t1 = Instant::now();
            tb.on_event(&InputEvent::Char { c: 'x', now_ms: 0 }, &mut inv);
            ev_ms += t1.elapsed().as_secs_f64() * 1000.0;
            let t2 = Instant::now();
            paint(&tb, &mut buf);
            paint_ms += t2.elapsed().as_secs_f64() * 1000.0;
        }
        println!(
            "  on_event {:.2} ms · paint {:.2} ms",
            ev_ms / 20.0,
            paint_ms / 20.0
        );
        println!(
            "type+paint at {label} avg: {:.2} ms",
            t.elapsed().as_secs_f64() * 1000.0 / 20.0
        );
    }
    let (scanned, measured) = tb.paint_work();
    println!(
        "paint work total: hl rows {scanned} · measured rows {measured} · approx {:.1} MB",
        tb.approx_bytes() as f64 / 1048576.0
    );
}
