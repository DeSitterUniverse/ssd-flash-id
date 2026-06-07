pub struct NvmeDevice;

#[allow(clippy::too_many_arguments)]
impl NvmeDevice {
    pub fn open(path: &str) -> Result<Self, String> {
        Err(format!(
            "Windows NVMe transport is not implemented for '{}'",
            path
        ))
    }

    pub fn admin_read(
        &self,
        _opcode: u8,
        _nsid: u32,
        _cdw10: u32,
        _cdw11: u32,
        _cdw12: u32,
        _cdw13: u32,
        _cdw14: u32,
        _cdw15: u32,
        _buf: &mut [u8],
    ) -> Result<u32, String> {
        Err("Windows NVMe transport is not implemented".into())
    }

    pub fn admin_write(
        &self,
        _opcode: u8,
        _nsid: u32,
        _cdw10: u32,
        _cdw11: u32,
        _cdw12: u32,
        _cdw13: u32,
        _cdw14: u32,
        _cdw15: u32,
        _buf: &[u8],
    ) -> Result<u32, String> {
        Err("Windows NVMe transport is not implemented".into())
    }

    pub fn admin_no_data(
        &self,
        _opcode: u8,
        _nsid: u32,
        _cdw10: u32,
        _cdw11: u32,
        _cdw12: u32,
        _cdw13: u32,
        _cdw14: u32,
        _cdw15: u32,
    ) -> Result<u32, String> {
        Err("Windows NVMe transport is not implemented".into())
    }

    pub fn identify_controller(&self) -> Result<[u8; 4096], String> {
        Err("Windows NVMe transport is not implemented".into())
    }
}
