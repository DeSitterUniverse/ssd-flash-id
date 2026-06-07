#[cfg(target_os = "linux")]
#[path = "ata/linux.rs"]
mod imp;
#[cfg(windows)]
#[path = "ata/windows.rs"]
mod imp;

pub use imp::AtaDevice;

pub struct AtaIdentify {
    pub model: String,
    pub serial: String,
    pub firmware: String,
}

pub fn parse_ata_identify(data: &[u8; 512]) -> AtaIdentify {
    let serial = ata_string_trim(&data[20..40]);
    let firmware = ata_string_trim(&data[46..54]);
    let model = ata_string_trim(&data[54..94]);
    AtaIdentify {
        model,
        serial,
        firmware,
    }
}

/// ATA strings store each 16-bit word with the high byte first (byte-swapped relative to host).
fn ata_string_trim(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    for pair in raw.chunks_exact(2) {
        out.push(pair[1]);
        out.push(pair[0]);
    }
    let s: String = out
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
