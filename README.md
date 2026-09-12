# Nexa UI

**nexa 계열(Nexa Clip · Nexa Beep · Nexa SQL …) 공용 UI 라이브러리.**
전부 Rust · 자체 CPU 래스터라이저 · 프레임워크 없음(Qt·WebView·Electron 없음) → 3-OS(Windows · macOS · Linux) 동일 화면.

| 크레이트 | 역할 |
|---|---|
| `nexa-gfx` | CPU 래스터라이저 · 텍스트 스택(`ab_glyph`) |
| `nexa-ctl` | 드로잉 어휘(`DrawCtx`) · 기하 · 입력 이벤트 · 위젯 계약 · 컨트롤 17종 · 디자인 토큰 |
| `nexa-conf` | 설정 직렬화·영속(의존 0) |

사용(path 의존):
```toml
nexa-ctl = { path = "../nexa-ui/crates/nexa-ctl" }
```

설계·진행 기록은 [`docs/`](docs/)에 있다. 계보·결정은 [CLAUDE.md](CLAUDE.md).

## 라이선스

[PolyForm Noncommercial 1.0.0](LICENSE.md) — 비상업적 사용 무료 · 상업적 사용은 별도 라이선스. ([한국어 번역](LICENSE.ko.md))
