#![allow(non_snake_case)] // COM interface method names follow the Windows SDK.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{E_FAIL, E_NOTIMPL};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::UI::Shell::*;
use windows::core::{HRESULT, PCWSTR, Ref, Result, implement};

use crate::{NativeTrash, TrashBackend, TrashReceipt};

impl TrashBackend for NativeTrash {
    fn trash(&self, path: &Path) -> io::Result<TrashReceipt> {
        let path = path.to_path_buf();
        in_apartment(move || {
            let state = Arc::new(Mutex::new(None));
            let sink: IFileOperationProgressSink = ReceiptSink {
                result: state.clone(),
            }
            .into();
            unsafe {
                let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
                // Recycle-only; never use a permanent deletion fallback or elevation.
                operation.SetOperationFlags(
                    FOFX_RECYCLEONDELETE
                        | FOFX_ADDUNDORECORD
                        | FOFX_EARLYFAILURE
                        | FOF_NOERRORUI
                        | FOF_SILENT
                        | FOF_NO_CONNECTED_ELEMENTS,
                )?;
                operation.DeleteItem(&shell_item(&path)?, &sink)?;
                operation.PerformOperations()?;
                if operation.GetAnyOperationsAborted()?.as_bool() {
                    return Err(E_FAIL.into());
                }
            }
            let result = state
                .lock()
                .map_err(|_| windows::core::Error::from(E_FAIL))?
                .take();
            result.ok_or_else(|| {
                windows::core::Error::new(
                    E_FAIL,
                    "No Recycle Bin receipt; inspect the item in Windows Explorer",
                )
            })?
        })
    }

    fn restore(&self, receipt: &TrashReceipt, destination: &Path) -> io::Result<()> {
        let identifier = receipt.identifier.clone();
        let destination = destination.to_path_buf();
        in_apartment(move || {
            let parent = destination
                .parent()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            let name: Vec<u16> = destination
                .file_name()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?
                .encode_wide()
                .chain(Some(0))
                .collect();
            let state = Arc::new(Mutex::new(None));
            let sink: IFileOperationProgressSink = ReceiptSink {
                result: state.clone(),
            }
            .into();
            unsafe {
                let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
                // A racing destination creation can cause a renamed restore, never an overwrite.
                operation.SetOperationFlags(
                    FOF_RENAMEONCOLLISION
                        | FOFX_EARLYFAILURE
                        | FOF_NOERRORUI
                        | FOF_SILENT
                        | FOF_NO_CONNECTED_ELEMENTS,
                )?;
                operation.MoveItem(
                    &shell_item(&identifier)?,
                    &shell_item(parent)?,
                    PCWSTR(name.as_ptr()),
                    &sink,
                )?;
                operation.PerformOperations()?;
                if operation.GetAnyOperationsAborted()?.as_bool() {
                    return Err(E_FAIL.into());
                }
            }
            let result = state
                .lock()
                .map_err(|_| windows::core::Error::from(E_FAIL))?
                .take()
                .ok_or_else(|| {
                    windows::core::Error::new(
                        E_FAIL,
                        "Restore outcome is unknown; inspect Windows Explorer",
                    )
                })??;
            if result.path != destination {
                return Err(windows::core::Error::new(
                    E_FAIL,
                    &format!(
                        "Windows restored to {}; inspect the renamed item",
                        result.path.display()
                    ),
                ));
            }
            Ok(())
        })
    }
}

fn in_apartment<T: Send + 'static>(
    action: impl FnOnce() -> Result<T> + Send + 'static,
) -> io::Result<T> {
    std::thread::spawn(move || {
        // Each operation owns its COM apartment; pool threads may already use another model.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        action()
    })
    .join()
    .map_err(|_| io::Error::other("Windows shell worker failed"))?
    .map_err(|error| io::Error::other(error.to_string()))
}

unsafe fn shell_item(path: &Path) -> Result<IShellItem> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(E_FAIL.into());
    }
    unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
}

unsafe fn item_path(item: &IShellItem, kind: SIGDN) -> Result<PathBuf> {
    let text = unsafe { item.GetDisplayName(kind)? };
    let path = PathBuf::from(OsString::from_wide(unsafe { text.as_wide() }));
    unsafe {
        CoTaskMemFree(Some(text.0.cast()));
    }
    Ok(path)
}

type ReceiptResult = Arc<Mutex<Option<Result<TrashReceipt>>>>;

#[implement(IFileOperationProgressSink)]
struct ReceiptSink {
    result: ReceiptResult,
}

impl ReceiptSink_Impl {
    fn record(&self, status: HRESULT, item: Ref<'_, IShellItem>) -> Result<()> {
        let receipt = (|| {
            status.ok()?;
            let item = item
                .as_ref()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            unsafe {
                Ok(TrashReceipt {
                    identifier: item_path(item, SIGDN_DESKTOPABSOLUTEPARSING)?,
                    path: item_path(item, SIGDN_FILESYSPATH)?,
                })
            }
        })();
        *self
            .result
            .lock()
            .map_err(|_| windows::core::Error::from(E_FAIL))? = Some(receipt);
        Ok(())
    }
}

impl IFileOperationProgressSink_Impl for ReceiptSink_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, result: HRESULT) -> Result<()> {
        result.ok()
    }
    fn PreDeleteItem(&self, flags: u32, _: Ref<'_, IShellItem>) -> Result<()> {
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            return Err(windows::core::Error::new(
                E_FAIL,
                "Windows did not offer a recyclable action; permanent deletion is forbidden",
            ));
        }
        Ok(())
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        result: HRESULT,
        item: Ref<'_, IShellItem>,
    ) -> Result<()> {
        self.record(result, item)
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Ok(())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        result: HRESULT,
        item: Ref<'_, IShellItem>,
    ) -> Result<()> {
        self.record(result, item)
    }
    fn PreRenameItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn PreNewItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}
