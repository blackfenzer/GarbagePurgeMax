#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub struct NativeTrash;

impl NativeTrash {
    pub fn supported() -> bool {
        cfg!(any(target_os = "macos", windows))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
impl crate::TrashBackend for NativeTrash {
    fn trash(&self, _: &std::path::Path) -> std::io::Result<crate::TrashReceipt> {
        Err(std::io::Error::other(
            "Native Trash is supported only on macOS and Windows",
        ))
    }
    fn restore(&self, _: &crate::TrashReceipt, _: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "Native restore is unsupported on this OS",
        ))
    }
}
