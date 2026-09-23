//! 구문 강조(09-14 사용자 요청 · Sublime Text 차용) — **데이터 주도 토크나이저** + 강조 규격 파일 + HTML 내보내기.
//!
//! - [`Highlighter`]: 편집기가 줄 단위로 부르는 포트(줄 시작 상태 `u32`를 이어 받는다 — 여러 줄 블록 주석).
//! - [`SyntaxSpec`]: 규격 하나 = 이름 · 확장자 · 키워드 · 줄/블록 주석 · 문자열 구분자. `.nexa-syntax` 텍스트 파일로
//!   기술하며(§파일 형식) 앱은 Sublime 패키지 배치(`Packages/<이름>/*.nexa-syntax`)로 **플러그인 설치**한다.
//!   정규식 기반 `.sublime-syntax` 전체 호환은 정규식 엔진(외부 크레이트 0 정책)이 필요해 후속.
//! - [`to_html`]: 강조 결과를 `<pre><span style>`로 — 서식 있는 복사(PPT 등에 색·굵기 유지).
//!
//! # 파일 형식(`.nexa-syntax`)
//! ```text
//! # 줄 첫 '#' = 주석
//! name = SQL
//! extensions = sql, ddl, dml, pks, pkb
//! case_insensitive = true
//! line_comment = --
//! block_comment = /* */
//! string = ' "
//! escape_backslash = false
//! ident_extra = _$#@
//! keywords = SELECT FROM WHERE ...      (여러 줄 반복 가능 · 공백/쉼표 구분)
//! ```

use crate::theme::{Color, Theme};
use std::collections::HashSet;

/// 토큰 종류 — 테마 색 4종 + 기본.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
}

impl TokenKind {
    /// 테마 색 매핑.
    #[must_use]
    pub fn color(self, th: &Theme) -> Color {
        match self {
            TokenKind::Plain => th.text,
            TokenKind::Keyword => th.syn_keyword,
            TokenKind::Str => th.syn_string,
            TokenKind::Comment => th.syn_comment,
            TokenKind::Number => th.syn_number,
        }
    }
}

/// 줄 단위 강조 포트. `state` = 줄 시작 상태(0 = 없음 · n = n번째 블록 주석 안) — 다음 줄로 이어진다.
/// `out` = (글자 수, 종류) 연속 스팬(줄 전체를 덮는다).
pub trait Highlighter: std::fmt::Debug {
    fn line_spans(&self, line: &str, state: &mut u32, out: &mut Vec<(usize, TokenKind)>);
    fn name(&self) -> &str;
    /// 이 구문에서 문자열 안의 **같은 인용부호 두 번**(`'O''Neil'` · `"a""b"`)이 한 글자 이스케이프인가 — 쌍 표([`super::PairTable`])가
    /// 그 자리를 문자열의 시작/끝으로 잡지 않게(nexa-sql 사용자 09-23 "SQL의 `''`는 `'` 하나를 전달하는 이스케이프 · 쌍 대상에서 제외").
    fn doubled_quote_escapes(&self) -> bool {
        false
    }
    /// 이 구문의 문자열이 줄을 넘는가 — `false`면 쌍 표가 **줄 끝에서 열린 인용부호를 버리고 다시 시작**한다(짝 찾기 fail-over ·
    /// JetBrains 렉서·Sublime 구문의 "문자열은 줄 끝에서 끝난다" 규칙 · nexa-sql 사용자 09-23 "`'` 하나가 전체 짝 찾기를 오염").
    fn strings_span_lines(&self) -> bool {
        true
    }
}

/// 강조 규격(데이터) — [`Highlighter`] 구현.
#[derive(Clone, Debug)]
pub struct SyntaxSpec {
    pub name: String,
    /// 소문자 · 점 없음.
    pub extensions: Vec<String>,
    /// `case_insensitive`면 대문자로 보관.
    pub keywords: HashSet<String>,
    pub case_insensitive: bool,
    pub line_comments: Vec<String>,
    pub block_comments: Vec<(String, String)>,
    /// 문자열 구분자(같은 문자 2개 = 이스케이프).
    pub strings: Vec<char>,
    pub escape_backslash: bool,
    /// 식별자에 허용되는 추가 문자.
    pub ident_extra: String,
    /// 숫자 리터럴을 칠하는가(`.nexa-syntax`의 `numbers = off`로 끔 · Plain Text = 끔 — 읽을거리의 버전 번호·날짜가
    /// 초록으로 칠해지던 것 · nexa-sql 사용자 09-19).
    pub numbers: bool,
}

impl SyntaxSpec {
    /// 강조 없음(Plain Text).
    #[must_use]
    pub fn plain() -> Self {
        SyntaxSpec {
            name: "Plain Text".into(),
            extensions: vec!["txt".into(), "log".into(), "md".into()],
            keywords: HashSet::new(),
            case_insensitive: false,
            line_comments: Vec::new(),
            block_comments: Vec::new(),
            strings: Vec::new(),
            escape_backslash: false,
            ident_extra: "_".into(),
            numbers: false,
        }
    }

    /// 내장 SQL(Oracle·MSSQL·PostgreSQL 공통 키워드).
    #[must_use]
    pub fn sql() -> Self {
        Self::parse(SQL_SPEC).unwrap_or_else(|_| Self::plain())
    }

    /// `.nexa-syntax` 텍스트 파싱.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut s = SyntaxSpec {
            name: String::new(),
            extensions: Vec::new(),
            keywords: HashSet::new(),
            case_insensitive: false,
            line_comments: Vec::new(),
            block_comments: Vec::new(),
            strings: Vec::new(),
            escape_backslash: false,
            ident_extra: "_".into(),
            numbers: true,
        };
        let mut raw_keywords: Vec<String> = Vec::new();
        for (ln, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("line {}: expected key = value", ln + 1));
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "name" => s.name = v.to_string(),
                "extensions" => s.extensions.extend(
                    v.split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|e| !e.is_empty())
                        .map(|e| e.trim_start_matches('.').to_lowercase()),
                ),
                "case_insensitive" => s.case_insensitive = matches!(v, "true" | "on" | "yes" | "1"),
                "escape_backslash" => s.escape_backslash = matches!(v, "true" | "on" | "yes" | "1"),
                "line_comment" => s.line_comments.push(v.to_string()),
                "block_comment" => {
                    let mut it = v.split_whitespace();
                    match (it.next(), it.next()) {
                        (Some(a), Some(b)) => s.block_comments.push((a.to_string(), b.to_string())),
                        _ => {
                            return Err(format!("line {}: block_comment = <open> <close>", ln + 1))
                        }
                    }
                }
                "string" => s
                    .strings
                    .extend(v.split_whitespace().filter_map(|t| t.chars().next())),
                "ident_extra" => s.ident_extra = v.to_string(),
                "numbers" => s.numbers = matches!(v, "true" | "on" | "yes" | "1"),
                "keywords" => raw_keywords.extend(
                    v.split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|e| !e.is_empty())
                        .map(str::to_string),
                ),
                other => return Err(format!("line {}: unknown key '{other}'", ln + 1)),
            }
        }
        if s.name.is_empty() {
            return Err("missing 'name'".into());
        }
        s.keywords = raw_keywords
            .into_iter()
            .map(|k| {
                if s.case_insensitive {
                    k.to_uppercase()
                } else {
                    k
                }
            })
            .collect();
        Ok(s)
    }

    fn is_ident_start(&self, c: char) -> bool {
        c.is_alphabetic() || self.ident_extra.contains(c)
    }

    fn is_ident(&self, c: char) -> bool {
        c.is_alphanumeric() || self.ident_extra.contains(c)
    }

    fn is_keyword(&self, word: &str) -> bool {
        if self.keywords.is_empty() {
            return false;
        }
        if self.case_insensitive {
            self.keywords.contains(&word.to_uppercase())
        } else {
            self.keywords.contains(word)
        }
    }
}

fn starts_with_at(chars: &[char], i: usize, pat: &str) -> bool {
    for (j, pc) in (i..).zip(pat.chars()) {
        if chars.get(j) != Some(&pc) {
            return false;
        }
    }
    true
}

fn push_span(out: &mut Vec<(usize, TokenKind)>, n: usize, k: TokenKind) {
    if n == 0 {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.1 == k {
            last.0 += n;
            return;
        }
    }
    out.push((n, k));
}

impl Highlighter for SyntaxSpec {
    fn name(&self) -> &str {
        &self.name
    }

    /// 규격이 문자열 구분자를 정의하면 두 번 = 이스케이프(`strings` 필드 규약 · SQL `''` · `""`).
    fn doubled_quote_escapes(&self) -> bool {
        !self.strings.is_empty()
    }
    /// 규격의 문자열 토큰은 줄 단위(`line_spans`가 줄마다 새로 시작 · 상태는 블록 주석만 이어진다).
    fn strings_span_lines(&self) -> bool {
        false
    }

    fn line_spans(&self, line: &str, state: &mut u32, out: &mut Vec<(usize, TokenKind)>) {
        let chars: Vec<char> = line.chars().collect();
        let n = chars.len();
        let mut i = 0;
        // 이어지는 블록 주석
        if *state > 0 {
            let idx = (*state - 1) as usize;
            let close = self
                .block_comments
                .get(idx)
                .map(|b| b.1.clone())
                .unwrap_or_default();
            let mut j = i;
            let mut closed = false;
            while j < n {
                if !close.is_empty() && starts_with_at(&chars, j, &close) {
                    j += close.chars().count();
                    closed = true;
                    break;
                }
                j += 1;
            }
            push_span(out, j - i, TokenKind::Comment);
            i = j;
            if closed {
                *state = 0;
            } else {
                return;
            }
        }
        'outer: while i < n {
            let c = chars[i];
            // 줄 주석
            for lc in &self.line_comments {
                if starts_with_at(&chars, i, lc) {
                    push_span(out, n - i, TokenKind::Comment);
                    i = n;
                    continue 'outer;
                }
            }
            // 블록 주석
            for (bi, (open, close)) in self.block_comments.iter().enumerate() {
                if starts_with_at(&chars, i, open) {
                    let mut j = i + open.chars().count();
                    let mut closed = false;
                    while j < n {
                        if starts_with_at(&chars, j, close) {
                            j += close.chars().count();
                            closed = true;
                            break;
                        }
                        j += 1;
                    }
                    push_span(out, j - i, TokenKind::Comment);
                    i = j;
                    if !closed {
                        *state = (bi + 1) as u32;
                        return;
                    }
                    continue 'outer;
                }
            }
            // 문자열
            if self.strings.contains(&c) {
                let q = c;
                let mut j = i + 1;
                while j < n {
                    if self.escape_backslash && chars[j] == '\\' {
                        j += 2;
                        continue;
                    }
                    if chars[j] == q {
                        if chars.get(j + 1) == Some(&q) {
                            j += 2;
                            continue;
                        }
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                let j = j.min(n);
                push_span(out, j - i, TokenKind::Str);
                i = j;
                continue;
            }
            // 숫자
            if self.numbers
                && (c.is_ascii_digit()
                    || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                let mut j = i + 1;
                while j < n
                    && (chars[j].is_ascii_alphanumeric() || chars[j] == '.' || chars[j] == '_')
                {
                    j += 1;
                }
                push_span(out, j - i, TokenKind::Number);
                i = j;
                continue;
            }
            // 식별자/키워드
            if self.is_ident_start(c) {
                let mut j = i + 1;
                while j < n && self.is_ident(chars[j]) {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                let k = if self.is_keyword(&word) {
                    TokenKind::Keyword
                } else {
                    TokenKind::Plain
                };
                push_span(out, j - i, k);
                i = j;
                continue;
            }
            push_span(out, 1, TokenKind::Plain);
            i += 1;
        }
    }
}

fn html_escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn hex(c: Color) -> String {
    format!("#{:06X}", c.0 & 0x00FF_FFFF)
}

/// 강조 결과를 HTML 조각으로(`<pre>` 하나 · 키워드 굵게 · 테마 색) — 서식 있는 복사용.
#[must_use]
pub fn to_html(
    text: &str,
    hl: &dyn Highlighter,
    th: &Theme,
    font_family: &str,
    font_px: i32,
) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    out.push_str(&format!(
        "<pre style=\"font-family:{font_family},monospace;font-size:{font_px}px;color:{};background:{};margin:0;white-space:pre\">",
        hex(th.text),
        hex(th.panel_bg)
    ));
    let mut state = 0u32;
    let mut spans = Vec::new();
    for (li, line) in text.split('\n').enumerate() {
        if li > 0 {
            out.push('\n');
        }
        spans.clear();
        hl.line_spans(line, &mut state, &mut spans);
        let chars: Vec<char> = line.chars().collect();
        let mut ci = 0;
        for (n, k) in &spans {
            let seg: String = chars[ci..(ci + n).min(chars.len())].iter().collect();
            ci += n;
            match k {
                TokenKind::Plain => html_escape(&seg, &mut out),
                TokenKind::Keyword => {
                    out.push_str(&format!(
                        "<span style=\"color:{};font-weight:bold\">",
                        hex(k.color(th))
                    ));
                    html_escape(&seg, &mut out);
                    out.push_str("</span>");
                }
                _ => {
                    out.push_str(&format!("<span style=\"color:{}\">", hex(k.color(th))));
                    html_escape(&seg, &mut out);
                    out.push_str("</span>");
                }
            }
        }
    }
    out.push_str("</pre>");
    out
}

/// 내장 SQL 규격 — 앱은 같은 형식의 파일을 `Packages/`에 두어 확장한다.
pub const SQL_SPEC: &str = r#"
name = SQL
extensions = sql, ddl, dml, pks, pkb, pls, plsql, prc, fnc, trg, vw, tsql, psql
case_insensitive = true
line_comment = --
block_comment = /* */
string = ' "
escape_backslash = false
ident_extra = _$#@
keywords = SELECT FROM WHERE AND OR NOT IN IS NULL LIKE BETWEEN EXISTS AS ON JOIN INNER LEFT RIGHT FULL OUTER CROSS NATURAL USING
keywords = GROUP BY HAVING ORDER ASC DESC LIMIT OFFSET FETCH FIRST NEXT ROWS ONLY TOP DISTINCT ALL UNION INTERSECT EXCEPT MINUS
keywords = INSERT INTO VALUES UPDATE SET DELETE MERGE MATCHED THEN WHEN CASE ELSE END IF ELSIF ELSEIF LOOP WHILE FOR RETURN RETURNING
keywords = CREATE ALTER DROP TABLE VIEW INDEX SEQUENCE SYNONYM TRIGGER PROCEDURE FUNCTION PACKAGE BODY TYPE SCHEMA DATABASE USER ROLE GRANT REVOKE
keywords = PRIMARY KEY FOREIGN REFERENCES UNIQUE CHECK DEFAULT CONSTRAINT CASCADE TRUNCATE RENAME TO ADD COLUMN MODIFY REPLACE TEMPORARY TEMP
keywords = BEGIN DECLARE EXCEPTION RAISE COMMIT ROLLBACK SAVEPOINT TRANSACTION WITH RECURSIVE OVER PARTITION WINDOW ROWNUM ROWID
keywords = CAST CONVERT COALESCE NVL NVL2 DECODE NULLIF IFNULL ISNULL COUNT SUM AVG MIN MAX ROUND TRUNC SUBSTR SUBSTRING INSTR LENGTH LEN TRIM LTRIM RTRIM UPPER LOWER
keywords = TO_CHAR TO_DATE TO_NUMBER SYSDATE SYSTIMESTAMP GETDATE NOW CURRENT_DATE CURRENT_TIMESTAMP EXTRACT DATEADD DATEDIFF
keywords = INT INTEGER BIGINT SMALLINT NUMBER NUMERIC DECIMAL FLOAT REAL DOUBLE PRECISION VARCHAR VARCHAR2 NVARCHAR NVARCHAR2 CHAR NCHAR TEXT CLOB BLOB DATE TIMESTAMP TIME BOOLEAN BOOL BIT SERIAL
keywords = TRUE FALSE EXEC EXECUTE IMMEDIATE CURSOR OPEN CLOSE INTO OUT INOUT ROWTYPE PRAGMA AUTONOMOUS_TRANSACTION GO
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(spec: &SyntaxSpec, line: &str, state: &mut u32) -> Vec<(usize, TokenKind)> {
        let mut out = Vec::new();
        spec.line_spans(line, state, &mut out);
        out
    }

    /// Plain Text = 숫자도 칠하지 않는다 · 규격 파일은 `numbers = off`로 끈다(기본 켬).
    #[test]
    fn plain_text_has_no_number_tokens() {
        let mut st = 0;
        let s = spans(&SyntaxSpec::plain(), "Rainbow Pairs 1.0.0 and 42", &mut st);
        assert!(s.iter().all(|(_, k)| *k == TokenKind::Plain), "{s:?}");
        let off = SyntaxSpec::parse("name = X\nnumbers = off").unwrap();
        let on = SyntaxSpec::parse("name = Y").unwrap();
        assert!(!off.numbers && on.numbers);
    }

    #[test]
    fn sql_keywords_strings_comments() {
        let sql = SyntaxSpec::sql();
        let mut st = 0;
        let s = spans(&sql, "select 'a''b' -- hi", &mut st);
        assert_eq!(s[0], (6, TokenKind::Keyword));
        assert_eq!(s[1], (1, TokenKind::Plain));
        assert_eq!(s[2], (6, TokenKind::Str));
        assert_eq!(s.last().copied(), Some((5, TokenKind::Comment)));
        assert_eq!(st, 0);
    }

    #[test]
    fn block_comment_spans_lines() {
        let sql = SyntaxSpec::sql();
        let mut st = 0;
        let s = spans(&sql, "x /* open", &mut st);
        assert_eq!(st, 1);
        assert_eq!(s.last().copied(), Some((7, TokenKind::Comment)));
        let s = spans(&sql, "still */ 42", &mut st);
        assert_eq!(st, 0);
        assert_eq!(s[0], (8, TokenKind::Comment));
        assert_eq!(s.last().copied(), Some((2, TokenKind::Number)));
    }

    #[test]
    fn spans_cover_whole_line() {
        let sql = SyntaxSpec::sql();
        let line = "SELECT a.b, 1.5e3, \"q\" FROM t WHERE x=1;";
        let mut st = 0;
        let s = spans(&sql, line, &mut st);
        let total: usize = s.iter().map(|x| x.0).sum();
        assert_eq!(total, line.chars().count());
    }

    #[test]
    fn parse_rejects_unknown_key() {
        assert!(SyntaxSpec::parse("name = X\nfoo = 1").is_err());
        assert!(SyntaxSpec::parse("extensions = x").is_err());
        let ok = SyntaxSpec::parse("name = Ini\nextensions = .ini, cfg\nline_comment = ;").unwrap();
        assert_eq!(ok.extensions, vec!["ini", "cfg"]);
    }

    #[test]
    fn html_has_bold_keyword() {
        let sql = SyntaxSpec::sql();
        let h = to_html("select 1 -- c", &sql, &Theme::light(), "Consolas", 14);
        assert!(h.contains("font-weight:bold\">select</span>"));
        let esc = to_html("a<b", &sql, &Theme::light(), "Consolas", 14);
        assert!(esc.contains("a&lt;b"));
    }
}
