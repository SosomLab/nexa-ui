//! 컬럼 모델 — "컬럼은 데이터"(원본 docs/23 §1). 위젯은 컬럼 의미를 모르고
//! `key`(불투명 식별자)로 [`crate::widgets::RowSource`]에 셀 값·정렬을 위임한다.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Right,
}

/// 컬럼 정의. `key`는 RowSource가 해석하는 불투명 식별자(0 = 트리 컬럼 관례).
#[derive(Clone, Debug)]
pub struct Column {
    pub key: u32,
    pub title: String,
    /// 현재 폭(px). 드래그 리사이즈로 변경.
    pub width: i32,
    pub min_width: i32,
    pub align: Align,
    pub sortable: bool,
    pub resizable: bool,
}

impl Column {
    pub fn new(key: u32, title: impl Into<String>, width: i32) -> Column {
        Column {
            key,
            title: title.into(),
            width,
            min_width: 40,
            align: Align::Left,
            sortable: true,
            resizable: true,
        }
    }

    pub fn right_aligned(mut self) -> Column {
        self.align = Align::Right;
        self
    }
}

/// 정렬 상태 표시용 원문자(다중 정렬 순번 — 원본 docs/23 §4 "컬럼명 뒤 원문자").
pub fn order_badge(order: usize) -> &'static str {
    const BADGES: [&str; 9] = ["①", "②", "③", "④", "⑤", "⑥", "⑦", "⑧", "⑨"];
    BADGES.get(order).copied().unwrap_or("⑨")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_covers_first_nine() {
        assert_eq!(order_badge(0), "①");
        assert_eq!(order_badge(8), "⑨");
        assert_eq!(order_badge(99), "⑨"); // 초과는 포화
    }
}
