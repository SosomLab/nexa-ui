//! 목록 보기 모드(FR-U-14 · [DR-12](../../../docs/10-decision-record.md)).
//!
//! 세 모드는 **렌더만 바꾼다** — 선택·키 조작은 동일하다(V-3).
//! ★ 행 높이가 **고정인지 가변인지**가 가상화 전략을 가른다
//! ([docs/20 §3-7](../../../docs/20-implementation-spec.md)).

/// 목록 보기 모드.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum ViewMode {
    /// 일반 — 이미지·서식을 **그대로** 그린다. ★ **행 높이 가변**.
    Rich,
    /// 간략 — 1행 + 작은 썸네일. **기본값**.
    #[default]
    Compact,
    /// 한 줄 — 평문 1줄. 최대 밀도.
    Plain,
}

impl ViewMode {
    /// 전 모드(툴바 세그먼트·설정 콤보 순회용) — **툴바에 보이는 순서 그대로**.
    pub const ALL: [ViewMode; 3] = [ViewMode::Rich, ViewMode::Compact, ViewMode::Plain];

    /// 설정 저장·복원 코드.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            ViewMode::Rich => "rich",
            ViewMode::Compact => "compact",
            ViewMode::Plain => "plain",
        }
    }

    /// 코드 → 모드(미지 코드는 `None` — 호출자가 기본으로 폴백).
    #[must_use]
    pub fn from_code(s: &str) -> Option<ViewMode> {
        match s {
            "rich" => Some(ViewMode::Rich),
            "compact" => Some(ViewMode::Compact),
            "plain" => Some(ViewMode::Plain),
            _ => None,
        }
    }

    /// ★ **행 높이가 가변인가** — `true`면 누적합 + 이진 탐색 가상화가 필요하다.
    #[must_use]
    pub fn is_variable_height(self) -> bool {
        matches!(self, ViewMode::Rich)
    }

    /// 고정 높이 모드의 행 높이(px · 1.0 배율 기준). 가변 모드는 `None`.
    #[must_use]
    pub fn fixed_row_height(self) -> Option<i32> {
        match self {
            ViewMode::Rich => None,
            ViewMode::Compact => Some(34),
            ViewMode::Plain => Some(24),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_compact() {
        assert_eq!(ViewMode::default(), ViewMode::Compact);
    }

    #[test]
    fn code_roundtrip() {
        for m in ViewMode::ALL {
            assert_eq!(ViewMode::from_code(m.code()), Some(m));
        }
        assert_eq!(ViewMode::from_code("nope"), None);
    }

    /// ★ 가변 높이는 일반 보기 하나뿐이다 — 나머지는 O(1) 인덱싱이 가능하다.
    #[test]
    fn only_rich_is_variable() {
        assert!(ViewMode::Rich.is_variable_height());
        assert!(ViewMode::Rich.fixed_row_height().is_none());
        for m in [ViewMode::Compact, ViewMode::Plain] {
            assert!(!m.is_variable_height());
            assert!(m.fixed_row_height().is_some());
        }
    }

    /// 한 줄 보기가 간략 보기보다 조밀해야 한다(세로 밀도 원칙 · DR-14).
    #[test]
    fn plain_is_denser_than_compact() {
        assert!(
            ViewMode::Plain.fixed_row_height() < ViewMode::Compact.fixed_row_height(),
            "한 줄 보기가 더 조밀해야 한다"
        );
    }
}
