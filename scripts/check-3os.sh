#!/usr/bin/env bash
# check-3os.sh — push 전 3타깃 검사(T-40 · 09-05): CI 16회 연속 빨강을 하루 동안 아무도 못 본 뒤의 처방.
#   호스트 + 나머지 두 OS 타깃에 `cargo clippy --all-targets -D warnings`(컴파일만 · 링크 없음) + `cargo fmt --check`.
#   ★ 한 OS에서 `cfg` 모듈 경로를 손대면(예: `super::hotkeys()` — win·sni 1단 · mac::hotkey 2단) 다른 OS가 깨진다 —
#     그 실수를 push 전에 잡는다. 타깃 std는 `rustup target add <타깃>`(없으면 안내하고 그 타깃만 건너뛴다).
# 사용:  scripts/check-3os.sh            # 전부(fmt → 호스트 → 다른 두 타깃)
#        scripts/check-3os.sh --quick    # fmt + 호스트 clippy만
# 시간:  타깃당 수십 초(캐시 적중 시 수 초) — 각 타깃은 target/<triple>/ 에 따로 쌓인다.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
FAIL=0
step() { printf '\n── %s ──\n' "$1"; }
run()  { if "$@"; then echo "  ✓"; else echo "  ✗ 실패"; FAIL=$((FAIL+1)); fi; }

# ★ rustup 툴체인을 쓴다(09-16 mac 실기 · nexa-sql 동일): PATH에 Homebrew Rust가 ~/.cargo/bin보다 앞서면 크로스 타깃 std를 못 찾는다(E0463).
if command -v rustup >/dev/null 2>&1; then
    RUSTUP_BIN="$(dirname "$(rustup which cargo 2>/dev/null || echo "$HOME/.cargo/bin/cargo")")"
    case "$(command -v cargo)" in
        "$RUSTUP_BIN"/*) ;;
        *) echo "⚠ cargo가 rustup 것이 아님($(command -v cargo)) — rustup 툴체인($RUSTUP_BIN)으로 실행"; export PATH="$HOME/.cargo/bin:$PATH" ;;
    esac
fi
HOST="$(rustc -vV | sed -n 's/^host: //p')"
case "$HOST" in
    *apple-darwin*)   OTHERS=(x86_64-pc-windows-msvc x86_64-unknown-linux-gnu) ;;
    *windows*)        OTHERS=(x86_64-apple-darwin x86_64-unknown-linux-gnu) ;;
    *)                OTHERS=(x86_64-pc-windows-msvc x86_64-apple-darwin) ;;
esac

# ★ T-328(nexa-sql 11-10 · 협업 처방): Windows 커밋은 100644로 올라가 맥·리눅스에서 ./ 실행이 안 된다 — 실행 비트는
#   `git update-index --chmod=+x <파일>`로만 반영된다(core.filemode=false).
step "scripts/*.sh 실행 비트(100755)"
NOX="$(git ls-files -s -- 'scripts/*.sh' ':!scripts/lib/*' 2>/dev/null | awk '$1 != "100755" {print $4}')"
if [[ -n "$NOX" ]]; then
    echo "  ✗ 실행 비트 없음(Windows 커밋 = 100644 · 맥·리눅스에서 ./ 실행 불가):"; echo "$NOX" | sed 's/^/    /'
    echo "    → git update-index --chmod=+x <파일>"; FAIL=$((FAIL+1))
else echo "  ✓"; fi

step "fmt --check"
run cargo fmt --all -- --check
step "clippy 호스트($HOST)"
run cargo clippy --workspace --all-targets -- -D warnings
# ★ 더 새 툴체인의 clippy(nexa-sql 11-15): CI는 `stable`이라 이 PC의 기본 rustc보다 새 버전으로 돈다 — 새 버전에서 생긴
#   lint(예: 1.100 needless_range_loop)는 기본 툴체인 검사로는 안 잡혀 push 뒤 CI가 빨개졌다. 설치돼 있는 **기본보다 새**
#   숫자 툴체인(`rustup toolchain install 1.100.0` 같은 것)마다 호스트 clippy를 한 번 더 돈다(산출물 = target/clippy-<버전>).
if command -v rustup >/dev/null 2>&1; then
    CUR="$(rustc -V | awk '{print $2}')"
    for TC in $(rustup toolchain list | sed -n 's/^\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)-.*/\1/p' | sort -uV); do
        if [[ "$TC" != "$CUR" && "$(printf '%s
%s
' "$CUR" "$TC" | sort -V | tail -1)" == "$TC" ]]; then
            step "clippy 호스트 · 툴체인 $TC(기본 $CUR보다 새 버전)"
            run env CARGO_TARGET_DIR="target/clippy-$TC" cargo "+$TC" clippy --workspace --all-targets -- -D warnings
        fi
    done
fi
if [[ "${1:-}" != "--quick" ]]; then
    for T in "${OTHERS[@]}"; do
        if ! rustup target list --installed | grep -q "^$T\$"; then
            echo "  ⚠ 타깃 std 없음: rustup target add $T (이번은 건너뜀)"; continue
        fi
        step "clippy --target $T"
        run cargo clippy --workspace --all-targets --target "$T" -- -D warnings
    done
fi
echo
if [[ $FAIL -eq 0 ]]; then echo "★ 3-OS 검사 통과 — push 가능 (push 뒤 gh run watch 로 CI도 확인)"; else echo "★ 실패 ${FAIL}건 — push 금지"; fi
[[ $FAIL -eq 0 ]]
