#[cfg(target_os = "linux")]
#[path = "nvme/linux.rs"]
mod imp;
#[cfg(windows)]
#[path = "nvme/windows.rs"]
mod imp;

pub use imp::NvmeDevice;

#[cfg(windows)]
pub fn admin_vendor_command_format_in_spec(data: &[u8; 4096]) -> bool {
    data[264] & 1 != 0
}

pub struct ControllerInfo {
    pub vid: u16,
    pub ssvid: u16,
    pub serial: String,
    pub model: String,
    pub firmware: String,
}

pub fn parse_identify(data: &[u8; 4096]) -> ControllerInfo {
    let vid = u16::from_le_bytes([data[0], data[1]]);
    let ssvid = u16::from_le_bytes([data[2], data[3]]);
    let serial = ascii_trim(&data[4..24]);
    let model = ascii_trim(&data[24..64]);
    let firmware = ascii_trim(&data[64..72]);
    ControllerInfo {
        vid,
        ssvid,
        serial,
        model,
        firmware,
    }
}

fn ascii_trim(bytes: &[u8]) -> String {
    let s: String = bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                ' '
            }
        })
        .collect();
    s.trim().to_string()
}
