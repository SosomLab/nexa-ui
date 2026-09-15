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
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, AtomicIsize, Ordering::Relaxed};
static BYTES: AtomicIsize = AtomicIsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        BYTES.fetch_add(l.size() as isize, Relaxed); COUNT.fetch_add(1, Relaxed); System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        BYTES.fetch_sub(l.size() as isize, Relaxed); System.dealloc(p, l)
    }
}
#[global_allocator] static A: Counting = Counting;

fn snap() -> (isize, usize) { (BYTES.load(Relaxed), COUNT.load(Relaxed)) }

fn main() {
    println!("== 실측 힙 (counting allocator) ==\n");
    for n in [65usize, 1601, 4899] {
        let (b0, c0) = snap();
        let m = model(n);                       // TreeModel.roots 상주분
        let (b1, c1) = snap();
        let rows = flatten(&m);                 // rows() 1회 = 프레임당 사본
        let (b2, c2) = snap();
        println!("{n:>5}행");
        println!("   TreeNode 상주 : {:>9.2} KB  ({:>6} 할당 · 행당 {:>5.0} B)",
                 (b1-b0) as f64/1024.0, c1-c0, (b1-b0) as f64/n as f64);
        println!("   flatten() 사본: {:>9.2} KB  ({:>6} 할당 · 행당 {:>5.0} B)  ← 매 프레임 생성·소멸",
                 (b2-b1) as f64/1024.0, c2-c1, (b2-b1) as f64/n as f64);
        drop(rows); drop(m);
    }
    println!("\n※ 여기에 self.entries(Vec<Entry>) 상주분과 refresh_grid 의 top(Entry 깊은 복사)가 별도로 더해진다.");
}
