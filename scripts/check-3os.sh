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

HOST="$(rustc -vV | sed -n 's/^host: //p')"
case "$HOST" in
    *apple-darwin*)   OTHERS=(x86_64-pc-windows-msvc x86_64-unknown-linux-gnu) ;;
    *windows*)        OTHERS=(x86_64-apple-darwin x86_64-unknown-linux-gnu) ;;
    *)                OTHERS=(x86_64-pc-windows-msvc x86_64-apple-darwin) ;;
esac

step "fmt --check"
run cargo fmt --all -- --check
step "clippy 호스트($HOST)"
run cargo clippy --workspace --all-targets -- -D warnings
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
if [[ $FAIL -eq 0 ]]; then echo "★ 3-OS 검사 통과 — push 가능 (push 뒤 gh run watch 로 CI도 확인)"; else echo "★ 실패 $FAIL건 — push 금지"; fi
[[ $FAIL -eq 0 ]]
