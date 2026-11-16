use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW},
};

pub struct InstanceGuard(HANDLE);

impl InstanceGuard {
    pub fn acquire() -> Option<Self> {
        let name: Vec<u16> = "Local\\VibeVoiceLite.SingleInstance\0"
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        let error = unsafe { GetLastError() };
        if handle.is_null() || error == ERROR_ALREADY_EXISTS {
            if !handle.is_null() {
                unsafe {
                    CloseHandle(handle);
                }
            }
            let text = if error == ERROR_ALREADY_EXISTS {
                "Vibe Voice Lite 已在运行。如果看不到悬浮条，请在任务管理器结束 vibe-voice-lite.exe 后再启动。"
            } else {
                "无法确认单实例状态，已停止启动。"
            };
            let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "Vibe Voice Lite\0".encode_utf16().collect();
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            return None;
        }
        Some(Self(handle))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
