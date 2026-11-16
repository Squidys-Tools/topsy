//! The default output device's peak meter, the same one the volume flyout draws: how loud the
//! mix is right now, read without capturing any audio.

use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

/// No meter (no output device) reads as silence.
pub struct Meter(Option<IAudioMeterInformation>);

impl Meter {
    /// The meter of the device playing now. Called on GPUI's thread, which has COM set up.
    pub fn open() -> Self {
        let meter = unsafe {
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .and_then(|devices| devices.GetDefaultAudioEndpoint(eRender, eMultimedia))
                .and_then(|device| device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None))
        };
        Self(meter.ok())
    }

    /// How loud the latest audio is, 0 to 1. Silence if the device has gone.
    pub fn peak(&self) -> f32 {
        let peak = self.0.as_ref().map(|m| unsafe { m.GetPeakValue() });
        peak.and_then(Result::ok).unwrap_or(0.0)
    }
}
