//! ★ 네트워크 경로 변경 신호(L0 · nexa-sql docs/107 §3-2 · T-313 ⑤ · T-130): VPN 어댑터가 내려가거나 기본 라우트가 바뀌면 OS 콜백으로
//! **깃발 하나**를 세우고 호출자의 `wake`를 부른다 — 주기 핑 없음 · 부하 = 콜백뿐(0). 호출자는 깨어난 뒤 `take_changed()`로 읽고
//! "의심(Suspect)"으로만 바꾼다(판정은 다음 동작이).
//!
//! | OS | 구현 | 비고 |
//! |---|---|---|
//! | Windows | iphlpapi `NotifyIpInterfaceChange`(AF_UNSPEC) + `NotifyRouteChange2` · 취소 `CancelMibChangeNotify2` · 수동 extern | 콜백은 시스템 스레드 |
//! | macOS(195차 · 10-10) | SystemConfiguration 동적 저장소 `SCDynamicStoreSetNotificationKeys`(키 `State:/Network/Global/IPv4`·`IPv6` + 패턴 `State:/Network/Interface/.*/Link`·`/IPv4`) · 전용 스레드 `nexa-netwatch`의 CFRunLoop · 취소 = `CFRunLoopStop` | 콜백은 그 스레드 · 프레임워크 = SystemConfiguration(CLI도 이미 링크) |
//! | Linux | 아직 없음 → `None`(호출자는 `probe.stale_secs`로 대신) | 후속(netlink RTMGRP_LINK) |
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

#[cfg(target_os = "macos")]
mod imp {
    //! macOS — SystemConfiguration 동적 저장소 알림. 등록·대기는 전용 스레드의 CFRunLoop에서(메인 스레드 런루프에 끼어들지 않는다 ·
    //! winit이 메인 런루프를 쥔다). 신호 = 전역 IPv4/IPv6 상태(기본 라우트 · DNS) · 인터페이스 Link/IPv4(VPN utun 올라옴/내려감).
    use super::Ctx;
    use core::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    type CfRef = *const c_void;
    type CfIndex = isize;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[repr(C)]
    struct StoreContext {
        version: CfIndex,
        info: *mut c_void,
        retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
        release: Option<unsafe extern "C" fn(*const c_void)>,
        copy_description: Option<unsafe extern "C" fn(*const c_void) -> CfRef>,
    }

    type Callout = unsafe extern "C" fn(store: CfRef, changed_keys: CfRef, info: *mut c_void);

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: CfRef;
        static kCFTypeArrayCallBacks: c_void;
        fn CFStringCreateWithCString(alloc: CfRef, cstr: *const c_char, encoding: u32) -> CfRef;
        fn CFArrayCreate(
            alloc: CfRef,
            values: *const CfRef,
            n: CfIndex,
            callbacks: *const c_void,
        ) -> CfRef;
        fn CFRelease(cf: CfRef);
        fn CFRetain(cf: CfRef) -> CfRef;
        fn CFRunLoopGetCurrent() -> CfRef;
        fn CFRunLoopAddSource(rl: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRemoveSource(rl: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRun();
        fn CFRunLoopStop(rl: CfRef);
    }

    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCDynamicStoreCreate(
            alloc: CfRef,
            name: CfRef,
            callout: Option<Callout>,
            context: *mut StoreContext,
        ) -> CfRef;
        fn SCDynamicStoreSetNotificationKeys(store: CfRef, keys: CfRef, patterns: CfRef) -> u8;
        fn SCDynamicStoreCreateRunLoopSource(alloc: CfRef, store: CfRef, order: CfIndex) -> CfRef;
    }

    /// 감시 손잡이 = 그 스레드의 런루프 참조(`CFRetain` 한 번) · 떨어뜨리면 멈추라고 하고 스레드가 뒷정리를 한다.
    pub(super) struct Handle {
        run_loop: usize,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: `CFRunLoopStop`·`CFRelease`는 어느 스레드에서 불러도 된다(CFRunLoop은 스레드 안전) · 스레드가 `CFRunLoopRun`에서
            //         돌아와 소스·저장소·문맥을 정리한다. 우리가 쥔 참조 하나를 놓는다.
            unsafe {
                let rl = self.run_loop as CfRef;
                CFRunLoopStop(rl);
                CFRelease(rl);
            }
        }
    }

    unsafe extern "C" fn on_change(_store: CfRef, _changed_keys: CfRef, info: *mut c_void) {
        // 어떤 키가 바뀌었는지는 보지 않는다(어떤 변경이든 "의심"으로 충분 · 107 L0). 깃발 → 깨우기.
        let p = info as *const Ctx;
        // SAFETY: `run`이 `Box::into_raw`로 고정한 문맥 · 런루프가 멈춘 뒤(콜백이 더는 돌지 않는 스레드 안)에서만 해제한다.
        if let Some((flag, wake)) = unsafe { p.as_ref() } {
            flag.store(true, Ordering::Release);
            wake();
        }
    }

    fn cf_str(s: &'static std::ffi::CStr) -> CfRef {
        // SAFETY: NUL로 끝나는 정적 C 문자열.
        unsafe { CFStringCreateWithCString(core::ptr::null(), s.as_ptr(), K_CF_STRING_ENCODING_UTF8) }
    }

    /// 문자열 배열(`CFArray<CFString>`) — 만든 뒤 원소 참조는 배열이 쥔다(우리 +1은 놓는다).
    fn cf_str_array(items: &[&'static std::ffi::CStr]) -> CfRef {
        let vals: Vec<CfRef> = items.iter().map(|s| cf_str(s)).collect();
        // SAFETY: 값 포인터 배열과 길이 · 타입 콜백(retain/release) = 배열이 원소를 쥔다.
        let arr = unsafe {
            CFArrayCreate(
                core::ptr::null(),
                vals.as_ptr(),
                vals.len() as CfIndex,
                core::ptr::addr_of!(kCFTypeArrayCallBacks),
            )
        };
        for v in vals {
            if !v.is_null() {
                // SAFETY: 배열이 retain했으니 우리 참조를 놓는다(만들기 실패면 그냥 놓는다).
                unsafe { CFRelease(v) };
            }
        }
        arr
    }

    /// 스레드 본체: 등록 → 런루프 참조를 알려 주고 → `CFRunLoopRun`(멈춤 요청까지) → 정리.
    fn run(ctx: *mut Ctx, ready: std::sync::mpsc::Sender<Option<usize>>) {
        // SAFETY: 아래 전부 문서의 서명 그대로 · 실패는 `None`을 알리고 돌아간다(그 전에 만든 것은 놓는다) · 문맥은 이 함수 끝에서 해제.
        unsafe {
            let name = cf_str(c"nexa-netwatch");
            let mut cx = StoreContext {
                version: 0,
                info: ctx.cast(),
                retain: None,
                release: None,
                copy_description: None,
            };
            let store = SCDynamicStoreCreate(core::ptr::null(), name, Some(on_change), &mut cx);
            if !name.is_null() {
                CFRelease(name);
            }
            if store.is_null() {
                let _ = ready.send(None);
                drop(Box::from_raw(ctx));
                return;
            }
            let keys = cf_str_array(&[c"State:/Network/Global/IPv4", c"State:/Network/Global/IPv6"]);
            let patterns = cf_str_array(&[
                c"State:/Network/Interface/.*/Link",
                c"State:/Network/Interface/.*/IPv4",
            ]);
            let ok = !keys.is_null()
                && !patterns.is_null()
                && SCDynamicStoreSetNotificationKeys(store, keys, patterns) != 0;
            if !keys.is_null() {
                CFRelease(keys);
            }
            if !patterns.is_null() {
                CFRelease(patterns);
            }
            let source = if ok {
                SCDynamicStoreCreateRunLoopSource(core::ptr::null(), store, 0)
            } else {
                core::ptr::null()
            };
            if source.is_null() {
                CFRelease(store);
                let _ = ready.send(None);
                drop(Box::from_raw(ctx));
                return;
            }
            let rl = CFRunLoopGetCurrent();
            CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
            // 손잡이가 쥘 참조(+1) — `ready`가 끊겨 있으면(호출자가 포기) 바로 정리한다.
            let rl_held = CFRetain(rl);
            if ready.send(Some(rl_held as usize)).is_err() {
                CFRelease(rl_held);
            } else {
                // 멈춤 요청(`CFRunLoopStop`)까지 잔다 — 소스가 하나 있으니 바로 돌아오지 않는다.
                CFRunLoopRun();
            }
            CFRunLoopRemoveSource(rl, source, kCFRunLoopDefaultMode);
            CFRelease(source);
            CFRelease(store);
            drop(Box::from_raw(ctx));
        }
    }

    pub(super) fn start(
        flag: Arc<AtomicBool>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Option<Handle> {
        let ctx: *mut Ctx = Box::into_raw(Box::new((flag, wake)));
        let ctx_u = ctx as usize;
        let (tx, rx) = std::sync::mpsc::channel::<Option<usize>>();
        let spawned = std::thread::Builder::new()
            .name("nexa-netwatch".into())
            .spawn(move || run(ctx_u as *mut Ctx, tx));
        if spawned.is_err() {
            // SAFETY: 스레드가 안 떴다 = 아무도 문맥을 보지 않는다 → 되찾아 떨어뜨린다.
            drop(unsafe { Box::from_raw(ctx) });
            return None;
        }
        // 스레드는 등록 결과를 `CFRunLoopRun` 전에 꼭 보낸다(실패도) — 기다리는 시간은 등록 비용(ms)뿐.
        let run_loop = rx.recv().ok().flatten()?;
        Some(Handle { run_loop })
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::Ctx;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    pub(super) struct Handle;

    /// Linux = 아직 없음(netlink 후속) → `None`. 호출자는 `probe.stale_secs`(동작 직전 판정)로 대신한다.
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

    /// 깃발 = 읽으면 내려간다 · 흉내 신호로 다시 선다(Windows·macOS에서는 실제 등록을 한 번 시도해 손잡이 생성·해제(스레드 종료)도 지난다).
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

    /// macOS: 등록은 반드시 된다(SystemConfiguration은 권한이 필요 없다) — 손잡이 생성 → 떨어뜨리기 = 스레드 종료(멈추지 않으면 시험이 끝나지 않는다).
    #[cfg(target_os = "macos")]
    #[test]
    fn mac_registers_and_stops() {
        let w = NetWatch::start(Box::new(|| {})).expect("SCDynamicStore 등록");
        assert!(!w.take_changed());
        drop(w);
    }
}
