use std::ptr::null_mut;

use windows_sys::Win32::System::Diagnostics::Debug::{
    FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS, FormatMessageW,
};

pub fn format_windows_error(code: u32) -> String {
    let mut buffer = [0u16; 2048];
    let length = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            null_mut(),
            code,
            0,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            null_mut(),
        )
    };

    if length == 0 {
        return format!("Windows error {}", code);
    }

    let message = String::from_utf16_lossy(&buffer[..length as usize]);
    format!("Windows error {}: {}", code, message.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_windows_error_code_with_system_message() {
        let message = format_windows_error(5);

        assert!(message.contains("Windows error 5"));
        assert!(message.len() > "Windows error 5".len());
    }
}
