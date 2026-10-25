//! Native selection runs on the command worker, leaving the desktop responsive.
#[cfg(windows)]
pub fn select_import() -> Result<Option<std::path::PathBuf>, String> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let mut buffer = vec![0u16; 32768];
    let filter: Vec<u16> = "JSON 数据文件\0*.json\0所有文件\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "选择要校验的数据文件\0".encode_utf16().collect();
    // OPENFILENAMEW is zero-initializable; backing UTF-16 buffers live through the modal call.
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFile = buffer.as_mut_ptr();
    dialog.nMaxFile = buffer.len() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrTitle = title.as_ptr();
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 {
            Ok(None)
        } else {
            Err(format!("文件选择失败：{error}"))
        };
    }
    use std::os::windows::ffi::OsStringExt;
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Ok(Some(std::path::PathBuf::from(
        std::ffi::OsString::from_wide(&buffer[..end]),
    )))
}
#[cfg(not(windows))]
pub fn select_import() -> Result<Option<std::path::PathBuf>, String> {
    Err("请填写数据目录内的文件路径，或通过浏览器上传文件。".into())
}
