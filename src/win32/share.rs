//! Windows' share sheet (Nearby Sharing, Mail, Teams and the other share targets), for files.

use std::path::PathBuf;
use std::sync::Mutex;

use windows::ApplicationModel::DataTransfer::{
    DataRequest, DataRequestedEventArgs, DataTransferManager,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::{IStorageItem, StorageFile, StorageFolder};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::COINIT_MULTITHREADED;
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows::core::{HSTRING, Interface, Result};
use windows_collections::IIterable;

use super::with_com;

use crate::log::LogErr;

/// What the next share hands over: its title and files. The sheet asks for it once it is up.
static PENDING: Mutex<Option<(String, Vec<PathBuf>)>> = Mutex::new(None);
/// The windows (as addresses) whose `DataRequested` handler is registered.
static REGISTERED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Open the share sheet for `paths`, over `hwnd`.
///
/// Takes the foreground for `hwnd`, and the sheet may ask for the files from inside this call:
/// both send messages to `hwnd`'s thread, so call it from a foreground task, never inside an
/// event handler.
pub fn share(hwnd: HWND, title: String, paths: Vec<PathBuf>) -> Result<()> {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((title, paths));
    let interop = windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()?;
    let mut registered = REGISTERED.lock().unwrap_or_else(|e| e.into_inner());
    if !registered.contains(&(hwnd.0 as usize)) {
        let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd) }?;
        manager.DataRequested(&TypedEventHandler::new(
            |_, args: windows::core::Ref<DataRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    on_request(args.Request()?)?;
                }
                Ok(())
            },
        ))?;
        // The handler goes with the manager, and the window lives as long as the app.
        std::mem::forget(manager);
        registered.push(hwnd.0 as usize);
    }
    drop(registered);
    // The sheet only comes up over the foreground window: for the basket, a no-activate
    // window, it otherwise returns success and never asks for the files.
    super::bring_to_front(hwnd);
    unsafe { interop.ShowShareUIForWindow(hwnd) }
}

/// The sheet wants the files. Opening them as storage items waits on the file broker, so it
/// happens on a thread of its own while a deferral holds the sheet.
fn on_request(request: DataRequest) -> Result<()> {
    let Some((title, paths)) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return request.FailWithDisplayText(&HSTRING::from("Nothing to share"));
    };
    let deferral = request.GetDeferral()?;
    std::thread::Builder::new()
        .name("topsy-share".into())
        .spawn(move || {
            with_com(COINIT_MULTITHREADED, || {
                if let Err(err) = fill(&request, &title, &paths) {
                    eprintln!("topsy: could not share: {err}");
                    request
                        .FailWithDisplayText(&HSTRING::from(err.message()))
                        .log_err();
                }
                crate::log("share filled");
                deferral.Complete().log_err();
            })
        })
        .map(drop)
        .map_err(|err| {
            windows::core::Error::new(windows::Win32::Foundation::E_FAIL, err.to_string())
        })
}

fn fill(request: &DataRequest, title: &str, paths: &[PathBuf]) -> Result<()> {
    let mut items: Vec<Option<IStorageItem>> = Vec::new();
    for path in paths {
        let wide = HSTRING::from(path.as_os_str());
        let item: IStorageItem = if path.is_dir() {
            StorageFolder::GetFolderFromPathAsync(&wide)?
                .join()?
                .cast()?
        } else {
            StorageFile::GetFileFromPathAsync(&wide)?.join()?.cast()?
        };
        items.push(Some(item));
    }
    let data = request.Data()?;
    data.Properties()?.SetTitle(&HSTRING::from(title))?;
    data.SetStorageItemsReadOnly(&IIterable::from(items))
}
