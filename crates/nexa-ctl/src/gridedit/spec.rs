//! 셀 명세 — 타입·길이·NULL·기본값과 **검증**(입력 문자열 → 정규형 `Option<String>`).

use super::datetime;

/// 셀 값의 종류(입력 검증·정렬·편집기 동작이 갈린다). DBMS 타입 이름에서 [`CellKind::from_type_name`]으로 추정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CellKind {
    #[default]
    Text,
    Number,
    Bool,
    Date,
    Time,
    DateTime,
    /// 이진(BLOB·BYTEA·RAW …) — 인라인 편집 없음(값 보기 창 몫).
    Binary,
    Other,
}

impl CellKind {
    /// SQL 타입 이름(`VARCHAR2(30)` · `NUMBER(10,2)` · `timestamp without time zone` · `bytea` …)에서 종류를 추정한다.
    /// 호스트가 방언 특성(Oracle `DATE` = 시각 포함)을 알면 덮어쓴다.
    #[must_use]
    pub fn from_type_name(ty: &str) -> CellKind {
        let u = ty.trim().to_ascii_uppercase();
        let head: String = u
            .chars()
            .take_while(|c| c.is_ascii_alphabetic() || *c == ' ' || *c == '_')
            .collect();
        let h = head.trim();
        let has = |s: &str| h.contains(s);
        if has("BOOL") || h == "BIT" {
            return CellKind::Bool;
        }
        if has("BLOB")
            || has("BINARY")
            || has("BYTEA")
            || h == "RAW"
            || h == "LONG RAW"
            || has("IMAGE")
        {
            return CellKind::Binary;
        }
        if has("TIMESTAMP") || has("DATETIME") {
            return CellKind::DateTime;
        }
        if h == "DATE" {
            return CellKind::Date;
        }
        if h.starts_with("TIME") {
            return CellKind::Time;
        }
        if has("CHAR")
            || has("TEXT")
            || has("CLOB")
            || has("STRING")
            || has("XML")
            || has("JSON")
            || has("UUID")
        {
            return CellKind::Text;
        }
        if has("INT")
            || has("NUM")
            || has("DEC")
            || has("FLOAT")
            || has("DOUBLE")
            || has("REAL")
            || has("MONEY")
            || has("SERIAL")
        {
            return CellKind::Number;
        }
        if h.is_empty() {
            return CellKind::Text;
        }
        CellKind::Other
    }

    /// 편집기에서 인라인으로 고칠 수 있는가.
    #[must_use]
    pub fn inline_editable(self) -> bool {
        !matches!(self, CellKind::Binary)
    }
}

/// 열(또는 셀) 하나의 편집 명세.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CellSpec {
    pub name: String,
    pub kind: CellKind,
    /// 문자열 최대 길이(글자 수) — 없으면 제한 없음.
    pub max_len: Option<usize>,
    pub nullable: bool,
    /// 기본값 식(서버 몫 · 새 행에서 비우면 `DEFAULT`로 보낼 근거).
    pub default: Option<String>,
    pub read_only: bool,
}

impl CellSpec {
    #[must_use]
    pub fn new(name: impl Into<String>, kind: CellKind) -> Self {
        CellSpec {
            name: name.into(),
            kind,
            max_len: None,
            nullable: true,
            default: None,
            read_only: false,
        }
    }
    #[must_use]
    pub fn text(name: impl Into<String>) -> Self {
        Self::new(name, CellKind::Text)
    }
    #[must_use]
    pub fn max_len(mut self, n: usize) -> Self {
        self.max_len = Some(n);
        self
    }
    #[must_use]
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }
    #[must_use]
    pub fn default_expr(mut self, d: impl Into<String>) -> Self {
        self.default = Some(d.into());
        self
    }
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// `VARCHAR2(30)` · `NVARCHAR(100)` · `CHAR(8 CHAR)` 같은 타입 이름에서 글자 수 상한을 뽑는다(문자 종류만).
    #[must_use]
    pub fn max_len_from_type_name(ty: &str) -> Option<usize> {
        let u = ty.trim().to_ascii_uppercase();
        if !(u.contains("CHAR") || u.contains("TEXT") || u.contains("STRING")) {
            return None;
        }
        let open = u.find('(')?;
        let close = u[open..].find(')')? + open;
        let inner = &u[open + 1..close];
        let digits: String = inner.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }

    /// 입력 문자열을 검증하고 **정규형**을 돌려준다(`Ok(None)` = NULL).
    /// `input = None`은 NULL 지정 · `Some("")`은 빈 문자열(호스트가 NULL로 바꿔 넘길지는 정책 · `paste::PasteOpts::empty_as_null`).
    pub fn validate(&self, input: Option<&str>) -> Result<Option<String>, EditError> {
        if self.read_only {
            return Err(EditError::ReadOnly);
        }
        let Some(raw) = input else {
            return if self.nullable {
                Ok(None)
            } else {
                Err(EditError::NotNull)
            };
        };
        match self.kind {
            CellKind::Binary => Err(EditError::Binary),
            CellKind::Text | CellKind::Other => {
                let n = raw.chars().count();
                if let Some(max) = self.max_len {
                    if n > max {
                        return Err(EditError::TooLong { len: n, max });
                    }
                }
                Ok(Some(raw.to_string()))
            }
            CellKind::Number => {
                let t: String = raw
                    .trim()
                    .chars()
                    .filter(|c| *c != ',' && *c != '_')
                    .collect();
                if t.is_empty() {
                    return Ok(Some(String::new()));
                }
                if is_number(&t) {
                    Ok(Some(t))
                } else {
                    Err(EditError::NotNumber)
                }
            }
            CellKind::Bool => match raw.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "t" | "y" | "yes" | "on" => Ok(Some("true".into())),
                "0" | "false" | "f" | "n" | "no" | "off" => Ok(Some("false".into())),
                "" => Ok(Some(String::new())),
                _ => Err(EditError::NotBool),
            },
            CellKind::Date | CellKind::Time | CellKind::DateTime => {
                if raw.trim().is_empty() {
                    return Ok(Some(String::new()));
                }
                datetime::parse(self.kind, raw)
                    .map(Some)
                    .ok_or(EditError::NotDateTime {
                        expected: datetime::expected_form(self.kind),
                    })
            }
        }
    }
}

/// 숫자 리터럴인가 — 부호 · 정수부 · 소수점 · 지수(`1e3`) · `.5` · `5.` 허용.
fn is_number(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut ed = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            ed += 1;
        }
        if ed == 0 {
            return false;
        }
    }
    i == b.len()
}

/// 검증 실패 이유(호스트가 자기 i18n으로 옮긴다 · [`EditError::message`]는 영어 기본문).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    ReadOnly,
    NotNull,
    TooLong { len: usize, max: usize },
    NotNumber,
    NotBool,
    NotDateTime { expected: &'static str },
    Binary,
}

impl EditError {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            EditError::ReadOnly => "read-only column".into(),
            EditError::NotNull => "NULL not allowed (NOT NULL)".into(),
            EditError::TooLong { len, max } => format!("too long: {len}/{max} characters"),
            EditError::NotNumber => "not a number".into(),
            EditError::NotBool => "not a boolean (true/false, 1/0, Y/N)".into(),
            EditError::NotDateTime { expected } => format!("not a date/time (e.g. {expected})"),
            EditError::Binary => "binary value: use the value viewer".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_from_type_names() {
        assert_eq!(CellKind::from_type_name("VARCHAR2(30)"), CellKind::Text);
        assert_eq!(CellKind::from_type_name("NUMBER(10,2)"), CellKind::Number);
        assert_eq!(CellKind::from_type_name("int4"), CellKind::Number);
        assert_eq!(CellKind::from_type_name("bigserial"), CellKind::Number);
        assert_eq!(CellKind::from_type_name("DATE"), CellKind::Date);
        assert_eq!(CellKind::from_type_name("TIMESTAMP(6)"), CellKind::DateTime);
        assert_eq!(
            CellKind::from_type_name("timestamp without time zone"),
            CellKind::DateTime
        );
        assert_eq!(CellKind::from_type_name("datetime2"), CellKind::DateTime);
        assert_eq!(CellKind::from_type_name("time"), CellKind::Time);
        assert_eq!(CellKind::from_type_name("BLOB"), CellKind::Binary);
        assert_eq!(CellKind::from_type_name("varbinary(max)"), CellKind::Binary);
        assert_eq!(CellKind::from_type_name("bytea"), CellKind::Binary);
        assert_eq!(CellKind::from_type_name("bit"), CellKind::Bool);
        assert_eq!(CellKind::from_type_name("boolean"), CellKind::Bool);
        assert_eq!(CellKind::from_type_name("CLOB"), CellKind::Text);
        assert_eq!(CellKind::from_type_name("nvarchar(max)"), CellKind::Text);
        assert_eq!(CellKind::from_type_name("geometry"), CellKind::Other);
        assert_eq!(CellSpec::max_len_from_type_name("VARCHAR2(30)"), Some(30));
        assert_eq!(CellSpec::max_len_from_type_name("CHAR(8 CHAR)"), Some(8));
        assert_eq!(CellSpec::max_len_from_type_name("NUMBER(10,2)"), None);
        assert_eq!(CellSpec::max_len_from_type_name("nvarchar(max)"), None);
    }

    #[test]
    fn validate_text_length_and_null() {
        let s = CellSpec::text("n").max_len(3);
        assert_eq!(s.validate(Some("abc")), Ok(Some("abc".into())));
        assert_eq!(s.validate(Some("한글셋")), Ok(Some("한글셋".into())));
        assert_eq!(
            s.validate(Some("abcd")),
            Err(EditError::TooLong { len: 4, max: 3 })
        );
        assert_eq!(s.validate(None), Ok(None));
        let nn = CellSpec::text("n").not_null();
        assert_eq!(nn.validate(None), Err(EditError::NotNull));
        assert_eq!(
            CellSpec::text("r").read_only().validate(Some("x")),
            Err(EditError::ReadOnly)
        );
    }

    #[test]
    fn validate_number_bool_binary() {
        let n = CellSpec::new("n", CellKind::Number);
        assert_eq!(n.validate(Some(" 1,234.5 ")), Ok(Some("1234.5".into())));
        assert_eq!(n.validate(Some("-0.5e3")), Ok(Some("-0.5e3".into())));
        assert_eq!(n.validate(Some(".5")), Ok(Some(".5".into())));
        assert_eq!(n.validate(Some("12a")), Err(EditError::NotNumber));
        assert_eq!(n.validate(Some("1e")), Err(EditError::NotNumber));
        let b = CellSpec::new("b", CellKind::Bool);
        assert_eq!(b.validate(Some("Y")), Ok(Some("true".into())));
        assert_eq!(b.validate(Some("0")), Ok(Some("false".into())));
        assert_eq!(b.validate(Some("maybe")), Err(EditError::NotBool));
        assert_eq!(
            CellSpec::new("x", CellKind::Binary).validate(Some("00")),
            Err(EditError::Binary)
        );
    }

    #[test]
    fn validate_datetime_kinds() {
        let d = CellSpec::new("d", CellKind::DateTime);
        assert_eq!(
            d.validate(Some("2026/9/26 14:05")),
            Ok(Some("2026-09-26 14:05:00".into()))
        );
        assert_eq!(d.validate(Some("today")), Ok(Some("today".into())));
        assert!(matches!(
            d.validate(Some("26th")),
            Err(EditError::NotDateTime { .. })
        ));
    }
}
