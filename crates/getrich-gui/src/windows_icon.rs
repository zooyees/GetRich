//! Windows taskbar icon integration (WiParse `windows_icon` parity).
//!
//! `ViewportBuilder::with_icon` updates winit's window icon, but some Windows
//! shell configurations keep the generic application icon. Apply the embedded
//! PE icon directly to the native top-level window as a reliable fallback.

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::ptr;

    type Bool = i32;
    type Dword = u32;
    type Handle = *mut c_void;
    type Hwnd = Handle;
    type Lparam = isize;
    type Lresult = isize;
    type Uint = u32;
    type Wparam = usize;

    const IMAGE_ICON: Uint = 1;
    const LR_DEFAULTCOLOR: Uint = 0;
    const LR_SHARED: Uint = 0x0000_8000;
    const WM_SETICON: Uint = 0x0080;
    const ICON_SMALL: Wparam = 0;
    const ICON_BIG: Wparam = 1;
    const GCLP_HICON: i32 = -14;
    const GCLP_HICONSM: i32 = -34;
    const ICON_RESOURCE_ID: usize = 1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> Dword;
        fn GetModuleHandleW(module_name: *const u16) -> Handle;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(
            callback: unsafe extern "system" fn(Hwnd, Lparam) -> Bool,
            data: Lparam,
        ) -> Bool;
        fn GetWindowThreadProcessId(window: Hwnd, process_id: *mut Dword) -> Dword;
        fn IsWindowVisible(window: Hwnd) -> Bool;
        fn LoadImageW(
            instance: Handle,
            name: *const u16,
            image_type: Uint,
            width: i32,
            height: i32,
            flags: Uint,
        ) -> Handle;
        fn SendMessageW(window: Hwnd, message: Uint, wparam: Wparam, lparam: Lparam) -> Lresult;
        fn SetClassLongPtrW(window: Hwnd, index: i32, value: isize) -> usize;
    }

    struct IconApply {
        process_id: Dword,
        big: Handle,
        small: Handle,
        applied: bool,
    }

    unsafe extern "system" fn apply_to_window(window: Hwnd, data: Lparam) -> Bool {
        let state = &mut *(data as *mut IconApply);
        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        if process_id != state.process_id || IsWindowVisible(window) == 0 {
            return 1;
        }

        SendMessageW(window, WM_SETICON, ICON_BIG, state.big as Lparam);
        SendMessageW(window, WM_SETICON, ICON_SMALL, state.small as Lparam);
        SetClassLongPtrW(window, GCLP_HICON, state.big as isize);
        SetClassLongPtrW(window, GCLP_HICONSM, state.small as isize);
        state.applied = true;
        1
    }

    pub fn set_process_app_id() {
        let app_id: Vec<u16> = "YuZhao.GetRich\0".encode_utf16().collect();
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
        }
    }

    pub fn apply_embedded_icon() -> bool {
        unsafe {
            let module = GetModuleHandleW(ptr::null());
            if module.is_null() {
                return false;
            }
            let resource = ICON_RESOURCE_ID as *const u16;
            let flags = LR_DEFAULTCOLOR | LR_SHARED;
            let big = LoadImageW(module, resource, IMAGE_ICON, 32, 32, flags);
            let small = LoadImageW(module, resource, IMAGE_ICON, 16, 16, flags);
            if big.is_null() || small.is_null() {
                return false;
            }
            let mut state = IconApply {
                process_id: GetCurrentProcessId(),
                big,
                small,
                applied: false,
            };
            EnumWindows(apply_to_window, &mut state as *mut IconApply as Lparam);
            state.applied
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::{apply_embedded_icon, set_process_app_id};

#[cfg(not(target_os = "windows"))]
pub fn set_process_app_id() {}

#[cfg(not(target_os = "windows"))]
pub fn apply_embedded_icon() -> bool {
    true
}
