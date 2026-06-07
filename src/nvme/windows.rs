use std::cell::RefCell;
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

use crate::windows::format_windows_error;

const IOCTL_STORAGE_PROTOCOL_COMMAND: u32 = 0x002D_D3C0;
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const STORAGE_PROTOCOL_STRUCTURE_VERSION: u32 = 1;
const PROTOCOL_TYPE_NVME: u32 = 3;
const STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST: u32 = 0x8000_0000;
const STORAGE_PROTOCOL_STATUS_SUCCESS: u32 = 1;
const STORAGE_PROTOCOL_SPECIFIC_NVME_ADMIN_COMMAND: u32 = 1;
const NVME_COMMAND_LEN: usize = 64;
const NVME_ERROR_INFO_LEN: usize = 64;
const PROTOCOL_HEADER_LEN: usize = 80;
#[cfg(test)]
const DEFAULT_TIMEOUT_SECONDS: u32 = 10;
const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;
const NVME_DATA_TYPE_IDENTIFY: u32 = 1;
const NVME_DATA_TYPE_LOG_PAGE: u32 = 2;
const COMMAND_EFFECTS_LOG_ID: u32 = 5;
const COMMAND_EFFECTS_LOG_LEN: usize = 4096;
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
    let error_info_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN;
    let data_offset = error_info_offset + NVME_ERROR_INFO_LEN;
    let mut packet = AlignedBuffer::zeroed(data_offset + data_len);

    *packet.header_mut() = StorageProtocolCommand {
        version: STORAGE_PROTOCOL_STRUCTURE_VERSION,
        length: size_of::<StorageProtocolCommand>() as u32,
        protocol_type: PROTOCOL_TYPE_NVME,
        flags: STORAGE_PROTOCOL_COMMAND_FLAG_ADAPTER_REQUEST,
        command_length: NVME_COMMAND_LEN as u32,
        error_info_length: NVME_ERROR_INFO_LEN as u32,
        error_info_offset: error_info_offset as u32,
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

    let command_end = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN;
    let command = &mut packet.as_mut_bytes()[PROTOCOL_HEADER_LEN..command_end];
    command[0] = opcode;
    command[4..8].copy_from_slice(&nsid.to_le_bytes());
    for (index, value) in cdws.into_iter().enumerate() {
        let offset = 40 + index * 4;
        command[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    packet
}

fn build_protocol_query(data_type: u32, request_value: u32, data_len: usize) -> AlignedBuffer {
    let data_offset = PROTOCOL_DATA_DESCRIPTOR_PREFIX_LEN + PROTOCOL_SPECIFIC_DATA_LEN;
    let mut packet = AlignedBuffer::zeroed(data_offset + data_len);
    let bytes = packet.as_mut_bytes();

    bytes[0..4].copy_from_slice(&STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY.to_le_bytes());
    bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&PROTOCOL_TYPE_NVME.to_le_bytes());
    bytes[12..16].copy_from_slice(&data_type.to_le_bytes());
    bytes[16..20].copy_from_slice(&request_value.to_le_bytes());
    bytes[24..28].copy_from_slice(&(PROTOCOL_SPECIFIC_DATA_LEN as u32).to_le_bytes());
    bytes[28..32].copy_from_slice(&(data_len as u32).to_le_bytes());
    packet
}

fn build_identify_query() -> AlignedBuffer {
    build_protocol_query(NVME_DATA_TYPE_IDENTIFY, 1, IDENTIFY_DATA_LEN)
}

fn build_command_effects_query() -> AlignedBuffer {
    build_protocol_query(
        NVME_DATA_TYPE_LOG_PAGE,
        COMMAND_EFFECTS_LOG_ID,
        COMMAND_EFFECTS_LOG_LEN,
    )
}

fn admin_command_supported(log: &[u8; COMMAND_EFFECTS_LOG_LEN], opcode: u8) -> bool {
    let offset = opcode as usize * size_of::<u32>();
    u32::from_le_bytes(log[offset..offset + 4].try_into().unwrap()) & 1 != 0
}

fn validate_protocol_response(
    packet: &AlignedBuffer,
    returned: u32,
    opcode: u8,
) -> Result<u32, String> {
    if returned as usize > packet.len || (returned as usize) < PROTOCOL_HEADER_LEN {
        return Err(format!(
            "NVMe pass-through returned an invalid byte count {} (opcode 0x{:02x})",
            returned, opcode
        ));
    }

    let header = packet.header();
    if header.return_status != STORAGE_PROTOCOL_STATUS_SUCCESS {
        return Err(format!(
            "NVMe command rejected: return status 0x{:x}, NVMe status 0x{:x} (opcode 0x{:02x})",
            header.return_status, header.error_code, opcode
        ));
    }

    let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN + NVME_ERROR_INFO_LEN;
    let expected_data_len = packet.len.saturating_sub(data_offset);
    if header.data_from_device_transfer_length != 0 {
        if header.data_from_device_transfer_length as usize != expected_data_len {
            return Err(format!(
                "NVMe command returned {} of {} requested bytes (opcode 0x{:02x})",
                header.data_from_device_transfer_length, expected_data_len, opcode
            ));
        }
        if (returned as usize) < data_offset + expected_data_len {
            return Err(format!(
                "NVMe pass-through response ended before its data buffer (opcode 0x{:02x})",
                opcode
            ));
        }
    }

    Ok(header.fixed_protocol_return_data)
}

fn copy_protocol_read_data(
    packet: &AlignedBuffer,
    returned: u32,
    opcode: u8,
    output: &mut [u8],
) -> Result<u32, String> {
    let result = validate_protocol_response(packet, returned, opcode)?;
    let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN + NVME_ERROR_INFO_LEN;
    output.copy_from_slice(&packet.as_bytes()[data_offset..data_offset + output.len()]);
    Ok(result)
}

fn extract_identify_response(
    packet: &AlignedBuffer,
    returned: u32,
) -> Result<[u8; IDENTIFY_DATA_LEN], String> {
    extract_protocol_data(packet, returned, "NVMe identify query")
}

fn extract_protocol_data<const N: usize>(
    packet: &AlignedBuffer,
    returned: u32,
    description: &str,
) -> Result<[u8; N], String> {
    if returned as usize > packet.len
        || (returned as usize) < PROTOCOL_DATA_DESCRIPTOR_PREFIX_LEN
    {
        return Err(format!("{} returned an invalid byte count", description));
    }

    let bytes = packet.as_bytes();
    let protocol_offset = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
    let protocol_length = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize;
    let data_offset = PROTOCOL_DATA_DESCRIPTOR_PREFIX_LEN
        .checked_add(protocol_offset)
        .ok_or_else(|| format!("{} returned an invalid data offset", description))?;
    let data_end = data_offset
        .checked_add(N)
        .ok_or_else(|| format!("{} returned an invalid data length", description))?;
    if protocol_length < N || data_end > bytes.len() || data_end > returned as usize {
        return Err(format!("{} returned an invalid data buffer", description));
    }

    let mut data = [0u8; N];
    data.copy_from_slice(&bytes[data_offset..data_end]);
    Ok(data)
}

pub struct NvmeDevice {
    handle: HANDLE,
    timeout_seconds: u32,
    command_effects: RefCell<Option<[u8; COMMAND_EFFECTS_LOG_LEN]>>,
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
                "failed to open '{}': {}",
                path,
                format_windows_error(unsafe { GetLastError() })
            ));
        }
        Ok(Self {
            handle,
            timeout_seconds,
            command_effects: RefCell::new(None),
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
        self.ensure_admin_command_supported(opcode)?;
        let mut packet = build_protocol_command(
            DataDirection::Read,
            opcode,
            nsid,
            [cdw10, cdw11, cdw12, cdw13, cdw14, cdw15],
            buf.len(),
            self.timeout_seconds,
        );
        let (_, returned) = self.submit(&mut packet, opcode)?;
        copy_protocol_read_data(&packet, returned, opcode, buf)
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
        self.ensure_admin_command_supported(opcode)?;
        let mut packet = build_protocol_command(
            DataDirection::Write,
            opcode,
            nsid,
            [cdw10, cdw11, cdw12, cdw13, cdw14, cdw15],
            buf.len(),
            self.timeout_seconds,
        );
        let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN + NVME_ERROR_INFO_LEN;
        packet.as_mut_bytes()[data_offset..data_offset + buf.len()].copy_from_slice(buf);
        self.submit(&mut packet, opcode).map(|(result, _)| result)
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
        self.ensure_admin_command_supported(opcode)?;
        let mut packet = build_protocol_command(
            DataDirection::None,
            opcode,
            nsid,
            [cdw10, cdw11, cdw12, cdw13, cdw14, cdw15],
            0,
            self.timeout_seconds,
        );
        self.submit(&mut packet, opcode).map(|(result, _)| result)
    }

    pub fn identify_controller(&self) -> Result<[u8; 4096], String> {
        let mut packet = build_identify_query();
        let returned = self.submit_query(&mut packet, "NVMe identify query")?;
        extract_identify_response(&packet, returned)
    }

    fn ensure_admin_command_supported(&self, opcode: u8) -> Result<(), String> {
        if self.command_effects.borrow().is_none() {
            let mut packet = build_command_effects_query();
            let returned = self.submit_query(&mut packet, "NVMe command-effects query")?;
            let log = extract_protocol_data(
                &packet,
                returned,
                "NVMe command-effects query",
            )?;
            *self.command_effects.borrow_mut() = Some(log);
        }

        let supported = self
            .command_effects
            .borrow()
            .as_ref()
            .is_some_and(|log| admin_command_supported(log, opcode));
        if supported {
            Ok(())
        } else {
            Err(format!(
                "Windows NVMe driver will reject opcode 0x{:02x}: the drive does not mark it supported in Command Effects Log page 0x05",
                opcode
            ))
        }
    }

    fn submit_query(&self, packet: &mut AlignedBuffer, description: &str) -> Result<u32, String> {
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
                "{} failed: {}",
                description,
                format_windows_error(unsafe { GetLastError() })
            ));
        }
        Ok(returned)
    }

    fn submit(&self, packet: &mut AlignedBuffer, opcode: u8) -> Result<(u32, u32), String> {
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
                "NVMe pass-through failed: {} (opcode 0x{:02x})",
                format_windows_error(unsafe { GetLastError() }),
                opcode
            ));
        }

        validate_protocol_response(packet, returned, opcode).map(|result| (result, returned))
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
        assert_eq!(header.error_info_length, NVME_ERROR_INFO_LEN as u32);
        assert_eq!(
            header.error_info_offset,
            (PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN) as u32
        );
        assert_eq!(
            header.data_from_device_buffer_offset,
            (PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN + NVME_ERROR_INFO_LEN) as u32
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

    #[test]
    fn simulated_nvme_response_rejects_short_and_partial_transfers() {
        let mut packet = build_protocol_command(
            DataDirection::Read,
            0xC1,
            0,
            [0; 6],
            512,
            DEFAULT_TIMEOUT_SECONDS,
        );
        packet.header_mut().return_status = STORAGE_PROTOCOL_STATUS_SUCCESS;

        assert!(validate_protocol_response(&packet, 40, 0xC1).is_err());

        packet.header_mut().data_from_device_transfer_length = 256;
        assert!(validate_protocol_response(&packet, packet.len as u32, 0xC1).is_err());
    }

    #[test]
    fn simulated_identify_response_rejects_malformed_offset() {
        let mut packet = build_identify_query();
        packet.as_mut_bytes()[24..28].copy_from_slice(&u32::MAX.to_le_bytes());

        assert!(extract_identify_response(&packet, packet.len as u32).is_err());
    }

    #[test]
    fn simulated_nvme_response_copies_valid_read_data() {
        let mut packet = build_protocol_command(
            DataDirection::Read,
            0xC1,
            0,
            [0; 6],
            4,
            DEFAULT_TIMEOUT_SECONDS,
        );
        packet.header_mut().return_status = STORAGE_PROTOCOL_STATUS_SUCCESS;
        let data_offset = PROTOCOL_HEADER_LEN + NVME_COMMAND_LEN + NVME_ERROR_INFO_LEN;
        packet.as_mut_bytes()[data_offset..data_offset + 4].copy_from_slice(&[1, 2, 3, 4]);
        let mut output = [0u8; 4];

        copy_protocol_read_data(&packet, packet.len as u32, 0xC1, &mut output).unwrap();

        assert_eq!(output, [1, 2, 3, 4]);
    }

    #[test]
    fn command_effects_log_marks_supported_admin_opcodes() {
        let mut log = [0u8; COMMAND_EFFECTS_LOG_LEN];
        log[0xC2 * 4..0xC2 * 4 + 4].copy_from_slice(&1u32.to_le_bytes());

        assert!(admin_command_supported(&log, 0xC2));
        assert!(!admin_command_supported(&log, 0xD2));
    }

    #[test]
    fn command_effects_query_requests_log_page_five() {
        let packet = build_command_effects_query();
        let bytes = packet.as_bytes();

        assert_eq!(&bytes[12..16], &NVME_DATA_TYPE_LOG_PAGE.to_le_bytes());
        assert_eq!(&bytes[16..20], &COMMAND_EFFECTS_LOG_ID.to_le_bytes());
        assert_eq!(
            &bytes[28..32],
            &(COMMAND_EFFECTS_LOG_LEN as u32).to_le_bytes()
        );
    }

    #[test]
    fn storage_protocol_ioctl_matches_windows_sdk() {
        assert_eq!(IOCTL_STORAGE_PROTOCOL_COMMAND, 0x002D_D3C0);
    }

    #[test]
    fn protocol_header_length_matches_windows_sdk_structure() {
        let packet = build_protocol_command(
            DataDirection::None,
            0xF2,
            0,
            [0; 6],
            0,
            DEFAULT_TIMEOUT_SECONDS,
        );

        assert_eq!(
            packet.header().length as usize,
            size_of::<StorageProtocolCommand>()
        );
    }
}
