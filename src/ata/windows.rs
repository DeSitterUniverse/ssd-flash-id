pub struct AtaDevice;

#[allow(clippy::too_many_arguments)]
impl AtaDevice {
    pub fn open(path: &str) -> Result<Self, String> {
        Err(format!(
            "Windows ATA transport is not implemented for '{}'",
            path
        ))
    }

    pub fn ata_identify(&self) -> Result<[u8; 512], String> {
        Err("Windows ATA transport is not implemented".into())
    }

    pub fn ata_read(
        &self,
        _command: u8,
        _features: u8,
        _count: u8,
        _lba_low: u8,
        _lba_mid: u8,
        _lba_high: u8,
        _device: u8,
        _buf: &mut [u8],
    ) -> Result<(), String> {
        Err("Windows ATA transport is not implemented".into())
    }

    pub fn ata_dma_read(
        &self,
        _command: u8,
        _features: u8,
        _count: u8,
        _lba_low: u8,
        _lba_mid: u8,
        _lba_high: u8,
        _device: u8,
        _buf: &mut [u8],
    ) -> Result<(), String> {
        Err("Windows ATA transport is not implemented".into())
    }

    pub fn ata_write(
        &self,
        _command: u8,
        _features: u8,
        _count: u8,
        _lba_low: u8,
        _lba_mid: u8,
        _lba_high: u8,
        _device: u8,
        _buf: &[u8],
    ) -> Result<(), String> {
        Err("Windows ATA transport is not implemented".into())
    }

    pub fn ata_no_data(
        &self,
        _command: u8,
        _features: u8,
        _count: u8,
        _lba_low: u8,
        _lba_mid: u8,
        _lba_high: u8,
        _device: u8,
    ) -> Result<(), String> {
        Err("Windows ATA transport is not implemented".into())
    }

    pub fn ata_read_ext(
        &self,
        _command: u8,
        _features: u8,
        _count: u8,
        _lba_low: u8,
        _lba_mid: u8,
        _lba_high: u8,
        _device: u8,
        _prev_features: u8,
        _prev_count: u8,
        _prev_lba_low: u8,
        _prev_lba_mid: u8,
        _prev_lba_high: u8,
        _buf: &mut [u8],
    ) -> Result<(), String> {
        Err("Windows ATA transport is not implemented".into())
    }

    pub fn ata_no_data_ext(
        &self,
        _command: u8,
        _features: u8,
        _count: u8,
        _lba_low: u8,
        _lba_mid: u8,
        _lba_high: u8,
        _device: u8,
        _prev_features: u8,
        _prev_count: u8,
        _prev_lba_low: u8,
        _prev_lba_mid: u8,
        _prev_lba_high: u8,
    ) -> Result<(), String> {
        Err("Windows ATA transport is not implemented".into())
    }
}
