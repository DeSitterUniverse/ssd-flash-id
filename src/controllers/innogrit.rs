use crate::controllers::{FlashBank, FlashIdResult, is_bank_empty};
use crate::nvme::NvmeDevice;

const GRIT_MAGIC: u32 = 0x54495247;
const INNO_MAGIC: u32 = 0x4F4E4E49;

const DID_OFFSET: usize = 0x62E;
const FID_OFFSET_5208: usize = 0x548;
const FID_OFFSET_5220: usize = 0x24E;
const MAX_BANKS_5208: usize = 32;
const MAX_BANKS_5220: usize = 64;
const FID_ENTRY_SIZE: usize = 6;

pub fn read_flash_id(dev: &NvmeDevice) -> Result<FlashIdResult, String> {
    let mut buf = [0u8; 4096];
    // "GRIT" and "INNO" are the controller's required signature dwords in
    // CDW14/CDW15 for the 0xF2 identification page.
    dev.admin_read(0xF2, 0, 0x400, 0, 0, 0, GRIT_MAGIC, INNO_MAGIC, &mut buf)
        .map_err(|e| format!("Innogrit vendor command failed: {}", e))?;

    Ok(parse_flash_id_response(&buf))
}

fn parse_flash_id_response(buf: &[u8; 4096]) -> FlashIdResult {
    let did = u16::from_le_bytes([buf[DID_OFFSET], buf[DID_OFFSET + 1]]);
    let ctrl_name = format!("IG{did:04X}");

    // IG5208/IG5216 use an older 32-bank table; later controllers use the
    // 64-entry layout beginning at 0x24E.
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

    FlashIdResult {
        controller_name: ctrl_name,
        banks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_controller_did_as_four_digit_hex() {
        let mut buf = [0u8; 4096];
        buf[DID_OFFSET..DID_OFFSET + 2].copy_from_slice(&0x5236u16.to_le_bytes());
        buf[FID_OFFSET_5220..FID_OFFSET_5220 + FID_ENTRY_SIZE]
            .copy_from_slice(&[0x2C, 0xC3, 0x08, 0x32, 0xEA, 0x30]);

        let result = parse_flash_id_response(&buf);

        assert_eq!(result.controller_name, "IG5236");
        assert_eq!(result.banks.len(), 1);
        assert_eq!(result.banks[0].bank_num, 0);
        assert_eq!(
            result.banks[0].flash_id,
            [0x2C, 0xC3, 0x08, 0x32, 0xEA, 0x30, 0, 0]
        );
    }
}
