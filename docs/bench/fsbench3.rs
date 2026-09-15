use std::fs;
use std::path::Path;
use std::time::Instant;

// nexa-fs::list_opts 의 메타데이터 획득 방식 (경로 기반 · 링크 추적)
fn list_pathmeta(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(dir) {
        for de in rd.flatten() {
            let path = de.path();
            let _m = fs::metadata(&path).or_else(|_| de.metadata());
            n += 1;
        }
    }
    n
}

// 대안: DirEntry 캐시 메타 우선 · 링크일 때만 경로 stat
fn list_dirmeta(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(dir) {
        for de in rd.flatten() {
            let ft = de.file_type();
            let _m = match &ft {
                Ok(t) if t.is_symlink() => fs::metadata(de.path()).or_else(|_| de.metadata()),
                _ => de.metadata(),
            };
            n += 1;
        }
    }
    n
}

// nexa-fs::has_visible_child — 필터 있음(최악: 일치 없음 → 전량 스캔)
fn has_visible_child(dir: &Path, exts: &[String]) -> (bool, usize) {
    let mut seen = 0;
    let Ok(rd) = fs::read_dir(dir) else { return (true, 0) };
    for de in rd.flatten() {
        seen += 1;
        let name = de.file_name().to_string_lossy().into_owned();
        let meta = fs::metadata(de.path()).or_else(|_| de.metadata());
        let is_dir = match &meta {
            Ok(m) => m.is_dir(),
            Err(_) => de.file_type().map(|t| t.is_dir()).unwrap_or(false),
        };
        if is_dir { return (true, seen); }
        if exts.is_empty() { return (true, seen); }
        let ext = Path::new(&name).extension()
            .map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if exts.iter().any(|x| *x == ext) { return (true, seen); }
    }
    (false, seen)
}

fn bench(label: &str, dir: &Path, exts: &[String]) {
    if !dir.is_dir() { println!("{label}: (없음) {}", dir.display()); return; }
    let t = Instant::now(); let n1 = list_pathmeta(dir); let d1 = t.elapsed();
    let t = Instant::now(); let n2 = list_dirmeta(dir);  let d2 = t.elapsed();
    println!("\n[{label}] {}  ({n1}개)", dir.display());
    println!("  list: fs::metadata(path) {:>10?}   |  DirEntry::metadata {:>10?}   → {:.1}배",
             d1, d2, d1.as_secs_f64() / d2.as_secs_f64().max(1e-9));

    // 하위 폴더마다 has_visible_child (make_row 가 행마다 호출)
    let subs: Vec<_> = fs::read_dir(dir).map(|rd| rd.flatten()
        .filter(|e| e.path().is_dir()).map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default();
    let t = Instant::now();
    let mut scanned = 0usize;
    for s in &subs { let (_, sc) = has_visible_child(s, exts); scanned += sc; }
    let d = t.elapsed();
    println!("  has_visible_child × {} 하위폴더 (필터 {:?}): {:?}  (검사 항목 {} 개)",
             subs.len(), if exts.is_empty() {"없음"} else {"*.sql"}, d, scanned);
    println!("  → 이 폴더 1회 진입 총 비용 ≈ {:?}", d1 + d);
    let _ = n2;
}
fn fair(label: &str, dir: &Path) {
    if !dir.is_dir() { return; }
    // 양쪽 모두 2회 워밍 후 측정 (순서 편향 제거: dirmeta 먼저)
    for _ in 0..2 { list_dirmeta(dir); list_pathmeta(dir); }
    let t = Instant::now(); let n = list_dirmeta(dir);  let d2 = t.elapsed();
    let t = Instant::now(); let _ = list_pathmeta(dir); let d1 = t.elapsed();
    // 한 번 더 역순으로
    let t = Instant::now(); let _ = list_pathmeta(dir); let e1 = t.elapsed();
    let t = Instant::now(); let _ = list_dirmeta(dir);  let e2 = t.elapsed();
    let p = (d1 + e1) / 2; let q = (d2 + e2) / 2;
    println!("{label:<28} {n:>6}개  path-meta {:>10?}  entry-meta {:>10?}  → {:.1}배",
             p, q, p.as_secs_f64() / q.as_secs_f64().max(1e-9));
}

fn main() {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:/Users".into());
    println!("== 워밍 후 · 양방향 평균 (OS 캐시 hot) ==");
    fair("홈", Path::new(&home));
    fair("System32", Path::new("C:/Windows/System32"));
    fair("Windows", Path::new("C:/Windows"));
    fair("target/debug/deps", Path::new("D:/Projects/kiros33/nexa-sql/target/debug/deps"));
    fair("target", Path::new("D:/Projects/kiros33/nexa-sql/target"));
}
