//! Windows 11 "power throttling" (EcoQoS) moves background apps without a visible
//! window onto slow efficiency cores. That made transcription several times slower
//! in the release build, so me-voice opts out. Costs nothing while idle: the app
//! sleeps either way, it just runs at full speed when it does work.

pub fn disable_throttling() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        };
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            // Control execution speed, and set it to "not throttled".
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: 0,
        };
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const _,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Normal,
    /// Live preview while talking.
    AboveNormal,
    /// Final transcription: run ahead of background work (VS Code, browsers, …) so
    /// whisper's threads aren't paused; it only lasts a second or two.
    High,
}

/// Sets the process priority. Back to `Normal` as soon as the work is done.
pub fn set_priority(priority: Priority) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, SetPriorityClass, ABOVE_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        };
        let class = match priority {
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Priority::High => HIGH_PRIORITY_CLASS,
        };
        SetPriorityClass(GetCurrentProcess(), class);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = priority;
}
