//! ★ 네트워크 경로 변경 신호(L0 · nexa-sql docs/107 §3-2 · T-313 ⑤ · T-130): VPN 어댑터가 내려가거나 기본 라우트가 바뀌면 OS 콜백으로
//! **깃발 하나**를 세우고 호출자의 `wake`를 부른다 — 주기 핑 없음 · 부하 = 콜백뿐(0). 호출자는 깨어난 뒤 `take_changed()`로 읽고
//! "의심(Suspect)"으로만 바꾼다(판정은 다음 동작이).
//!
//! | OS | 구현 | 비고 |
//! |---|---|---|
//! | Windows | iphlpapi `NotifyIpInterfaceChange`(AF_UNSPEC) + `NotifyRouteChange2` · 취소 `CancelMibChangeNotify2` · 수동 extern | 콜백은 시스템 스레드 |
//! | macOS(195차 · 10-10) | SystemConfiguration 동적 저장소 `SCDynamicStoreSetNotificationKeys`(키 `State:/Network/Global/IPv4`·`IPv6` + 패턴 `State:/Network/Interface/.*/Link`·`/IPv4`) · 전용 스레드 `nexa-netwatch`의 CFRunLoop · 취소 = `CFRunLoopStop` | 콜백은 그 스레드 · 프레임워크 = SystemConfiguration(CLI도 이미 링크) |
//! | Linux(200차 · 10-11) | rtnetlink `AF_NETLINK`/`NETLINK_ROUTE` 소켓 + 멀티캐스트 `RTMGRP_LINK·IPV4/IPV6_IFADDR·IPV4/IPV6_ROUTE` · 전용 스레드 `nexa-netwatch`가 `poll`(소켓 + 중지 파이프) · 수동 extern | 메시지 내용은 보지 않음(어떤 변경이든 신호) · `ENOBUFS` = 큐 넘침도 신호 · 권한 불필요 |
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

/// 콜백에 넘기는 문맥(깃발 + 깨우기) — 등록이 살아 있는 동안 힙에 고정(`Box::into_raw`) · 취소 뒤에 되찾아 떨어뜨린다(Linux는 스레드가 소유 · 불필요).
#[cfg(not(target_os = "linux"))]
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
        unsafe {
            CFStringCreateWithCString(core::ptr::null(), s.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        }
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
            let keys =
                cf_str_array(&[c"State:/Network/Global/IPv4", c"State:/Network/Global/IPv6"]);
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

#[cfg(target_os = "linux")]
mod imp {
    //! Linux — rtnetlink(nexa-sql T-321 · 200차 · 10-11 Linux 전수). 소켓 하나를 링크·주소·라우트 멀티캐스트 묶음에 가입시키고 전용 스레드가
    //! `poll`로 잔다(중지 = 파이프 한 바이트 · fd는 `File`로 감싸 std로 쓰고 닫는다). 메시지는 읽어 버리기만 한다(종류 불문 "의심"으로 충분 · 107 L0). 문맥(깃발 + 깨우기)은
    //! 스레드가 **소유**하므로 Windows·macOS와 달리 원시 포인터가 없다. 외부 crate 0(소켓·poll·pipe2만 수동 extern · 읽기·쓰기·닫기는 std `File`).
    use std::fs::File;
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;

    #[repr(C)]
    struct SockaddrNl {
        nl_family: u16,
        nl_pad: u16,
        nl_pid: u32,
        nl_groups: u32,
    }

    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    extern "C" {
        fn socket(domain: i32, ty: i32, protocol: i32) -> i32;
        fn bind(fd: i32, addr: *const SockaddrNl, len: u32) -> i32;
        fn recv(fd: i32, buf: *mut u8, len: usize, flags: i32) -> isize;
        fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        pub(super) fn pipe2(fds: *mut i32, flags: i32) -> i32;
        fn close(fd: i32) -> i32;
    }

    const AF_NETLINK: i32 = 16;
    const SOCK_RAW: i32 = 3;
    const SOCK_CLOEXEC: i32 = 0o2000000;
    const NETLINK_ROUTE: i32 = 0;
    const RTMGRP_LINK: u32 = 0x1;
    const RTMGRP_IPV4_IFADDR: u32 = 0x10;
    const RTMGRP_IPV4_ROUTE: u32 = 0x40;
    const RTMGRP_IPV6_IFADDR: u32 = 0x100;
    const RTMGRP_IPV6_ROUTE: u32 = 0x400;
    pub(super) const O_CLOEXEC: i32 = 0o2000000;
    const POLLIN: i16 = 0x1;
    const EINTR: i32 = 4;
    const EAGAIN: i32 = 11;
    const ENOBUFS: i32 = 105;

    /// 감시 손잡이 = 중지 파이프의 쓰기 끝 + 스레드. 떨어뜨리면 한 바이트를 써서 깨우고 **기다린다**(스레드가 소켓을 닫는다 · 파이프는 std가 닫는다).
    pub(super) struct Handle {
        stop_wr: File,
        join: Option<JoinHandle<()>>,
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            let _ = self.stop_wr.write_all(&[1u8]);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    /// 중지 파이프(읽기 끝 · 쓰기 끝) — fd를 `File`로 감싸 std가 쓰고 닫는다(`std::io::pipe`는 1.87 · 이 워크스페이스 MSRV 밖).
    pub(super) fn pipe_pair() -> Option<(File, File)> {
        let mut p = [0i32; 2];
        // SAFETY: int[2] 버퍼 · 성공하면 두 fd의 소유권을 `File`이 받는다.
        unsafe {
            if pipe2(p.as_mut_ptr(), O_CLOEXEC) != 0 {
                return None;
            }
            Some((File::from_raw_fd(p[0]), File::from_raw_fd(p[1])))
        }
    }

    /// 스레드 본체: 소켓 또는 중지 파이프가 읽힐 때까지 잔다 · 소켓이 읽히면 비우고 깃발 → 깨우기 · 끝나면 소켓을 닫는다.
    /// (시험은 `UnixDatagram::pair`의 한쪽 fd를 `nl` 자리에 넣어 같은 길을 돈다 — rtnetlink 유니캐스트는 비루트에 `EPERM`이라 실제 소켓으로는 흉내 낼 수 없다.)
    pub(super) fn run(
        nl: i32,
        stop_rd: File,
        flag: Arc<AtomicBool>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) {
        let mut buf = vec![0u8; 8192];
        loop {
            let mut fds = [
                PollFd {
                    fd: nl,
                    events: POLLIN,
                    revents: 0,
                },
                PollFd {
                    fd: stop_rd.as_raw_fd(),
                    events: POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: fd 둘 · 무한 대기 · revents는 커널이 채운다.
            let n = unsafe { poll(fds.as_mut_ptr(), 2, -1) };
            if n < 0 {
                if errno() == EINTR {
                    continue;
                }
                break;
            }
            if fds[1].revents != 0 {
                break; // 중지(또는 쓰기 끝 닫힘).
            }
            if fds[0].revents == 0 {
                continue;
            }
            // SAFETY: 살아 있는 소켓 · 버퍼 길이만큼.
            let r = unsafe { recv(nl, buf.as_mut_ptr(), buf.len(), 0) };
            if r < 0 {
                match errno() {
                    EINTR | EAGAIN => continue,
                    // 커널 큐 넘침 = 신호를 놓쳤다 = 그 자체가 "뭔가 바뀜".
                    ENOBUFS => {}
                    _ => break,
                }
            }
            flag.store(true, Ordering::Release);
            wake();
        }
        // SAFETY: 이 스레드만 쓰는 소켓(파이프 읽기 끝은 `stop_rd`가 여기서 떨어지며 닫힌다).
        unsafe {
            close(nl);
        }
    }

    pub(super) fn start(
        flag: Arc<AtomicBool>,
        wake: Box<dyn Fn() + Send + Sync>,
    ) -> Option<Handle> {
        // SAFETY: 문서의 서명 그대로 · 실패는 전부 `None`(만든 소켓은 닫는다).
        let nl = unsafe {
            let nl = socket(AF_NETLINK, SOCK_RAW | SOCK_CLOEXEC, NETLINK_ROUTE);
            if nl < 0 {
                return None;
            }
            let addr = SockaddrNl {
                nl_family: AF_NETLINK as u16,
                nl_pad: 0,
                nl_pid: 0,
                nl_groups: RTMGRP_LINK
                    | RTMGRP_IPV4_IFADDR
                    | RTMGRP_IPV4_ROUTE
                    | RTMGRP_IPV6_IFADDR
                    | RTMGRP_IPV6_ROUTE,
            };
            if bind(nl, &addr, std::mem::size_of::<SockaddrNl>() as u32) != 0 {
                close(nl);
                return None;
            }
            nl
        };
        let Some((stop_rd, stop_wr)) = pipe_pair() else {
            // SAFETY: 방금 만든 소켓.
            unsafe { close(nl) };
            return None;
        };
        let spawned = std::thread::Builder::new()
            .name("nexa-netwatch".into())
            .spawn(move || run(nl, stop_rd, flag, wake));
        match spawned {
            Ok(join) => Some(Handle {
                stop_wr,
                join: Some(join),
            }),
            Err(_) => {
                // SAFETY: 스레드가 안 떴다 = 소켓은 우리 것.
                unsafe { close(nl) };
                None
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    use super::Ctx;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    pub(super) struct Handle;

    /// 그 밖 OS = 없음 → `None`. 호출자는 `probe.stale_secs`(동작 직전 판정)로 대신한다.
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

    /// Linux: rtnetlink 소켓은 권한이 필요 없다 — 등록 → 첫 깃발 없음 → 떨어뜨리기 = 스레드 종료(join · 1초 안). `AF_NETLINK`를 막은
    /// 샌드박스(일부 컨테이너)에서는 `None`이 계약이라 그때만 건너뛴다.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_registers_and_stops() {
        let Some(w) = NetWatch::start(Box::new(|| {})) else {
            eprintln!("AF_NETLINK 사용 불가(샌드박스) — 건너뜀");
            return;
        };
        assert!(!w.take_changed(), "등록 직후 가짜 신호 없음");
        let t0 = std::time::Instant::now();
        drop(w);
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(1),
            "Drop = 중지 파이프 + join"
        );
    }

    /// Linux: 신호 경로 전체(poll → recv → 깃발 → wake · 중지 파이프 → 종료)를 `UnixDatagram::pair`로 몬다 — 감시 스레드 본체 `run`은
    /// fd가 무엇이든 같은 길이라, rtnetlink 유니캐스트가 비루트에 막혀 있어도(`EPERM`) 흉내 신호를 넣을 수 있다.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_run_loop_signals_flag_and_stops() {
        use std::io::Write;
        use std::os::fd::IntoRawFd;
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let (fake_nl, feeder) = std::os::unix::net::UnixDatagram::pair().expect("pair");
        let (stop_rd, mut stop_wr) = imp::pipe_pair().expect("pipe");
        let f2 = Arc::clone(&flag);
        let fake_fd = fake_nl.into_raw_fd(); // `run`이 끝에서 닫는다.
        let th = std::thread::spawn(move || {
            imp::run(
                fake_fd,
                stop_rd,
                f2,
                Box::new(move || {
                    let _ = tx.send(());
                }),
            );
        });
        // 신호 흉내 = 한 데이터그램(내용 무관).
        feeder.send(&[7u8; 4]).expect("send");
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("깨우기 호출");
        assert!(flag.swap(false, Ordering::AcqRel), "깃발이 섰다");
        // 중지 = 파이프 한 바이트 → 스레드가 끝난다.
        stop_wr.write_all(&[1u8]).expect("stop");
        th.join().expect("스레드 종료");
    }
}
