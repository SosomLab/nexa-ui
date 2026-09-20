//! 되돌리기·큰 편집 벤치(nexa-sql docs/60 · 사용자 09-19 "undo/redo 설계 전체 점검"):
//! `cargo run --release -p nexa-ctl --example bench_undo [lines]` — N줄 본문에서
//! ① 단어 단위 타이핑의 묶음 기록 비용 ② 붙여넣기(10 KB · 100 KB) ③ 모두 바꾸기 흉내(`replace_range` × K)
//! ④ 되돌리기/다시 실행 ⑤ 히스토리가 쥔 글자 수를 찍는다.
use nexa_ctl::controls::TextBox;
use nexa_ctl::event::InputEvent;
use nexa_ctl::geom::Rect;
use nexa_ctl::widget::{Invalidations, Widget};
use nexa_ctl::Control;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let lines: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40_000);
    let mut text = String::new();
    for i in 0..lines {
        text.push_str(&format!(
            "SELECT COL_{} AS C{i}, 'text {i}' AS K, SYSDATE FROM DUAL WHERE ROWNUM <= {};\n",
            i % 97,
            i % 50
        ));
    }
    let n_chars = text.chars().count();
    println!(
        "text: {} bytes · {lines} lines · {n_chars} chars",
        text.len()
    );
    let mut tb = TextBox::new("").with_multiline();
    let mut inv = Invalidations::default();
    tb.set_bounds(Rect::new(0, 0, 1600, 1000), &mut inv);
    tb.set_focused(true);
    tb.set_text(&text);
    let mid = n_chars / 2;
    tb.select_range(mid, mid, &mut inv);

    // ① 단어 50개 타이핑(단어 = 되돌리기 묶음 하나).
    let t = Instant::now();
    for w in 0..50 {
        for c in format!("word{w} ").chars() {
            tb.on_event(&InputEvent::Char { c, now_ms: 0 }, &mut inv);
        }
    }
    println!(
        "type 50 words: {:.1} ms total · {:.2} ms/word · history {} chars",
        ms(t),
        ms(t) / 50.0,
        tb.history_stats().1
    );

    // ② 붙여넣기.
    for kb in [10usize, 100] {
        let clip: String = "INSERT INTO t VALUES (1, 'x');\n".repeat(kb * 1024 / 31);
        let t = Instant::now();
        tb.paste(&clip, &mut inv);
        println!("paste {kb} KB: {:.1} ms", ms(t));
    }

    // ③ 모두 바꾸기 — 호스트가 쓰는 `replace_many`(한 번 훑기 · 되돌리기 한 단계).
    for k in [200usize, 2000] {
        let chars = tb.text().chars().count();
        let step = chars / (k + 1);
        let before = tb.history_stats().0;
        let t = Instant::now();
        let edits: Vec<(usize, usize, &str)> =
            (1..=k).map(|i| (i * step, i * step + 3, "XYZW")).collect();
        tb.replace_many(&edits, &mut inv);
        println!(
            "replace_many ×{k}: {:.1} ms · undo steps +{} · history {} chars",
            ms(t),
            tb.history_stats().0 - before,
            tb.history_stats().1
        );
    }

    // ④ 되돌리기 / 다시 실행 20회.
    let t = Instant::now();
    for _ in 0..20 {
        tb.on_event(&InputEvent::Undo, &mut inv);
    }
    let undo_ms = ms(t) / 20.0;
    let t = Instant::now();
    for _ in 0..20 {
        tb.on_event(&InputEvent::Redo, &mut inv);
    }
    println!(
        "undo {:.2} ms/step · redo {:.2} ms/step",
        undo_ms,
        ms(t) / 20.0
    );
}
