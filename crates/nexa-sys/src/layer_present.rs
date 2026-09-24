//! **macOS 화면 내보내기** — CPU로 그린 픽셀을 `IOSurface`에 직접 그려 `CALayer.contents`로 넘긴다(nexa-sql docs/62 §2 · D-133 ② · T-147).
//!
//! 왜: `softbuffer` 0.4의 CoreGraphics 뒷단은 프레임마다 ① 새 버퍼를 할당하고(레티나 메인 창 = 21 MB) ② `CGImage`를
//! *DeviceRGB*로 만들어 넘긴다 → CoreAnimation이 **프레임마다 전체를 CPU(vImage)로 색 변환**한다(실측 present 37 ms · 프레임 45 ms).
//! 여기서는 ① 표면 몇 장을 돌려 쓰고(할당 0 · 복사 0) ② 표면에 **sRGB 색 공간 태그**를 달아 색 맞춤을 합성기(GPU)에 맡긴다.
//!
//! 규칙(nexa-sys 공통): 외부 crate 0 · 수동 FFI · 실패는 전부 `None`(호출자는 종전 경로로 돌아간다) · macOS 밖에서는 같은 모양의 빈 구현.
//! ★ 메인 스레드 전용(AppKit 레이어를 만진다) — 타입이 `Send`가 아니다.
//!
//! 픽셀 형식 = `softbuffer`와 같다: `u32` = `0x00RRGGBB`(리틀 엔디언 메모리 = B,G,R,X) · 행 간격 = 폭(패딩 없음).
//! 표면의 행 바이트는 OS가 정렬한다 — `폭 × 4`와 같으면 표면에 직접 그리고(복사 0), 다르면 중간 버퍼에 그린 뒤 행 단위로
//! 옮긴다(복사 1 · 알파 채우기와 한 번에 · 그래도 색 변환은 없다). 알파는 present 때 0xFF로 채운다('BGRA'의 알파 0 = 투명).

use std::ffi::c_void;

/// 한 프레임 — [`LayerPresenter::frame`]이 준다. 다 그린 뒤 [`Frame::present`].
#[allow(missing_debug_implementations)] // 픽셀 수백만 개를 찍을 일이 없다.
pub struct Frame<'a> {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    owner: &'a mut LayerPresenter,
    pixels: &'a mut [u32],
}

impl Frame<'_> {
    /// `폭 × 높이` 픽셀(행 우선 · 위에서 아래). 내용은 **이전 프레임을 보존하지 않는다**(전부 다시 그린다).
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels
    }

    /// 읽기 전용 보기.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        self.pixels
    }

    /// 화면에 낸다(암묵 애니메이션 없음).
    pub fn present(self) {
        #[cfg(target_os = "macos")]
        self.owner.present_locked();
    }
}

#[cfg(target_os = "macos")]
pub use mac::LayerPresenter;

/// 앱이 활성(전경)인가 — macOS `NSApplication.sharedApplication.isActive`(nexa-sql 09-24: 창은 키 창이어도 앱은 비활성일 수 있어
/// `WindowEvent::Focused`만으로는 "뒤에 있는 앱"을 못 가른다 · 캐럿 깜빡임 정지 판정). 다른 OS = `None`(창 포커스로 충분).
#[must_use]
pub fn app_active() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        mac::app_active()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub use other::LayerPresenter;

#[cfg(not(target_os = "macos"))]
mod other {
    use super::{c_void, Frame};

    /// macOS 밖 — 만들 수 없다(`new` = `None`).
    #[derive(Debug)]
    pub struct LayerPresenter {
        _never: std::convert::Infallible,
    }

    impl LayerPresenter {
        /// 늘 `None`.
        ///
        /// # Safety
        /// 포인터를 쓰지 않는다(모양만 macOS와 같게).
        #[must_use]
        pub unsafe fn new(_ns_view: *mut c_void) -> Option<LayerPresenter> {
            None
        }

        /// 도달하지 않는다.
        pub fn resize(&mut self, _w: u32, _h: u32, _scale: f64) -> bool {
            false
        }

        /// 도달하지 않는다.
        pub fn frame(&mut self) -> Option<Frame<'_>> {
            None
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{c_void, Frame};

    type Id = *mut c_void;
    type Sel = *const c_void;
    type CfRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CgRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const std::ffi::c_char) -> Id;
        fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
        fn objc_msgSend();
    }

    #[link(name = "QuartzCore", kind = "framework")]
    extern "C" {
        static kCAGravityTopLeft: Id;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        fn CFDictionaryCreateMutable(
            alloc: CfRef,
            capacity: isize,
            key_cb: *const c_void,
            val_cb: *const c_void,
        ) -> *mut c_void;
        fn CFDictionarySetValue(dict: *mut c_void, key: CfRef, value: CfRef);
        fn CFNumberCreate(alloc: CfRef, kind: isize, value: *const c_void) -> CfRef;
        fn CFRelease(cf: CfRef);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        static kCGColorSpaceSRGB: CfRef;
        fn CGColorSpaceCreateWithName(name: CfRef) -> CfRef;
        fn CGColorSpaceCopyPropertyList(space: CfRef) -> CfRef;
    }

    #[link(name = "IOSurface", kind = "framework")]
    extern "C" {
        static kIOSurfaceWidth: CfRef;
        static kIOSurfaceHeight: CfRef;
        static kIOSurfaceBytesPerElement: CfRef;
        static kIOSurfacePixelFormat: CfRef;
        static kIOSurfaceColorSpace: CfRef;
        fn IOSurfaceCreate(props: CfRef) -> *mut c_void;
        fn IOSurfaceLock(s: *mut c_void, options: u32, seed: *mut u32) -> i32;
        fn IOSurfaceUnlock(s: *mut c_void, options: u32, seed: *mut u32) -> i32;
        fn IOSurfaceGetBaseAddress(s: *mut c_void) -> *mut c_void;
        fn IOSurfaceGetBytesPerRow(s: *mut c_void) -> usize;
        fn IOSurfaceIsInUse(s: *mut c_void) -> u8;
        fn IOSurfaceSetValue(s: *mut c_void, key: CfRef, value: CfRef);
    }

    /// `kCFNumberSInt64Type`.
    const CF_NUMBER_I64: isize = 4;
    /// `'BGRA'`.
    const PIXEL_BGRA: i64 = 0x4247_5241;
    /// 돌려 쓰는 표면 수의 상한 — 합성기가 앞 장을 쥐고 있는 동안 다음 장에 그린다(보통 2장이면 된다).
    const POOL_MAX: usize = 3;

    fn sel(name: &'static std::ffi::CStr) -> Sel {
        // SAFETY: NUL로 끝나는 정적 C 문자열.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    // `objc_msgSend`는 호출하는 쪽 서명으로 캐스팅해 부른다(인자·반환형이 곧 ABI). 아래 도우미는 쓰는 모양만 둔다.
    unsafe fn send_id(obj: Id, s: Sel) -> Id {
        let f: unsafe extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s)
    }
    unsafe fn send_void(obj: Id, s: Sel) {
        let f: unsafe extern "C" fn(Id, Sel) = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s);
    }
    unsafe fn send_bool(obj: Id, s: Sel) -> bool {
        let f: unsafe extern "C" fn(Id, Sel) -> i8 = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s) != 0
    }

    /// `[[NSApplication sharedApplication] isActive]`.
    pub(super) fn app_active() -> Option<bool> {
        // SAFETY: 클래스·셀렉터 이름은 NUL로 끝나는 정적 문자열 · 반환은 BOOL(i8) · 메인 스레드에서 부른다.
        unsafe {
            let cls = objc_getClass(c"NSApplication".as_ptr());
            if cls.is_null() {
                return None;
            }
            let app = send_id(cls, sel(c"sharedApplication"));
            if app.is_null() {
                return None;
            }
            Some(send_bool(app, sel(c"isActive")))
        }
    }
    unsafe fn send_void_id(obj: Id, s: Sel, a: Id) {
        let f: unsafe extern "C" fn(Id, Sel, Id) = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, a);
    }
    unsafe fn send_void_bool(obj: Id, s: Sel, a: bool) {
        let f: unsafe extern "C" fn(Id, Sel, i8) = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, i8::from(a));
    }
    unsafe fn send_void_f64(obj: Id, s: Sel, a: f64) {
        let f: unsafe extern "C" fn(Id, Sel, f64) = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, a);
    }
    unsafe fn send_void_point(obj: Id, s: Sel, a: CgPoint) {
        let f: unsafe extern "C" fn(Id, Sel, CgPoint) =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, a);
    }
    unsafe fn send_void_rect(obj: Id, s: Sel, a: CgRect) {
        let f: unsafe extern "C" fn(Id, Sel, CgRect) =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, a);
    }

    /// 표면 한 장.
    struct Surf {
        raw: *mut c_void,
        /// 행 바이트(정렬 때문에 `폭 × 4`보다 클 수 있다).
        row_bytes: usize,
    }

    impl Drop for Surf {
        fn drop(&mut self) {
            // SAFETY: `IOSurfaceCreate`가 준 +1 참조를 놓는다(레이어가 쥔 참조는 레이어 몫).
            unsafe { CFRelease(self.raw) };
        }
    }

    /// 창의 내용 뷰에 붙는 표시 레이어 + 표면 풀.
    #[allow(missing_debug_implementations)] // 원시 포인터 묶음.
    pub struct LayerPresenter {
        layer: Id,
        w: usize,
        h: usize,
        scale: f64,
        pool: Vec<Surf>,
        /// 지금 화면에 걸린 장.
        front: Option<usize>,
        /// 잠가 둔(그리는 중인) 장.
        locked: Option<usize>,
        /// 행 간격이 폭과 다를 때만 쓰는 중간 버퍼.
        staging: Vec<u32>,
        /// sRGB 색 공간의 직렬화(표면마다 태그로 단다).
        srgb_plist: CfRef,
    }

    impl Drop for LayerPresenter {
        fn drop(&mut self) {
            if let Some(i) = self.locked.take() {
                // SAFETY: 우리가 잠근 표면.
                unsafe { IOSurfaceUnlock(self.pool[i].raw, 0, std::ptr::null_mut()) };
            }
            self.pool.clear();
            // SAFETY: 우리가 만든 레이어 · 메인 스레드에서만 존재한다.
            unsafe {
                send_void_id(self.layer, sel(c"setContents:"), std::ptr::null_mut());
                send_void(self.layer, sel(c"removeFromSuperlayer"));
                send_void(self.layer, sel(c"release"));
                if !self.srgb_plist.is_null() {
                    CFRelease(self.srgb_plist);
                }
            }
        }
    }

    impl LayerPresenter {
        /// 내용 뷰(`NSView*`)에 표시 레이어를 단다. 실패 = `None`.
        ///
        /// # Safety
        /// `ns_view`는 살아 있는 `NSView`여야 하고, 이 값이 사라질 때까지 살아 있어야 한다. **메인 스레드에서만** 부른다.
        #[must_use]
        pub unsafe fn new(ns_view: *mut c_void) -> Option<LayerPresenter> {
            if ns_view.is_null() {
                return None;
            }
            send_void_bool(ns_view, sel(c"setWantsLayer:"), true);
            let root = send_id(ns_view, sel(c"layer"));
            if root.is_null() {
                return None;
            }
            let class = objc_getClass(c"CALayer".as_ptr());
            if class.is_null() {
                return None;
            }
            let layer = send_id(class, sel(c"new"));
            if layer.is_null() {
                return None;
            }
            // softbuffer와 같은 배치: 원점 = 왼쪽 위 · 크기가 바뀌는 동안 내용은 왼쪽 위에 붙는다(늘어나지 않는다).
            send_void_point(layer, sel(c"setAnchorPoint:"), CgPoint { x: 0.0, y: 0.0 });
            send_void_bool(layer, sel(c"setGeometryFlipped:"), true);
            send_void_id(layer, sel(c"setContentsGravity:"), kCAGravityTopLeft);
            // 내용은 늘 불투명이다(알파는 present 때 0xFF로 채운다) — 합성기가 뒤를 섞지 않게 알린다.
            send_void_bool(layer, sel(c"setOpaque:"), true);
            send_void_id(root, sel(c"addSublayer:"), layer);
            let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            let srgb_plist = if space.is_null() {
                std::ptr::null()
            } else {
                let p = CGColorSpaceCopyPropertyList(space);
                CFRelease(space);
                p
            };
            Some(LayerPresenter {
                layer,
                w: 0,
                h: 0,
                scale: 1.0,
                pool: Vec::new(),
                front: None,
                locked: None,
                staging: Vec::new(),
                srgb_plist,
            })
        }

        /// 픽셀 크기와 배율을 맞춘다(같으면 아무 일도 없다). `false` = 크기 0.
        pub fn resize(&mut self, w: u32, h: u32, scale: f64) -> bool {
            if w == 0 || h == 0 {
                return false;
            }
            let (w, h) = (w as usize, h as usize);
            let scale = if scale > 0.0 { scale } else { 1.0 };
            if (w, h) == (self.w, self.h) && (scale - self.scale).abs() < f64::EPSILON {
                return true;
            }
            self.w = w;
            self.h = h;
            self.scale = scale;
            // 크기가 다른 표면은 못 쓴다 — 풀을 비운다(화면에 걸린 장은 레이어가 쥐고 있어 다음 present까지 그대로 보인다).
            self.pool.clear();
            self.front = None;
            self.locked = None;
            // SAFETY: 우리 레이어 · 메인 스레드.
            unsafe {
                self.begin();
                send_void_f64(self.layer, sel(c"setContentsScale:"), scale);
                send_void_rect(
                    self.layer,
                    sel(c"setFrame:"),
                    CgRect {
                        x: 0.0,
                        y: 0.0,
                        w: w as f64 / scale,
                        h: h as f64 / scale,
                    },
                );
                self.commit();
            }
            true
        }

        /// 그릴 프레임을 연다(쓸 수 있는 장을 잠근다). 실패 = `None`.
        pub fn frame(&mut self) -> Option<Frame<'_>> {
            if self.w == 0 || self.h == 0 {
                return None;
            }
            if let Some(i) = self.locked.take() {
                // 앞 프레임이 present 없이 버려졌다 — 잠금만 푼다.
                // SAFETY: 우리가 잠근 표면.
                unsafe { IOSurfaceUnlock(self.pool[i].raw, 0, std::ptr::null_mut()) };
            }
            let i = self.pick()?;
            let (raw, row_bytes) = (self.pool[i].raw, self.pool[i].row_bytes);
            // SAFETY: 살아 있는 표면 · 잠금은 `present_locked`/다음 `frame`/`Drop`에서 푼다.
            let base = unsafe {
                if IOSurfaceLock(raw, 0, std::ptr::null_mut()) != 0 {
                    return None;
                }
                IOSurfaceGetBaseAddress(raw)
            };
            if base.is_null() {
                // SAFETY: 방금 잠근 표면.
                unsafe { IOSurfaceUnlock(raw, 0, std::ptr::null_mut()) };
                return None;
            }
            self.locked = Some(i);
            let n = self.w * self.h;
            let direct = row_bytes == self.w * 4;
            let pixels: &mut [u32] = if direct {
                // SAFETY: 표면 메모리는 `row_bytes × h` = `w × 4 × h` 바이트 · 페이지 정렬(u32 정렬 충족) · 잠겨 있는 동안 우리만 쓴다.
                unsafe { std::slice::from_raw_parts_mut(base.cast::<u32>(), n) }
            } else {
                self.staging.resize(n, 0);
                // SAFETY: 수명을 `&mut self`에 묶는다(아래 Frame이 self를 독점).
                unsafe { std::slice::from_raw_parts_mut(self.staging.as_mut_ptr(), n) }
            };
            Some(Frame {
                owner: self,
                pixels,
            })
        }

        /// 쓸 장을 고른다: 화면에 걸리지 않았고 합성기가 쓰고 있지 않은 장 → 없으면 새로(상한까지) → 그래도 없으면 걸리지 않은 아무 장.
        fn pick(&mut self) -> Option<usize> {
            let front = self.front;
            // SAFETY: 살아 있는 표면의 사용 여부 질의.
            let free = (0..self.pool.len())
                .find(|&i| Some(i) != front && unsafe { IOSurfaceIsInUse(self.pool[i].raw) } == 0);
            if free.is_some() {
                return free;
            }
            if self.pool.len() < POOL_MAX {
                let s = self.create()?;
                self.pool.push(s);
                return Some(self.pool.len() - 1);
            }
            (0..self.pool.len()).find(|&i| Some(i) != front)
        }

        fn create(&self) -> Option<Surf> {
            // SAFETY: CF 객체를 만들고 같은 자리에서 놓는다 · 키는 프레임워크 상수.
            unsafe {
                let dict = CFDictionaryCreateMutable(
                    std::ptr::null(),
                    0,
                    std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks),
                    std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks),
                );
                if dict.is_null() {
                    return None;
                }
                let put = |key: CfRef, v: i64| {
                    let n = CFNumberCreate(
                        std::ptr::null(),
                        CF_NUMBER_I64,
                        std::ptr::addr_of!(v).cast(),
                    );
                    if !n.is_null() {
                        CFDictionarySetValue(dict, key, n);
                        CFRelease(n);
                    }
                };
                put(kIOSurfaceWidth, self.w as i64);
                put(kIOSurfaceHeight, self.h as i64);
                put(kIOSurfaceBytesPerElement, 4);
                // ★ 행 바이트는 **지정하지 않는다**(OS 기본 정렬 · 보통 64바이트 = 16픽셀 단위). `폭 × 4`를 강제하면 표면은 만들어지지만
                //   합성기가 그리지 않는다(실측: Intel + AMD · 폭 2750 → 11000바이트 = 빈 창 · 11008 = 정상). 폭이 정렬 단위면
                //   `row_bytes == 폭 × 4`라 직접 그리고, 아니면 중간 버퍼에서 행 단위로 옮긴다.
                put(kIOSurfacePixelFormat, PIXEL_BGRA);
                let raw = IOSurfaceCreate(dict);
                CFRelease(dict);
                if raw.is_null() {
                    return None;
                }
                if !self.srgb_plist.is_null() {
                    IOSurfaceSetValue(raw, kIOSurfaceColorSpace, self.srgb_plist);
                }
                let row_bytes = IOSurfaceGetBytesPerRow(raw);
                if row_bytes < self.w * 4 {
                    CFRelease(raw);
                    return None;
                }
                Some(Surf { raw, row_bytes })
            }
        }

        pub(super) fn present_locked(&mut self) {
            let Some(i) = self.locked.take() else {
                return;
            };
            let (raw, row_bytes) = (self.pool[i].raw, self.pool[i].row_bytes);
            // SAFETY: 우리가 잠근 표면 · 중간 버퍼는 `w × h` · 표면은 `row_bytes × h` 바이트.
            unsafe {
                // ★ 알파 채우기: 그리는 쪽은 `0x00RRGGBB`를 쓰는데 'BGRA' 표면의 알파 0은 **투명**으로 합성된다
                //   (`opaque`는 힌트일 뿐 — 실측: 창이 하얗게 나온다). 한 번 훑어 0xFF를 채운다(자동 벡터화 · 레티나 메인 창 ≈ 1~2 ms).
                let base = IOSurfaceGetBaseAddress(raw);
                if row_bytes == self.w * 4 {
                    let px = std::slice::from_raw_parts_mut(base.cast::<u32>(), self.w * self.h);
                    for p in px.iter_mut() {
                        *p |= 0xFF00_0000;
                    }
                } else {
                    for y in 0..self.h {
                        let src = &self.staging[y * self.w..(y + 1) * self.w];
                        let dst = std::slice::from_raw_parts_mut(
                            base.cast::<u8>().add(y * row_bytes).cast::<u32>(),
                            self.w,
                        );
                        for (d, s) in dst.iter_mut().zip(src) {
                            *d = *s | 0xFF00_0000;
                        }
                    }
                }
                IOSurfaceUnlock(raw, 0, std::ptr::null_mut());
                self.begin();
                send_void_id(self.layer, sel(c"setContents:"), raw);
                self.commit();
            }
            self.front = Some(i);
        }

        /// 암묵 애니메이션 없는 트랜잭션.
        unsafe fn begin(&self) {
            let tx = objc_getClass(c"CATransaction".as_ptr());
            send_void(tx, sel(c"begin"));
            send_void_bool(tx, sel(c"setDisableActions:"), true);
        }

        unsafe fn commit(&self) {
            let tx = objc_getClass(c"CATransaction".as_ptr());
            send_void(tx, sel(c"commit"));
        }

        /// 지금 풀에 있는 표면 수(진단).
        #[must_use]
        pub fn pool_len(&self) -> usize {
            self.pool.len()
        }
    }
}
