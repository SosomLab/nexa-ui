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
}
