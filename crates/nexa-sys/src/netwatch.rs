//! ★ 네트워크 경로 변경 신호(L0 · nexa-sql docs/107 §3-2 · T-313 ⑤ · T-130): VPN 어댑터가 내려가거나 기본 라우트가 바뀌면 OS 콜백으로
//! **깃발 하나**를 세우고 호출자의 `wake`를 부른다 — 주기 핑 없음 · 부하 = 콜백뿐(0). 호출자는 깨어난 뒤 `take_changed()`로 읽고
//! "의심(Suspect)"으로만 바꾼다(판정은 다음 동작이).
//!
//! | OS | 구현 | 비고 |
//! |---|---|---|
//! | Windows | iphlpapi `NotifyIpInterfaceChange`(AF_UNSPEC) + `NotifyRouteChange2` · 취소 `CancelMibChangeNotify2` · 수동 extern | 콜백은 시스템 스레드 |
//! | macOS · Linux | 아직 없음 → `None`(호출자는 `probe.stale_secs`로 대신) | T-130 후속(SCNetworkReachability · netlink RTMGRP_LINK) |
//!
//! 규칙(크레이트 머리): 외부 crate 0 · 실패 = `None` · 프로세스 생성 0.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 감시 손잡이 — 떨어뜨리면(Drop) 등록을 푼다.
pub struct NetWatch {
    flag: Arc<AtomicBool>,
    _imp: imp::Handle,
}

impl std::fmt::Debug for NetWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetWatch")
            .field("changed", &self.flag.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl NetWatch {
    /// 감시 시작. 신호마다 깃발을 세우고 `wake`를 부른다(여러 신호는 깃발 하나로 합쳐진다). 미지원 OS · 등록 실패 = `None`.
    #[must_use]
    pub fn start(wake: Box<dyn Fn() + Send + Sync>) -> Option<NetWatch> {
        let flag = Arc::new(AtomicBool::new(false));
        let h = imp::start(Arc::clone(&flag), wake)?;
        Some(NetWatch { flag, _imp: h })
    }

    /// 깃발을 내리며 읽는다(마지막 읽기 뒤 신호가 있었는가).
    #[must_use]
    pub fn take_changed(&self) -> bool {
        self.flag.swap(false, Ordering::AcqRel)
    }

    /// 시험·진단용: 신호를 흉내 낸다(깃발만 · `wake`는 부르지 않는다).
    pub fn simulate(&self) {
        self.flag.store(true, Ordering::Release);
    }
}

/// 콜백에 넘기는 문맥(깃발 + 깨우기) — 등록이 살아 있는 동안 힙에 고정(`Box::into_raw`) · 취소 뒤에 되찾아 떨어뜨린다.
type Ctx = (Arc<AtomicBool>, Box<dyn Fn() + Send + Sync>);

#[cfg(windows)]
mod imp {
    use super::Ctx;
    use core::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    type IfCallback = unsafe extern "system" fn(*const c_void, *const c_void, i32);

    #[link(name = "iphlpapi")]
    extern "system" {
        fn NotifyIpInterfaceChange(
            family: u16,
            callback: IfCallback,
            context: *const c_void,
            initial: u8,
            handle: *mut *mut c_void,
        ) -> u32;
        fn NotifyRouteChange2(
            family: u16,
            callback: IfCallback,
            context: *const c_void,
            initial: u8,
            handle: *mut *mut c_void,
        ) -> u32;
        fn CancelMibChangeNotify2(handle: *mut c_void) -> u32;
    }

    const AF_UNSPEC: u16 = 0;
    const NO_ERROR: u32 = 0;

    pub(super) struct Handle {
        h_if: *mut c_void,
        h_route: *mut c_void,
        ctx: *mut Ctx,
    }

    // SAFETY: 손잡이는 OS 등록 번호와 힙 문맥 포인터뿐 — 어느 스레드에서 취소해도 되고(문서) 문맥은 취소 뒤에만 떨어뜨린다.
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    unsafe extern "system" fn on_change(ctx: *const c_void, _row: *const c_void, _kind: i32) {
        // 행 내용은 보지 않는다(어떤 변경이든 "의심"으로 충분 · 107 L0). 깃발 → 깨우기.
        let p = ctx as *const Ctx;
        // SAFETY: `start`가 `Box::into_raw`로 고정한 문맥이고 취소(`CancelMibChangeNotify2` · 동기) 뒤에만 해제한다.
        if let Some((flag, wake)) = unsafe { p.as_ref() } {
            flag.store(true, Ordering::Release);
            wake();
        }
    }

    pub(super) fn start(
        flag: Arc<AtomicBool>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Option<Handle> {
        let ctx: *mut Ctx = Box::into_raw(Box::new((flag, wake)));
        let mut h_if: *mut c_void = core::ptr::null_mut();
        let mut h_route: *mut c_void = core::ptr::null_mut();
        // SAFETY: 문서의 서명 그대로 · 콜백·문맥 포인터는 등록이 살아 있는 동안 유효 · `initial = 0`(등록 직후 가짜 신호 없음).
        let r1 = unsafe { NotifyIpInterfaceChange(AF_UNSPEC, on_change, ctx.cast(), 0, &mut h_if) };
        if r1 != NO_ERROR {
            // SAFETY: 등록 실패 = 콜백이 문맥을 쓰지 않는다 → 되찾아 떨어뜨린다.
            drop(unsafe { Box::from_raw(ctx) });
            return None;
        }
        // 라우트 변경(VPN 기본 라우트 추가/제거)은 보조 — 실패해도 인터페이스 신호만으로 간다.
        let r2 = unsafe { NotifyRouteChange2(AF_UNSPEC, on_change, ctx.cast(), 0, &mut h_route) };
        if r2 != NO_ERROR {
            h_route = core::ptr::null_mut();
        }
        Some(Handle { h_if, h_route, ctx })
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: 취소는 동기(돌아오면 콜백이 더는 돌지 않는다) → 그 뒤 문맥 해제.
            unsafe {
                if !self.h_route.is_null() {
                    let _ = CancelMibChangeNotify2(self.h_route);
                }
                if !self.h_if.is_null() {
                    let _ = CancelMibChangeNotify2(self.h_if);
                }
                drop(Box::from_raw(self.ctx));
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Ctx;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    pub(super) struct Handle;

    /// macOS · Linux = 아직 없음(T-130 후속) → `None`. 호출자는 `probe.stale_secs`(동작 직전 판정)로 대신한다.
    pub(super) fn start(
        _flag: Arc<AtomicBool>,
        _wake: Box<dyn Fn() + Send + Sync>,
    ) -> Option<Handle> {
        let _: Option<Ctx> = None;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 깃발 = 읽으면 내려간다 · 흉내 신호로 다시 선다(OS 등록 없이 — Windows에서는 실제 등록을 한 번 시도해 손잡이 생성·해제도 지난다).
    #[test]
    fn flag_take_and_simulate() {
        let Some(w) = NetWatch::start(Box::new(|| {})) else {
            return; // 미지원 OS(또는 권한 없는 환경) = None 자체가 계약.
        };
        assert!(!w.take_changed());
        w.simulate();
        assert!(w.take_changed());
        assert!(!w.take_changed());
        drop(w);
    }
}
