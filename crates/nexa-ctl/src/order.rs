//! 순서/표시 모델 — 도구 모음 · 상태줄 · 컬럼처럼 "항목의 순서와 표시 여부"를 설정 한 줄로 들고 다니는 문법(nexa-dir3 `order.rs`의
//! 문법 그대로 · 둘째 사용처 nexa-sql 상태줄 10-04 → 부품): **`블록:vis[자식:vis,…]|블록:vis|…`**(자식 없는 블록 = 대괄호 생략 ·
//! `:0/1` 생략 = 표시). 파싱은 모르는 블록/자식·중복을 버리고 **빠진 것은 정의 순으로 보충**한다(전방 호환 — 새 항목이 저장된
//! 옛 순서에도 정의상 앞 형제 바로 뒤에 들어간다). 순수 함수만 있다(그리기·창 없음).

/// 순서 정의 — `(블록 key, 자식 key 목록)` · 빈 자식 = 단일 블록.
pub type OrderDefs = &'static [(&'static str, &'static [&'static str])];

/// 파싱된 블록 — `(블록 key, 블록 표시, 자식[(key, 표시)])`. 블록 숨김 = 통째 비표시(자식 상태는 보존).
pub type OrderBlock = (String, bool, Vec<(String, bool)>);

/// 기본으로 숨기는 것 `(블록, 자식)` — 자식이 `""`이면 블록 자체. 빈 설정값 · 빠진 항목 보충에 쓴다.
pub type Hidden = &'static [(&'static str, &'static str)];

fn visible_by_default(hidden: Hidden, block: &str, item: &str) -> bool {
    !hidden.iter().any(|(b, i)| *b == block && *i == item)
}

/// `key[:vis]` → (key, 표시 · 생략 = `None`).
fn key_vis(tok: &str) -> (&str, Option<bool>) {
    match tok.split_once(':') {
        Some((k, v)) => (k.trim(), Some(v.trim() != "0")),
        None => (tok.trim(), None),
    }
}

/// 빠진 key를 정의 순으로 보충 — 정의상 앞 형제(이미 들어 있는 것 중 가장 가까운) 바로 뒤 · 없으면 맨 앞.
fn fill_missing<T>(
    out: &mut Vec<T>,
    defs: &[&'static str],
    key_of: impl Fn(&T) -> &str,
    make: impl Fn(&'static str) -> T,
) {
    for (di, d) in defs.iter().enumerate() {
        if out.iter().any(|x| key_of(x) == *d) {
            continue;
        }
        let pos = defs[..di]
            .iter()
            .rev()
            .find_map(|prev| out.iter().position(|x| key_of(x) == *prev).map(|p| p + 1))
            .unwrap_or(0);
        out.insert(pos, make(d));
    }
}

/// 파싱 + 검증(빠진 것 보충 · 모르는 것/중복 제거). 빈 문자열 = 기본.
#[must_use]
pub fn parse(defs: OrderDefs, hidden: Hidden, s: &str) -> Vec<OrderBlock> {
    let mut out: Vec<OrderBlock> = Vec::new();
    for tok in s.split('|') {
        let tok = tok.trim();
        let (head, inner) = match tok.split_once('[') {
            Some((n, rest)) => (n, Some(rest.trim_end_matches(']'))),
            None => (tok, None),
        };
        let (name, bvis) = key_vis(head);
        let Some((bkey, def_items)) = defs.iter().find(|(b, _)| *b == name) else {
            continue;
        };
        if out.iter().any(|(b, _, _)| b == name) {
            continue;
        }
        let mut items: Vec<(String, bool)> = Vec::new();
        for it in inner.into_iter().flat_map(|i| i.split(',')) {
            let (k, vis) = key_vis(it);
            if def_items.contains(&k) && !items.iter().any(|(x, _)| x == k) {
                items.push((k.to_string(), vis.unwrap_or(true)));
            }
        }
        fill_missing(
            &mut items,
            def_items,
            |x| x.0.as_str(),
            |d| (d.to_string(), visible_by_default(hidden, bkey, d)),
        );
        out.push((
            name.to_string(),
            bvis.unwrap_or_else(|| visible_by_default(hidden, bkey, "")),
            items,
        ));
    }
    let block_keys: Vec<&'static str> = defs.iter().map(|(b, _)| *b).collect();
    fill_missing(
        &mut out,
        &block_keys,
        |x| x.0.as_str(),
        |b| {
            let items = defs
                .iter()
                .find(|(k, _)| *k == b)
                .map(|(_, items)| *items)
                .unwrap_or(&[]);
            (
                b.to_string(),
                visible_by_default(hidden, b, ""),
                items
                    .iter()
                    .map(|i| (i.to_string(), visible_by_default(hidden, b, i)))
                    .collect(),
            )
        },
    );
    out
}

/// 직렬화 — 늘 표시 여부(`:0/1`)를 적는다.
#[must_use]
pub fn serialize(order: &[OrderBlock]) -> String {
    order
        .iter()
        .map(|(b, bv, items)| {
            let head = format!("{b}:{}", u8::from(*bv));
            if items.is_empty() {
                head
            } else {
                let inner: Vec<String> = items
                    .iter()
                    .map(|(k, v)| format!("{k}:{}", u8::from(*v)))
                    .collect();
                format!("{head}[{}]", inner.join(","))
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// 기본 순서 문자열(정의 순 · 기본 숨김 반영).
#[must_use]
pub fn default_order(defs: OrderDefs, hidden: Hidden) -> String {
    serialize(&parse(defs, hidden, ""))
}

/// 정규화(파싱 → 직렬화) — 저장 전에 한 번 거친다. 기본과 같으면 빈 문자열(설정 파일에 줄을 남기지 않게).
#[must_use]
pub fn normalize(defs: OrderDefs, hidden: Hidden, s: &str) -> String {
    let n = serialize(&parse(defs, hidden, s));
    if n == default_order(defs, hidden) {
        String::new()
    } else {
        n
    }
}

/// 표시할 블록 key만 순서대로(블록 단위 · 자식은 호출자가 [`parse`]로 본다).
#[must_use]
pub fn visible_blocks(defs: OrderDefs, hidden: Hidden, s: &str) -> Vec<String> {
    parse(defs, hidden, s)
        .into_iter()
        .filter(|(_, v, _)| *v)
        .map(|(b, _, _)| b)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFS: OrderDefs = &[("a", &[]), ("b", &["x", "y"]), ("c", &[]), ("d", &[])];
    const HID: Hidden = &[("c", ""), ("b", "y")];

    #[test]
    fn empty_is_default_with_hidden() {
        assert_eq!(default_order(DEFS, HID), "a:1|b:1[x:1,y:0]|c:0|d:1");
        assert_eq!(normalize(DEFS, HID, ""), "");
        assert_eq!(normalize(DEFS, HID, "a:1|b:1[x:1,y:0]|c:0|d:1"), "");
        assert_eq!(visible_blocks(DEFS, HID, ""), vec!["a", "b", "d"]);
    }

    #[test]
    fn order_and_visibility_round_trip() {
        let s = "d:1|a:0|b:1[y:1,x:0]|c:1";
        assert_eq!(serialize(&parse(DEFS, HID, s)), s);
        assert_eq!(normalize(DEFS, HID, s), s);
        assert_eq!(visible_blocks(DEFS, HID, s), vec!["d", "b", "c"]);
        // 표시 생략 = 블록은 기본 표시 규칙 · 자식은 표시 · 빠진 것(d · x)은 정의상 앞 형제 뒤(없으면 맨 앞).
        assert_eq!(
            serialize(&parse(DEFS, HID, "c|b[y]|a")),
            "c:0|d:1|b:1[x:1,y:1]|a:1"
        );
    }

    #[test]
    fn unknown_and_duplicates_dropped_missing_filled_after_prev_sibling() {
        // 모르는 블록 z · 중복 a · 모르는 자식 q 는 버린다.
        assert_eq!(
            serialize(&parse(DEFS, HID, "z:1|a:1|a:0|b:1[q:1,x:0]")),
            "a:1|b:1[x:0,y:0]|c:0|d:1"
        );
        // 빠진 b는 정의상 앞 형제 a 바로 뒤 · 앞 형제가 없으면 맨 앞.
        assert_eq!(
            serialize(&parse(DEFS, HID, "d:1|a:1")),
            "d:1|a:1|b:1[x:1,y:0]|c:0"
        );
        assert_eq!(
            serialize(&parse(DEFS, HID, "d:1")),
            "a:1|b:1[x:1,y:0]|c:0|d:1"
        );
        // 깨진 문법도 죽지 않는다.
        assert_eq!(normalize(DEFS, HID, "|||[[]]:::"), "");
    }
}
