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

const IOCTL_ATA_PASS_THROUGH: u32 = 0x0004_D02C;
const ATA_FLAGS_DATA_IN: u16 = 1 << 1;
const ATA_FLAGS_DATA_OUT: u16 = 1 << 2;
const ATA_FLAGS_48BIT_COMMAND: u16 = 1 << 3;
const ATA_FLAGS_USE_DMA: u16 = 1 << 4;
const ATA_STATUS_ERROR: u8 = 1;
const TIMEOUT_SECONDS: u32 = 10;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AtaPassThroughEx {
    length: u16,
    ata_flags: u16,
    path_id: u8,
    target_id: u8,
    lun: u8,
    reserved_as_uchar: u8,
    data_transfer_length: u32,
    timeout_value: u32,
    reserved_as_ulong: u32,
    data_buffer_offset: usize,
    previous_task_file: [u8; 8],
    current_task_file: [u8; 8],
}

#[derive(Clone, Copy)]
enum AtaDirection {
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
        let word_count = len.div_ceil(size_of::<usize>());
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

    fn header(&self) -> &AtaPassThroughEx {
        unsafe { &*self.as_ptr().cast::<AtaPassThroughEx>() }
    }

    fn header_mut(&mut self) -> &mut AtaPassThroughEx {
        unsafe { &mut *self.as_mut_ptr().cast::<AtaPassThroughEx>() }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_ata_packet(
    direction: AtaDirection,
    use_dma: bool,
    command: u8,
    features: u8,
    count: u8,
    lba_low: u8,
    lba_mid: u8,
    lba_high: u8,
    device: u8,
    previous: Option<[u8; 5]>,
    data_len: usize,
) -> AlignedBuffer {
    let header_len = size_of::<AtaPassThroughEx>();
    let mut packet = AlignedBuffer::zeroed(header_len + data_len);
    let mut flags = match direction {
        AtaDirection::None => 0,
        AtaDirection::Read => ATA_FLAGS_DATA_IN,
        AtaDirection::Write => ATA_FLAGS_DATA_OUT,
    };
    if use_dma {
        flags |= ATA_FLAGS_USE_DMA;
    }
    if previous.is_some() {
        flags |= ATA_FLAGS_48BIT_COMMAND;
    }

    let mut previous_task_file = [0u8; 8];
    if let Some(previous) = previous {
        previous_task_file[..5].copy_from_slice(&previous);
    }

    *packet.header_mut() = AtaPassThroughEx {
        length: header_len as u16,
        ata_flags: flags,
        data_transfer_length: data_len as u32,
        timeout_value: TIMEOUT_SECONDS,
        data_buffer_offset: if data_len == 0 { 0 } else { header_len },
        previous_task_file,
        current_task_file: [
            features, count, lba_low, lba_mid, lba_high, device, command, 0,
        ],
        ..AtaPassThroughEx::default()
    };
    packet
}

pub struct AtaDevice {
    handle: HANDLE,
}

#[allow(clippy::too_many_arguments)]
impl AtaDevice {
    pub fn open(path: &str) -> Result<Self, String> {
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
        Ok(Self { handle })
    }

    pub fn ata_identify(&self) -> Result<[u8; 512], String> {
        let mut buf = [0u8; 512];
        self.ata_read(0xEC, 0, 1, 0, 0, 0, 0xE0, &mut buf)?;
        Ok(buf)
    }

    pub fn ata_read(
        &self,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
        buf: &mut [u8],
    ) -> Result<(), String> {
        self.read(
            false, command, features, count, lba_low, lba_mid, lba_high, device, None, buf,
        )
    }

    pub fn ata_dma_read(
        &self,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
        buf: &mut [u8],
    ) -> Result<(), String> {
        self.read(
            true, command, features, count, lba_low, lba_mid, lba_high, device, None, buf,
        )
    }

    pub fn ata_write(
        &self,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
        buf: &[u8],
    ) -> Result<(), String> {
        let mut packet = build_ata_packet(
            AtaDirection::Write,
            false,
            command,
            features,
            count,
            lba_low,
            lba_mid,
            lba_high,
            device,
            None,
            buf.len(),
        );
        let offset = size_of::<AtaPassThroughEx>();
        packet.as_mut_bytes()[offset..offset + buf.len()].copy_from_slice(buf);
        self.submit(&mut packet, command)
    }

    pub fn ata_no_data(
        &self,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
    ) -> Result<(), String> {
        let mut packet = build_ata_packet(
            AtaDirection::None,
            false,
            command,
            features,
            count,
            lba_low,
            lba_mid,
            lba_high,
            device,
            None,
            0,
        );
        self.submit(&mut packet, command)
    }

    pub fn ata_read_ext(
        &self,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
        prev_features: u8,
        prev_count: u8,
        prev_lba_low: u8,
        prev_lba_mid: u8,
        prev_lba_high: u8,
        buf: &mut [u8],
    ) -> Result<(), String> {
        self.read(
            false,
            command,
            features,
            count,
            lba_low,
            lba_mid,
            lba_high,
            device,
            Some([
                prev_features,
                prev_count,
                prev_lba_low,
                prev_lba_mid,
                prev_lba_high,
            ]),
            buf,
        )
    }

    pub fn ata_no_data_ext(
        &self,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
        prev_features: u8,
        prev_count: u8,
        prev_lba_low: u8,
        prev_lba_mid: u8,
        prev_lba_high: u8,
    ) -> Result<(), String> {
        let mut packet = build_ata_packet(
            AtaDirection::None,
            false,
            command,
            features,
            count,
            lba_low,
            lba_mid,
            lba_high,
            device,
            Some([
                prev_features,
                prev_count,
                prev_lba_low,
                prev_lba_mid,
                prev_lba_high,
            ]),
            0,
        );
        self.submit(&mut packet, command)
    }

    #[allow(clippy::too_many_arguments)]
    fn read(
        &self,
        use_dma: bool,
        command: u8,
        features: u8,
        count: u8,
        lba_low: u8,
        lba_mid: u8,
        lba_high: u8,
        device: u8,
        previous: Option<[u8; 5]>,
        buf: &mut [u8],
    ) -> Result<(), String> {
        let mut packet = build_ata_packet(
            AtaDirection::Read,
            use_dma,
            command,
            features,
            count,
            lba_low,
            lba_mid,
            lba_high,
            device,
            previous,
            buf.len(),
        );
        self.submit(&mut packet, command)?;
        let offset = size_of::<AtaPassThroughEx>();
        buf.copy_from_slice(&packet.as_bytes()[offset..offset + buf.len()]);
        Ok(())
    }

    fn submit(&self, packet: &mut AlignedBuffer, command: u8) -> Result<(), String> {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_ATA_PASS_THROUGH,
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
                "ATA pass-through failed: Windows error {} (command 0x{:02x})",
                unsafe { GetLastError() },
                command
            ));
        }

        let task_file = packet.header().current_task_file;
        if task_file[6] & ATA_STATUS_ERROR != 0 {
            return Err(format!(
                "ATA command 0x{:02x} failed: status=0x{:02x}, error=0x{:02x}",
                command, task_file[6], task_file[0]
            ));
        }
        Ok(())
    }
}

impl Drop for AtaDevice {
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
    fn encodes_current_task_file_registers() {
        let packet = build_ata_packet(
            AtaDirection::Read,
            false,
            0xEC,
            0x11,
            0x22,
            0x33,
            0x44,
            0x55,
            0xE0,
            None,
            512,
        );
        assert_eq!(
            packet.header().current_task_file,
            [0x11, 0x22, 0x33, 0x44, 0x55, 0xE0, 0xEC, 0]
        );
        assert_eq!(packet.header().ata_flags, ATA_FLAGS_DATA_IN);
    }

    #[test]
    fn encodes_48_bit_previous_task_file_and_dma_flag() {
        let packet = build_ata_packet(
            AtaDirection::Read,
            true,
            0x25,
            1,
            2,
            3,
            4,
            5,
            0x40,
            Some([6, 7, 8, 9, 10]),
            512,
        );
        assert_eq!(
            packet.header().previous_task_file,
            [6, 7, 8, 9, 10, 0, 0, 0]
        );
        assert_eq!(
            packet.header().ata_flags,
            ATA_FLAGS_DATA_IN | ATA_FLAGS_48BIT_COMMAND | ATA_FLAGS_USE_DMA
        );
    }
}
