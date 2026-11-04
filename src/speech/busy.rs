//! Whether another app is playing sound right now (music, a call, a video), so me-voice
//! can keep its T3 updates to itself instead of talking over it.

/// True when any other app's audio session is audible. Samples for ~300 ms, so short gaps
/// in music don't count as silence. Blocks: call it off the UI thread.
pub fn other_audio_playing() -> bool {
    #[cfg(target_os = "windows")]
    {
        for i in 0..6 {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            match windows_impl::peak_of_others() {
                Ok(peak) if peak > 0.01 => return true,
                Ok(_) => {}
                Err(e) => {
                    crate::app::log::write(&format!("audio check: {e}"));
                    return false;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Test helper: (process id, peak) of other apps' active audio sessions.
#[cfg(target_os = "windows")]
pub fn sessions() -> Vec<(u32, f32)> {
    windows_impl::sessions().unwrap_or_default()
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};

    /// Loudest current peak (0..1) across active sessions on the default output, ours excluded.
    pub fn peak_of_others() -> windows::core::Result<f32> {
        Ok(sessions()?.iter().map(|(_, peak)| *peak).fold(0.0, f32::max))
    }

    /// Process id and current peak of each active session on the default output, ours excluded.
    pub fn sessions() -> windows::core::Result<Vec<(u32, f32)>> {
        unsafe {
            let init = CoInitializeEx(None, COINIT_MULTITHREADED);
            let result = (|| {
                let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
                let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
                let sessions = manager.GetSessionEnumerator()?;
                let me = std::process::id();
                let mut found = Vec::new();
                for i in 0..sessions.GetCount()? {
                    let Ok(control) = sessions.GetSession(i) else { continue };
                    let Ok(control) = control.cast::<IAudioSessionControl2>() else { continue };
                    if control.GetProcessId().unwrap_or(0) == me || control.GetState().ok() != Some(AudioSessionStateActive) {
                        continue;
                    }
                    if let Ok(meter) = control.cast::<IAudioMeterInformation>() {
                        found.push((control.GetProcessId().unwrap_or(0), meter.GetPeakValue().unwrap_or(0.0)));
                    }
                }
                Ok(found)
            })();
            if init.is_ok() {
                CoUninitialize();
            }
            result
        }
    }
}
