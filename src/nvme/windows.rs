use std::ffi::c_void;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const IOCTL_STORAGE_PROTOCOL_COMMAND: u32 = 0x002D_0C00;
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const STORAGE_PROTOCOL_STRUCTURE_VERSION: u32 = 1;
const STORAGE_PROTOCOL_COMMAND_SIZE: u32 = 84;
const PROTOCOL_TYPE_NVME: u32 = 3;
const STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST: u32 = 0x8000_0000;
const STORAGE_PROTOCOL_STATUS_SUCCESS: u32 = 1;
const STORAGE_PROTOCOL_SPECIFIC_NVME_ADMIN_COMMAND: u32 = 1;
const NVME_COMMAND_LEN: usize = 64;
const PROTOCOL_HEADER_LEN: usize = 80;
#[cfg(test)]
const DEFAULT_TIMEOUT_SECONDS: u32 = 10;
const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;
const NVME_DATA_TYPE_IDENTIFY: u32 = 1;
const PROTOCOL_SPECIFIC_DATA_LEN: usize = 40;
const PROTOCOL_DATA_DESCRIPTOR_PREFIX_LEN: usize = 8;
const IDENTIFY_DATA_LEN: usize = 4096;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct StorageProtocolCommand {
    version: u32,
    length: u32,
    protocol_type: u32,
    flags: u32,
    return_status: u32,
    error_code: u32,
    command_length: u32,
    error_info_length: u32,
    data_to_device_transfer_length: u32,
    data_from_device_transfer_length: u32,
    timeout_value: u32,
    error_info_offset: u32,
    data_to_device_buffer_offset: u32,
    data_from_device_buffer_offset: u32,
    command_specific: u32,
    reserved0: u32,
    fixed_protocol_return_data: u32,
    fixed_protocol_return_data2: u32,
    reserved1: [u32; 2],
}

#[derive(Clone, Copy)]
enum DataDirection {
    None,
    Read,
    Write,
}

struct AlignedBuffer {
    words: Vec<usize>,
    len: usize,
}

impl AlignedBuffer {
    fn zeroed(len: usize) -> Self {
        let word_size = size_of::<usize>();
        let word_count = len.div_ceil(word_size);
        Self {
            words: vec![0; word_count],
            len,
        }
    }

    fn as_ptr(&self) -> *const u8 {
        self.words.as_ptr().cast()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.words.as_mut_ptr().cast()
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len) }
    }

    fn as_mut_bytes(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.as_mut_ptr(), self.len) }
    }

    fn header(&self) -> &StorageProtocolCommand {
        unsafe { &*self.as_ptr().cast::<StorageProtocolCommand>() }
    }

    fn header_mut(&mut self) -> &mut StorageProtocolCommand {
        unsafe { &mut *self.as_mut_ptr().cast::<StorageProtocolCommand>() }
    }
}

fn build_protocol_command(
    direction: DataDirection,
    opcode: u8,
    nsid: u32,
    cdws: [u32; 6],
    data_len: usize,
    timeout_seconds: u32,
) -> AlignedBuffer {
    let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN;
    let mut packet = AlignedBuffer::zeroed(data_offset + data_len);

    *packet.header_mut() = StorageProtocolCommand {
        version: STORAGE_PROTOCOL_STRUCTURE_VERSION,
        length: STORAGE_PROTOCOL_COMMAND_SIZE,
        protocol_type: PROTOCOL_TYPE_NVME,
        flags: STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST,
        command_length: NVME_COMMAND_LEN as u32,
        timeout_value: timeout_seconds,
        command_specific: STORAGE_PROTOCOL_SPECIFIC_NVME_ADMIN_COMMAND,
        ..StorageProtocolCommand::default()
    };

    match direction {
        DataDirection::None => {}
        DataDirection::Read => {
            let header = packet.header_mut();
            header.data_from_device_transfer_length = data_len as u32;
            header.data_from_device_buffer_offset = data_offset as u32;
        }
        DataDirection::Write => {
            let header = packet.header_mut();
            header.data_to_device_transfer_length = data_len as u32;
            header.data_to_device_buffer_offset = data_offset as u32;
        }
    }

    let command = &mut packet.as_mut_bytes()[PROTOCOL_HEADER_LEN..data_offset];
    command[0] = opcode;
    command[4..8].copy_from_slice(&nsid.to_le_bytes());
    for (index, value) in cdws.into_iter().enumerate() {
        let offset = 40 + index * 4;
        command[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    packet
}

fn build_identify_query() -> AlignedBuffer {
    let data_offset = PROTOCOL_DATA_DESCRIPTOR_PREFIX_LEN + PROTOCOL_SPECIFIC_DATA_LEN;
    let mut packet = AlignedBuffer::zeroed(data_offset + IDENTIFY_DATA_LEN);
    let bytes = packet.as_mut_bytes();

    bytes[0..4].copy_from_slice(&STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY.to_le_bytes());
    bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&PROTOCOL_TYPE_NVME.to_le_bytes());
    bytes[12..16].copy_from_slice(&NVME_DATA_TYPE_IDENTIFY.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&(PROTOCOL_SPECIFIC_DATA_LEN as u32).to_le_bytes());
    bytes[28..32].copy_from_slice(&(IDENTIFY_DATA_LEN as u32).to_le_bytes());
    packet
}

pub struct NvmeDevice {
    handle: HANDLE,
    timeout_seconds: u32,
}

#[allow(clippy::too_many_arguments)]
impl NvmeDevice {
    pub fn open_with_timeout(path: &str, timeout_seconds: u32) -> Result<Self, String> {
        let wide_path: Vec<u16> = Path::new(path)
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "failed to open '{}': Windows error {}",
                path,
                unsafe { GetLastError() }
            ));
        }
        Ok(Self {
            handle,
            timeout_seconds,
        })
    }

    pub fn admin_read(
        &self,
        opcode: u8,
        nsid: u32,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
        cdw14: u32,
        cdw15: u32,
        buf: &mut [u8],
    ) -> Result<u32, String> {
        let mut packet = build_protocol_command(
            DataDirection::Read,
            opcode,
            nsid,
            [cdw10, cdw11, cdw12, cdw13, cdw14, cdw15],
            buf.len(),
            self.timeout_seconds,
        );
        let result = self.submit(&mut packet, opcode)?;
        let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN;
        buf.copy_from_slice(&packet.as_bytes()[data_offset..data_offset + buf.len()]);
        Ok(result)
    }

    pub fn admin_write(
        &self,
        opcode: u8,
        nsid: u32,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
        cdw14: u32,
        cdw15: u32,
        buf: &[u8],
    ) -> Result<u32, String> {
        let mut packet = build_protocol_command(
            DataDirection::Write,
            opcode,
            nsid,
            [cdw10, cdw11, cdw12, cdw13, cdw14, cdw15],
            buf.len(),
            self.timeout_seconds,
        );
        let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN;
        packet.as_mut_bytes()[data_offset..data_offset + buf.len()].copy_from_slice(buf);
        self.submit(&mut packet, opcode)
    }

    pub fn admin_no_data(
        &self,
        opcode: u8,
        nsid: u32,
        cdw10: u32,
        cdw11: u32,
        cdw12: u32,
        cdw13: u32,
        cdw14: u32,
        cdw15: u32,
    ) -> Result<u32, String> {
        let mut packet = build_protocol_command(
            DataDirection::None,
            opcode,
            nsid,
            [cdw10, cdw11, cdw12, cdw13, cdw14, cdw15],
            0,
            self.timeout_seconds,
        );
        self.submit(&mut packet, opcode)
    }

    pub fn identify_controller(&self) -> Result<[u8; 4096], String> {
        let mut packet = build_identify_query();
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                packet.as_ptr().cast::<c_void>(),
                packet.len as u32,
                packet.as_mut_ptr().cast::<c_void>(),
                packet.len as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "NVMe identify query failed: Windows error {}",
                unsafe { GetLastError() }
            ));
        }

        let bytes = packet.as_bytes();
        let protocol_offset = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
        let protocol_length = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize;
        let data_offset = PROTOCOL_DATA_DESCRIPTOR_PREFIX_LEN + protocol_offset;
        if protocol_length < IDENTIFY_DATA_LEN || data_offset + IDENTIFY_DATA_LEN > bytes.len() {
            return Err("NVMe identify query returned an invalid data buffer".into());
        }

        let mut identify = [0u8; IDENTIFY_DATA_LEN];
        identify.copy_from_slice(&bytes[data_offset..data_offset + IDENTIFY_DATA_LEN]);
        Ok(identify)
    }

    fn submit(&self, packet: &mut AlignedBuffer, opcode: u8) -> Result<u32, String> {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_STORAGE_PROTOCOL_COMMAND,
                packet.as_ptr().cast::<c_void>(),
                packet.len as u32,
                packet.as_mut_ptr().cast::<c_void>(),
                packet.len as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "NVMe pass-through failed: Windows error {} (opcode 0x{:02x})",
                unsafe { GetLastError() },
                opcode
            ));
        }

        let header = packet.header();
        if header.return_status != STORAGE_PROTOCOL_STATUS_SUCCESS {
            return Err(format!(
                "NVMe command rejected: return status 0x{:x}, NVMe status 0x{:x} (opcode 0x{:02x})",
                header.return_status, header.error_code, opcode
            ));
        }
        Ok(header.fixed_protocol_return_data)
    }
}

impl Drop for NvmeDevice {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_nvme_admin_command_dwords() {
        let packet = build_protocol_command(
            DataDirection::Read,
            0xE2,
            0x1122_3344,
            [0x10, 0x11, 0x12, 0x13, 0x14, 0x15],
            4096,
            DEFAULT_TIMEOUT_SECONDS,
        );
        let bytes = packet.as_bytes();
        let command = &bytes[PROTOCOL_HEADER_LEN..PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN];

        assert_eq!(command[0], 0xE2);
        assert_eq!(&command[4..8], &0x1122_3344u32.to_le_bytes());
        for (index, value) in [0x10u32, 0x11, 0x12, 0x13, 0x14, 0x15]
            .into_iter()
            .enumerate()
        {
            let offset = 40 + index * 4;
            assert_eq!(&command[offset..offset + 4], &value.to_le_bytes());
        }
    }

    #[test]
    fn read_packet_places_data_after_aligned_command() {
        let packet = build_protocol_command(
            DataDirection::Read,
            0x06,
            0,
            [1, 0, 0, 0, 0, 0],
            4096,
            DEFAULT_TIMEOUT_SECONDS,
        );
        let header = packet.header();

        assert_eq!(packet.as_ptr() as usize % std::mem::align_of::<usize>(), 0);
        assert_eq!(header.command_length, NVME_COMMAND_LEN as u32);
        assert_eq!(
            header.data_from_device_buffer_offset,
            (PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN) as u32
        );
        assert_eq!(header.data_from_device_transfer_length, 4096);
        assert_eq!(header.data_to_device_transfer_length, 0);
    }

    #[test]
    fn identify_query_requests_controller_data() {
        let packet = build_identify_query();
        let bytes = packet.as_bytes();

        assert_eq!(&bytes[0..4], &50u32.to_le_bytes());
        assert_eq!(&bytes[8..12], &PROTOCOL_TYPE_NVME.to_le_bytes());
        assert_eq!(&bytes[12..16], &1u32.to_le_bytes());
        assert_eq!(&bytes[16..20], &1u32.to_le_bytes());
        assert_eq!(&bytes[24..28], &40u32.to_le_bytes());
        assert_eq!(&bytes[28..32], &4096u32.to_le_bytes());
    }

    #[test]
    fn protocol_packet_uses_requested_timeout() {
        let packet = build_protocol_command(DataDirection::None, 0xC1, 0, [0; 6], 0, 37);
        assert_eq!(packet.header().timeout_value, 37);
    }
}
