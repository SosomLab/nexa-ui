use std::time::Instant;
use std::rc::Rc;

// nexa-ctl tree.rs 와 동일 구조
struct IconImage { _w: u32 }
struct TreeNode { label: String, cells: Vec<String>, children: Vec<TreeNode>,
                  expanded: bool, image: Option<Rc<IconImage>> }
struct FlatRow { path: Vec<usize>, depth: usize, label: String, cells: Vec<String>,
                 has_children: bool, expanded: bool, image: Option<Rc<IconImage>> }

fn walk(nodes: &[TreeNode], path: &mut Vec<usize>, depth: usize, out: &mut Vec<FlatRow>) {
    for (i, n) in nodes.iter().enumerate() {
        path.push(i);
        out.push(FlatRow {
            path: path.clone(), depth, label: n.label.clone(), cells: n.cells.clone(),
            has_children: !n.children.is_empty(), expanded: n.expanded, image: n.image.clone(),
        });
        if n.expanded { walk(&n.children, path, depth + 1, out); }
        path.pop();
    }
}
fn flatten(roots: &[TreeNode]) -> Vec<FlatRow> {
    let mut out = Vec::new(); walk(roots, &mut Vec::new(), 0, &mut out); out
}

// 파일 대화상자 make_row 와 같은 셀 구성: 보이는 3열 + 숨은 3(경로·d/f·확장자)
fn model(n: usize) -> Vec<TreeNode> {
    let icon = Rc::new(IconImage { _w: 16 });
    (0..n).map(|i| TreeNode {
        label: format!("some_file_name_{i:05}.sql"),
        cells: vec![
            "2026-09-15 14:23".into(), "12.4 KB".into(), "SQL 파일".into(),
            format!("D:/Projects/kiros33/nexa-sql/examples/some_file_name_{i:05}.sql"),
            "f".into(), "sql".into(),
        ],
        children: Vec::new(), expanded: false, image: Some(icon.clone()),
    }).collect()
}

fn main() {
    println!("== TreeModel::flatten() — rows() 호출 1회 비용 (캐시 없음) ==");
    for n in [65usize, 500, 1601, 4899, 20000] {
        let m = model(n);
        for _ in 0..5 { std::hint::black_box(flatten(&m)); }
        let iters = if n > 5000 { 50 } else { 300 };
        let t = Instant::now();
        for _ in 0..iters { std::hint::black_box(flatten(&m)); }
        let d = t.elapsed() / iters;
        // 행당 힙 할당: path(1) + label(1) + cells vec(1) + 6 strings = 9
        println!("{n:>6}행: {:>10?} /회   (할당 ≈ {:>7}회, 60fps 예산의 {:>5.1}%)",
                 d, n * 9, d.as_secs_f64() / 0.0167 * 100.0);
    }
    println!("\n== 프레임당 rows() 가 여러 번 불리면 ==");
    let m = model(4899);
    for _ in 0..5 { std::hint::black_box(flatten(&m)); }
    let t = Instant::now();
    for _ in 0..100 { for _ in 0..3 { std::hint::black_box(flatten(&m)); } }
    println!("4899행 × 3회(paint + content_size + hit): {:?}/프레임", t.elapsed()/100);
}
