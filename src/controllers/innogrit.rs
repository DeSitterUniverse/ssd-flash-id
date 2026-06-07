use crate::controllers::{is_bank_empty, FlashBank, FlashIdResult};
use crate::nvme::NvmeDevice;

const GRIT_MAGIC: u32 = 0x54495247;
const INNO_MAGIC: u32 = 0x4F4E4E49;

const DID_OFFSET: usize = 0x62E;
const FID_OFFSET_5208: usize = 0x548;
const FID_OFFSET_5220: usize = 0x24E;
const MAX_BANKS_5208: usize = 32;
const MAX_BANKS_5220: usize = 64;
const FID_ENTRY_SIZE: usize = 6;
const INFO_LOG_ID: u8 = 0xE1;
const INFO_LOG_SIGNATURE: u8 = 0x5A;
const INFO_LOG_DID_OFFSET: usize = 2;
const INFO_LOG_FID_OFFSET: usize = 16;

fn parse_info_log(buf: &[u8; 512]) -> Result<FlashIdResult, String> {
    if buf[0] != INFO_LOG_SIGNATURE {
        return Err("Innogrit info log has an invalid signature".to_string());
    }

    let did = u16::from_le_bytes([
        buf[INFO_LOG_DID_OFFSET],
        buf[INFO_LOG_DID_OFFSET + 1],
    ]);
    let entry = &buf[INFO_LOG_FID_OFFSET..INFO_LOG_FID_OFFSET + FID_ENTRY_SIZE];
    if is_bank_empty(entry) {
        return Err("Innogrit info log did not contain a NAND flash ID".to_string());
    }

    let mut flash_id = [0u8; 8];
    flash_id[..FID_ENTRY_SIZE].copy_from_slice(entry);
    Ok(FlashIdResult {
        controller_name: format!("IG{did:04X}"),
        banks: vec![FlashBank {
            bank_num: 0,
            flash_id,
        }],
    })
}

fn read_info_log(dev: &NvmeDevice, command_error: &str) -> Result<FlashIdResult, String> {
    let info = dev
        .get_log_page::<512>(INFO_LOG_ID)
        .map_err(|log_error| format!("{command_error}; info-log fallback failed: {log_error}"))?;
    parse_info_log(&info)
        .map_err(|log_error| format!("{command_error}; info-log fallback failed: {log_error}"))
}

pub fn read_flash_id(dev: &NvmeDevice) -> Result<FlashIdResult, String> {
    #[cfg(windows)]
    if !dev.standard_admin_vendor_format_supported()? {
        return read_info_log(
            dev,
            "the controller reports AVSCC.CommandFormatInSpec=0, so Microsoft StorNVMe cannot map this proprietary Innogrit admin command's data buffer",
        );
    }

    let mut buf = [0u8; 4096];
    if let Err(command_error) = dev.admin_read(
        0xF2, 0, 0x400, 0, 0, 0, GRIT_MAGIC, INNO_MAGIC, &mut buf,
    ) {
        return read_info_log(
            dev,
            &format!("Innogrit vendor command failed: {command_error}"),
        );
    }

    let did = u16::from_le_bytes([buf[DID_OFFSET], buf[DID_OFFSET + 1]]);
    let ctrl_name = format!("IG{}", did);

    let (fid_offset, max_banks) = match did {
        0x5208 | 0x5216 => (FID_OFFSET_5208, MAX_BANKS_5208),
        _ => (FID_OFFSET_5220, MAX_BANKS_5220),
    };

    let mut banks = Vec::new();
    for i in 0..max_banks {
        let offset = fid_offset + i * FID_ENTRY_SIZE;
        if offset + FID_ENTRY_SIZE > buf.len() {
            break;
        }
        let entry = &buf[offset..offset + FID_ENTRY_SIZE];
        if is_bank_empty(entry) {
            continue;
        }
        let mut flash_id = [0u8; 8];
        flash_id[..FID_ENTRY_SIZE].copy_from_slice(entry);
        banks.push(FlashBank {
            bank_num: i as u32,
            flash_id,
        });
    }

    Ok(FlashIdResult {
        controller_name: ctrl_name,
        banks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_innogrit_info_log() {
        let mut log = [0u8; 512];
        log[0] = INFO_LOG_SIGNATURE;
        log[INFO_LOG_DID_OFFSET..INFO_LOG_DID_OFFSET + 2]
            .copy_from_slice(&0x5236u16.to_le_bytes());
        log[INFO_LOG_FID_OFFSET..INFO_LOG_FID_OFFSET + FID_ENTRY_SIZE]
            .copy_from_slice(&[0x2C, 0xC3, 0x08, 0x32, 0xAA, 0x00]);

        let result = parse_info_log(&log).unwrap();

        assert_eq!(result.controller_name, "IG5236");
        assert_eq!(result.banks.len(), 1);
        assert_eq!(
            result.banks[0].flash_id,
            [0x2C, 0xC3, 0x08, 0x32, 0xAA, 0x00, 0x00, 0x00]
        );
    }
}
