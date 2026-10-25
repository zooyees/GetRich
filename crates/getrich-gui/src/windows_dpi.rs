//! Windows mixed-DPI: apply the OS suggested rect; do not let winit nudge.
//!
//! winit 0.30.13 does not handle `WM_GETDPISCALEDSIZE`, then on `WM_DPICHANGED`
//! it **ignores** the system `lParam` rect and rebuilds a conservative size from
//! inner logical size. While `MARKER_IN_SIZE_MOVE` is set it also recenters with
//! `MonitorFromWindow` (still the *old* monitor on Win11 24H2, winit#4041/#4600).
//! That SetWindowPos ping-pongs DPI and grows/shrinks the window under the cursor.
//!
//! Swallowing `WM_DPICHANGED` and returning the *current* outer size from
//! `WM_GETDPISCALEDSIZE` was the wrong countermeasure: the window kept physical
//! pixels so it looked scaled-wrong on the other monitor, then jumped on drop.
//!
//! Correct sequence (Microsoft + Chromium):
//! - Let `WM_GETDPISCALEDSIZE` fall through (linear logical size; Windows keeps
//!   the cursor on the caption).
//! - Always deliver `WM_DPICHANGED` to winit so `scale_factor` updates.
//! - Then `SetWindowPos` the **suggested** rect so winit's nudge cannot stick.
//! - Re-entrancy guard: nested `WM_DPICHANGED` from those SetWindowPos is ignored.

#[cfg(target_os = "windows")]
mod imp {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    type Bool = i32;
    type Dword = u32;
    type Handle = *mut std::ffi::c_void;
    type Hwnd = Handle;
    type Lparam = isize;
    type Lresult = isize;
    type Uint = u32;
    type Wparam = usize;

    const GWLP_WNDPROC: i32 = -4;
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WM_DPICHANGED: Uint = 0x02E0;
    const WM_NCDESTROY: Uint = 0x0082;
    const HWND_TOP: Hwnd = std::ptr::null_mut();
    const SWP_NOZORDER: Uint = 0x0004;
    const SWP_NOACTIVATE: Uint = 0x0010;
    const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: Handle = -4isize as Handle;
    const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE: Handle = -3isize as Handle;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> Dword;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetProcessDpiAwarenessContext(value: Handle) -> Bool;
        fn GetWindowLongPtrW(window: Hwnd, index: i32) -> isize;
        fn SetWindowLongPtrW(window: Hwnd, index: i32, value: isize) -> isize;
        fn CallWindowProcW(
            prev: isize,
            window: Hwnd,
            message: Uint,
            wparam: Wparam,
            lparam: Lparam,
        ) -> Lresult;
        fn DefWindowProcW(window: Hwnd, message: Uint, wparam: Wparam, lparam: Lparam) -> Lresult;
        fn EnumWindows(
            callback: unsafe extern "system" fn(Hwnd, Lparam) -> Bool,
            data: Lparam,
        ) -> Bool;
        fn GetWindowThreadProcessId(window: Hwnd, process_id: *mut Dword) -> Dword;
        fn IsWindowVisible(window: Hwnd) -> Bool;
        fn GetWindowRect(window: Hwnd, rect: *mut Rect) -> Bool;
        fn SetWindowPos(
            window: Hwnd,
            insert_after: Hwnd,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: Uint,
        ) -> Bool;
        fn GetForegroundWindow() -> Hwnd;
        fn IsWindow(window: Hwnd) -> Bool;
    }

    static ORIG: std::sync::OnceLock<Mutex<HashMap<isize, isize>>> = std::sync::OnceLock::new();

    fn orig_map() -> &'static Mutex<HashMap<isize, isize>> {
        ORIG.get_or_init(|| Mutex::new(HashMap::new()))
    }
    static IN_DPI: AtomicBool = AtomicBool::new(false);

    fn hwnd_key(window: Hwnd) -> isize {
        window as isize
    }

    fn original_proc(window: Hwnd) -> isize {
        orig_map()
            .lock()
            .ok()
            .and_then(|g| g.get(&hwnd_key(window)).copied())
            .unwrap_or(0)
    }

    fn call_original(window: Hwnd, message: Uint, wparam: Wparam, lparam: Lparam) -> Lresult {
        let orig = original_proc(window);
        unsafe {
            if orig == 0 {
                DefWindowProcW(window, message, wparam, lparam)
            } else {
                CallWindowProcW(orig, window, message, wparam, lparam)
            }
        }
    }

    fn apply_suggested(window: Hwnd, lparam: Lparam) {
        if lparam == 0 {
            return;
        }
        let rect = unsafe { *(lparam as *const Rect) };
        let cx = rect.right - rect.left;
        let cy = rect.bottom - rect.top;
        if cx <= 0 || cy <= 0 {
            return;
        }
        unsafe {
            SetWindowPos(
                window,
                HWND_TOP,
                rect.left,
                rect.top,
                cx,
                cy,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn is_tool_window(window: Hwnd) -> bool {
        (unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } & WS_EX_TOOLWINDOW) != 0
    }

    fn is_real_window(window: Hwnd) -> bool {
        if window.is_null() || unsafe { IsWindow(window) } == 0 {
            return false;
        }
        if unsafe { IsWindowVisible(window) } == 0 || is_tool_window(window) {
            return false;
        }
        let mut rect = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(window, &mut rect) } == 0 {
            return false;
        }
        let cx = rect.right - rect.left;
        let cy = rect.bottom - rect.top;
        cx >= 200 && cy >= 150
    }

    unsafe extern "system" fn dpi_wnd_proc(
        window: Hwnd,
        message: Uint,
        wparam: Wparam,
        lparam: Lparam,
    ) -> Lresult {
        match message {
            WM_DPICHANGED => {
                if IN_DPI.swap(true, Ordering::SeqCst) {
                    return 0;
                }
                let result = call_original(window, message, wparam, lparam);
                apply_suggested(window, lparam);
                IN_DPI.store(false, Ordering::SeqCst);
                return result;
            }
            WM_NCDESTROY => {
                let result = call_original(window, message, wparam, lparam);
                if let Ok(mut g) = orig_map().lock() {
                    g.remove(&hwnd_key(window));
                }
                return result;
            }
            _ => {}
        }
        call_original(window, message, wparam, lparam)
    }

    pub fn enable_per_monitor_v2() {
        unsafe {
            if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
                let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
            }
        }
    }

    pub fn install_on_window(window: Hwnd) -> bool {
        if !is_real_window(window) {
            return false;
        }
        let new_proc = dpi_wnd_proc as *const () as isize;
        unsafe {
            let prev = GetWindowLongPtrW(window, GWLP_WNDPROC);
            if prev == 0 || prev == new_proc {
                return prev == new_proc;
            }
            if let Ok(mut g) = orig_map().lock() {
                g.insert(hwnd_key(window), prev);
            } else {
                return false;
            }
            let old = SetWindowLongPtrW(window, GWLP_WNDPROC, new_proc);
            if old == 0 {
                if let Ok(mut g) = orig_map().lock() {
                    g.remove(&hwnd_key(window));
                }
                return false;
            }
        }
        true
    }

    unsafe extern "system" fn hook_visible_window(window: Hwnd, data: Lparam) -> Bool {
        let state = &mut *(data as *mut (Dword, bool));
        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        if process_id == state.0 && install_on_window(window) {
            state.1 = true;
        }
        1
    }

    pub fn install() -> bool {
        unsafe {
            let mut state = (GetCurrentProcessId(), false);
            EnumWindows(
                hook_visible_window,
                &mut state as *mut (Dword, bool) as Lparam,
            );
            let fg = GetForegroundWindow();
            if install_on_window(fg) {
                state.1 = true;
            }
            state.1 || orig_map().lock().ok().is_some_and(|g| !g.is_empty())
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::{enable_per_monitor_v2, install, install_on_window};

#[cfg(not(target_os = "windows"))]
pub fn enable_per_monitor_v2() {}

#[cfg(not(target_os = "windows"))]
pub fn install() -> bool {
    true
}

#[cfg(not(target_os = "windows"))]
pub fn install_on_window(_window: *mut std::ffi::c_void) -> bool {
    true
}
