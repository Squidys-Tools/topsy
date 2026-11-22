//! Apple's Bluetooth LE broadcasts, heard by a passive scan that the caller starts for a few
//! seconds and stops: it is radio time, so it never runs on its own (`earbuds`).

use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEScanningMode,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;

use model::headset::apple::APPLE;

/// Start scanning; `heard` gets each broadcast's signal strength and Apple's manufacturer data,
/// on a thread pool thread. Dropping the watcher doesn't stop it: call `Stop`.
pub fn scan(
    heard: impl Fn(i16, Vec<u8>) + Send + 'static,
) -> windows::core::Result<BluetoothLEAdvertisementWatcher> {
    let watcher = BluetoothLEAdvertisementWatcher::new()?;
    watcher.SetScanningMode(BluetoothLEScanningMode::Passive)?;
    watcher.Received(&TypedEventHandler::new(
        move |_, args: windows::core::Ref<BluetoothLEAdvertisementReceivedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let rssi = args.RawSignalStrengthInDBm()?;
                for data in args.Advertisement()?.ManufacturerData()? {
                    if data.CompanyId()? == APPLE {
                        let buffer = data.Data()?;
                        let mut bytes = vec![0; buffer.Length()? as usize];
                        DataReader::FromBuffer(&buffer)?.ReadBytes(&mut bytes)?;
                        heard(rssi, bytes);
                    }
                }
            }
            Ok(())
        },
    ))?;
    watcher.Start()?;
    Ok(watcher)
}
