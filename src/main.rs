mod ata;
mod controllers;
mod detect;
mod nand_db;
mod nvme;

#[cfg(target_os = "linux")]
use std::os::unix::fs::FileTypeExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ptr::{null, null_mut};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    QueryDosDeviceW,
};
#[cfg(windows)]
use windows_sys::Win32::System::IO::DeviceIoControl;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::ata::{AtaDevice, parse_ata_identify};
use crate::controllers::FlashIdResult;
use crate::detect::{ControllerType, RtlVariant};
use crate::nand_db::{describe_flash, format_flash_id_hex};
use crate::nvme::{NvmeDevice, parse_identify};

struct Args {
    device: Option<String>,
    controller: Option<String>,
    rtl_variant: Option<RtlVariant>,
    help: bool,
    list: bool,
    raw: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        device: None,
        controller: None,
        rtl_variant: None,
        help: false,
        list: false,
        raw: false,
    };

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--help" | "-h" => args.help = true,
            "--list" | "-l" => args.list = true,
            "--raw" => args.raw = true,
            "--controller" | "-c" => {
                i += 1;
                if i < argv.len() {
                    args.controller = Some(argv[i].clone());
                } else {
                    eprintln!("error: --controller requires a value");
                    std::process::exit(1);
                }
            }
            "--rtl-variant" => {
                i += 1;
                if i < argv.len() {
                    args.rtl_variant = match argv[i].as_str() {
                        "v1" => Some(RtlVariant::V1),
                        "v2" => Some(RtlVariant::V2),
                        other => {
                            eprintln!("error: unknown rtl variant '{}' (expected v1 or v2)", other);
                            std::process::exit(1);
                        }
                    };
                } else {
                    eprintln!("error: --rtl-variant requires a value");
                    std::process::exit(1);
                }
            }
            s if s.starts_with('-') => {
                eprintln!("error: unknown option '{}'", s);
                eprintln!("try: ssd-flash-id --help");
                std::process::exit(1);
            }
            _ => {
                args.device = Some(argv[i].clone());
            }
        }
        i += 1;
    }
    args
}

fn print_usage() {
    let device_help = if cfg!(windows) {
        r"physical drive path (e.g. \\.\PhysicalDrive0)"
    } else {
        "device path (e.g. /dev/nvme0, /dev/sda)"
    };
    println!(
        "\
ssd-flash-id - Identify NAND flash chips on NVMe and SATA SSDs

usage: ssd-flash-id [options] [device]

arguments:
    device              {device_help}

options:
    -h, --help          show this help
    -l, --list          list NVMe and SATA devices
    -c, --controller    force controller type:
                        nvme: smi, rtl, phison, maxio, marvell, innogrit, tenafe
                        sata: jm, smi-sata, yeestor, sandforce, rtl-sata
    --rtl-variant       force Realtek variant: v1 (RTS5762/63), v2 (RTS5765/66/72)
    --raw               dump raw flash ID bytes as hex"
    );
}

fn check_root() {
    #[cfg(target_os = "linux")]
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("error: root privileges required\n");
        eprintln!("try: sudo ssd-flash-id [device]");
        std::process::exit(1);
    }

    #[cfg(windows)]
    if !is_process_elevated() {
        eprintln!("error: administrator privileges required\n");
        eprintln!("open an elevated terminal and run ssd-flash-id again");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn find_nvme_devices() -> Vec<String> {
    let mut devices = Vec::new();
    let dir = match std::fs::read_dir("/dev") {
        Ok(d) => d,
        Err(_) => return devices,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("nvme") {
            continue;
        }
        let suffix = &name[4..];
        // Skip namespace/partition devices (nvme0n1, nvme0n1p1): they have 'n' in the suffix
        if suffix.contains('n') {
            continue;
        }
        // Must be nvme followed by digits only (e.g. nvme0, nvme1)
        if !suffix.chars().all(|c| c.is_ascii_digit()) || suffix.is_empty() {
            continue;
        }
        let path = format!("/dev/{}", name);
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.file_type().is_char_device()
        {
            devices.push(path);
        }
    }
    devices.sort();
    devices
}

#[cfg(windows)]
fn find_nvme_devices() -> Vec<String> {
    enumerate_physical_drives()
        .into_iter()
        .filter(|path| storage_bus_type(path) == Some(BUS_TYPE_NVME))
        .collect()
}

#[cfg(target_os = "linux")]
fn find_sata_devices() -> Vec<String> {
    let mut devices = Vec::new();
    let dir = match std::fs::read_dir("/dev") {
        Ok(d) => d,
        Err(_) => return devices,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("sd") {
            continue;
        }
        let suffix = &name[2..];
        // Must be letters only (sda, sdb, ...) not partitions (sda1, sda2)
        if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_lowercase()) {
            continue;
        }
        let path = format!("/dev/{}", name);
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.file_type().is_block_device()
        {
            devices.push(path);
        }
    }
    devices.sort();
    devices
}

#[cfg(windows)]
fn find_sata_devices() -> Vec<String> {
    enumerate_physical_drives()
        .into_iter()
        .filter(|path| {
            matches!(
                storage_bus_type(path),
                Some(BUS_TYPE_ATA) | Some(BUS_TYPE_USB) | Some(BUS_TYPE_SATA)
            )
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn is_sata_path(path: &str) -> bool {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if name.starts_with("sd") {
        return true;
    }
    if name.starts_with("nvme") {
        return false;
    }
    // Unknown name pattern: check if block device (SATA) vs char device (NVMe)
    if let Ok(meta) = std::fs::metadata(path) {
        meta.file_type().is_block_device()
    } else {
        false
    }
}

#[cfg(windows)]
fn is_sata_path(path: &str) -> bool {
    storage_bus_type(path) != Some(BUS_TYPE_NVME)
}

#[cfg(windows)]
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
#[cfg(windows)]
const BUS_TYPE_ATA: u32 = 3;
#[cfg(windows)]
const BUS_TYPE_USB: u32 = 7;
#[cfg(windows)]
const BUS_TYPE_SATA: u32 = 11;
#[cfg(windows)]
const BUS_TYPE_NVME: u32 = 17;

#[cfg(windows)]
fn is_process_elevated() -> bool {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    unsafe {
        CloseHandle(token);
    }
    ok != 0 && elevation.TokenIsElevated != 0
}

#[cfg(windows)]
fn enumerate_physical_drives() -> Vec<String> {
    let mut buffer = vec![0u16; 65_536];
    let length = unsafe { QueryDosDeviceW(null(), buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 {
        return Vec::new();
    }

    let names = buffer[..length as usize]
        .split(|value| *value == 0)
        .take_while(|name| !name.is_empty())
        .filter_map(|name| String::from_utf16(name).ok())
        .collect::<Vec<_>>();
    physical_drive_paths(names.iter().map(String::as_str))
}

#[cfg(windows)]
fn physical_drive_paths<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut drives = names
        .into_iter()
        .filter_map(|name| {
            name.strip_prefix("PhysicalDrive")
                .and_then(|suffix| suffix.parse::<u32>().ok())
                .map(|number| (number, format!(r"\\.\PhysicalDrive{}", number)))
        })
        .collect::<Vec<_>>();
    drives.sort_unstable_by_key(|(number, _)| *number);
    drives.into_iter().map(|(_, path)| path).collect()
}

#[cfg(windows)]
fn storage_bus_type(path: &str) -> Option<u32> {
    let wide_path: Vec<u16> = std::path::Path::new(path)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }

    let query = [0u8; 12];
    let mut descriptor = [0u8; 64];
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            query.as_ptr().cast(),
            query.len() as u32,
            descriptor.as_mut_ptr().cast(),
            descriptor.len() as u32,
            &mut returned,
            null_mut(),
        )
    };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 || returned < 32 {
        return None;
    }
    Some(u32::from_le_bytes(descriptor[28..32].try_into().ok()?))
}
fn list_devices() {
    let nvme_devices = find_nvme_devices();
    let sata_devices = find_sata_devices();

    if nvme_devices.is_empty() && sata_devices.is_empty() {
        println!("no devices found");
        return;
    }

    for dev_path in &nvme_devices {
        match NvmeDevice::open(dev_path) {
            Ok(dev) => match dev.identify_controller() {
                Ok(id_data) => {
                    let info = parse_identify(&id_data);
                    println!(
                        "{}  {}  sn:{}  fw:{}",
                        dev_path, info.model, info.serial, info.firmware
                    );
                }
                Err(e) => println!("{}  (identify failed: {})", dev_path, e),
            },
            Err(e) => println!("{}  (open failed: {})", dev_path, e),
        }
    }

    for dev_path in &sata_devices {
        match AtaDevice::open(dev_path) {
            Ok(dev) => match dev.ata_identify() {
                Ok(id_data) => {
                    let info = parse_ata_identify(&id_data);
                    println!(
                        "{}  {}  sn:{}  fw:{}",
                        dev_path, info.model, info.serial, info.firmware
                    );
                }
                Err(e) => println!("{}  (identify failed: {})", dev_path, e),
            },
            Err(e) => println!("{}  (open failed: {})", dev_path, e),
        }
    }
}

fn resolve_nvme_controller_type(name: &str) -> Option<ControllerType> {
    match name {
        "smi" => Some(ControllerType::Smi("SMI (forced)".into())),
        "rtl" => Some(ControllerType::Realtek(
            "Realtek (forced)".into(),
            RtlVariant::V1,
        )),
        "phison" => Some(ControllerType::Phison("Phison (forced)".into())),
        "maxio" => Some(ControllerType::Maxio("Maxio (forced)".into())),
        "marvell" => Some(ControllerType::Marvell("Marvell (forced)".into())),
        "innogrit" => Some(ControllerType::Innogrit("Innogrit (forced)".into())),
        "tenafe" => Some(ControllerType::Tenafe("Tenafe (forced)".into())),
        _ => None,
    }
}

fn controller_family_display(ct: &ControllerType) -> &str {
    match ct {
        ControllerType::Smi(_) => "Silicon Motion",
        ControllerType::Realtek(_, _) => "Realtek",
        ControllerType::Phison(_) => "Phison",
        ControllerType::Maxio(_) => "Maxio",
        ControllerType::Marvell(_) => "Marvell",
        ControllerType::Innogrit(_) => "Innogrit",
        ControllerType::Tenafe(_) => "Tenafe",
    }
}

fn nvme_read_flash_id(dev: &NvmeDevice, ct: &ControllerType) -> Result<FlashIdResult, String> {
    match ct {
        ControllerType::Smi(_) => controllers::smi::read_flash_id(dev),
        ControllerType::Realtek(_, variant) => controllers::rtl::read_flash_id(dev, variant),
        ControllerType::Phison(_) => controllers::phison::read_flash_id(dev),
        ControllerType::Maxio(_) => controllers::maxio::read_flash_id(dev),
        ControllerType::Marvell(_) => controllers::marvell::read_flash_id(dev),
        ControllerType::Innogrit(_) => controllers::innogrit::read_flash_id(dev),
        ControllerType::Tenafe(_) => controllers::tenafe::read_flash_id(dev),
    }
}

fn print_banks(result: &FlashIdResult, raw: bool) {
    if result.banks.is_empty() {
        println!("no flash banks detected");
        return;
    }

    for bank in &result.banks {
        let hex = format_flash_id_hex(&bank.flash_id);
        if raw {
            println!("Bank{:02}: {}", bank.bank_num, hex);
        } else {
            let desc = describe_flash(&bank.flash_id);
            println!("Bank{:02}: {} - {}", bank.bank_num, hex, desc);
        }
    }
}

fn run_nvme(dev_path: &str, args: &Args) {
    let dev = match NvmeDevice::open(dev_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    let id_data = match dev.identify_controller() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to identify controller: {}", e);
            std::process::exit(1);
        }
    };
    let info = parse_identify(&id_data);

    let mut ct = if let Some(ref forced) = args.controller {
        match resolve_nvme_controller_type(forced) {
            Some(ct) => ct,
            None => {
                eprintln!(
                    "error: unknown controller type '{}'\n\nvalid nvme types: smi, rtl, phison, maxio, marvell, innogrit, tenafe",
                    forced
                );
                std::process::exit(1);
            }
        }
    } else {
        match detect::detect(&dev, &info) {
            Some(ct) => ct,
            None => {
                eprintln!(
                    "error: could not auto-detect controller type for {}\n\
                     model: {}\n\
                     firmware: {}\n\
                     vid: 0x{:04x}, ssvid: 0x{:04x}\n\n\
                     try: ssd-flash-id --controller <type> {}\n\
                     valid types: smi, rtl, phison, maxio, marvell, innogrit, tenafe",
                    dev_path, info.model, info.firmware, info.vid, info.ssvid, dev_path
                );
                std::process::exit(1);
            }
        }
    };

    // Override Realtek variant if user specified one
    if let Some(variant) = args.rtl_variant {
        if let ControllerType::Realtek(ref name, _) = ct {
            ct = ControllerType::Realtek(name.clone(), variant);
        } else if args.controller.as_deref() == Some("rtl") {
            ct = ControllerType::Realtek("Realtek (forced)".into(), variant);
        }
    }

    match nvme_read_flash_id(&dev, &ct) {
        Ok(result) => {
            println!("Model      : {}", info.model);
            println!("Firmware   : {}", info.firmware);
            println!(
                "Controller : {} ({})",
                result.controller_name,
                controller_family_display(&ct)
            );
            println!();
            print_banks(&result, args.raw);
        }
        Err(e) => {
            eprintln!("error: {} flash ID read failed: {}\n", ct.name(), e);
            eprintln!(
                "the {} vendor command (--controller {}) was rejected by this device.",
                controller_family_display(&ct),
                ct.family()
            );
            eprintln!("this may mean the controller is a different type than detected.\n");
            eprintln!("try a different controller type:");
            eprintln!("  ssd-flash-id --controller <type> {}", dev_path);
            eprintln!("  valid types: smi, rtl, phison, maxio, marvell, innogrit, tenafe");
            std::process::exit(1);
        }
    }
}

fn run_sata(dev_path: &str, args: &Args) {
    let forced = args.controller.as_deref();
    const SATA_TYPES: &[&str] = &["jm", "smi-sata", "yeestor", "sandforce", "rtl-sata"];
    if let Some(f) = forced
        && !SATA_TYPES.contains(&f)
    {
        eprintln!(
            "error: controller type '{}' is not supported for SATA devices\n\nsupported sata types: {}",
            f,
            SATA_TYPES.join(", ")
        );
        std::process::exit(1);
    }

    let dev = match AtaDevice::open(dev_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    let id_data = match dev.ata_identify() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to identify device: {}", e);
            std::process::exit(1);
        }
    };
    let info = parse_ata_identify(&id_data);

    // Check ATA IDENTIFY data for embedded flash IDs (some controllers store them in vendor words)
    let identify_fid = controllers::ata_identify_fid::extract_from_identify(&id_data);

    // Try controllers in order: firmware detection first, then probing
    let result = if forced == Some("jm") {
        try_jm_sata(&dev)
    } else if forced == Some("smi-sata") {
        try_smi_sata(&dev)
    } else if forced == Some("yeestor") {
        try_yeestor(&dev)
    } else if forced == Some("sandforce") {
        try_sandforce(&dev)
    } else if forced == Some("rtl-sata") {
        try_rtl_sata(&dev)
    } else {
        // Auto-detect: check firmware strings first
        if controllers::smi_sata::detect_from_firmware(&info.firmware).is_some() {
            try_smi_sata(&dev)
        } else if controllers::rtl_sata::detect_from_firmware(&info.firmware).is_some() {
            try_rtl_sata(&dev)
        } else {
            // Try each controller family in order of least-invasive
            try_yeestor(&dev)
                .or_else(|_| try_smi_sata(&dev))
                .or_else(|_| try_sandforce(&dev))
                .or_else(|_| try_jm_sata(&dev))
                .or_else(|_| try_rtl_sata(&dev))
                .or_else(|_| {
                    // Last resort: check if flash ID was embedded in ATA IDENTIFY data
                    identify_fid.clone().map(|r| (r, "SATA")).ok_or_else(|| {
                        "no vendor commands succeeded and no flash ID in IDENTIFY data".to_string()
                    })
                })
        }
    };

    let (result, family) = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}", e);
            eprintln!("\nmodel: {}", info.model);
            eprintln!("firmware: {}", info.firmware);
            eprintln!("\nthis SATA device may not have a supported controller.");
            eprintln!("supported sata types: {}", SATA_TYPES.join(", "));
            std::process::exit(1);
        }
    };

    println!("Model      : {}", info.model);
    println!("Firmware   : {}", info.firmware);
    println!("Controller : {} ({})", result.controller_name, family);
    println!();
    print_banks(&result, args.raw);
}

fn try_jm_sata(dev: &AtaDevice) -> Result<(FlashIdResult, &'static str), String> {
    let fw_response = controllers::jm_sata::read_firmware_id(dev)?;
    let result = controllers::jm_sata::read_flash_id(dev, &fw_response)?;
    Ok((result, "JMicron/Maxio"))
}

fn try_smi_sata(dev: &AtaDevice) -> Result<(FlashIdResult, &'static str), String> {
    let result = controllers::smi_sata::read_flash_id(dev)?;
    Ok((result, "Silicon Motion"))
}

fn try_yeestor(dev: &AtaDevice) -> Result<(FlashIdResult, &'static str), String> {
    let result = controllers::yeestor::read_flash_id(dev)?;
    Ok((result, "Yeestor/SiliconGo"))
}

fn try_sandforce(dev: &AtaDevice) -> Result<(FlashIdResult, &'static str), String> {
    let result = controllers::sandforce::read_flash_id(dev)?;
    Ok((result, "SandForce"))
}

fn try_rtl_sata(dev: &AtaDevice) -> Result<(FlashIdResult, &'static str), String> {
    let result = controllers::rtl_sata::read_flash_id(dev)?;
    Ok((result, "Realtek"))
}

fn main() {
    let args = parse_args();

    if args.help {
        print_usage();
        return;
    }

    check_root();

    if args.list {
        list_devices();
        return;
    }

    let dev_path = match &args.device {
        Some(p) => p.clone(),
        None => {
            // Auto-detect: NVMe only. SATA requires explicit device path.
            let devices = find_nvme_devices();
            if devices.is_empty() {
                eprintln!("error: no NVMe devices found");
                if cfg!(windows) {
                    eprintln!(
                        "\nfor SATA devices, specify the path: ssd-flash-id \\\\.\\PhysicalDriveN"
                    );
                } else {
                    eprintln!("\nfor SATA devices, specify the path: ssd-flash-id /dev/sdX");
                }
                std::process::exit(1);
            }
            if devices.len() > 1 {
                eprintln!("multiple NVMe devices found:");
                for d in &devices {
                    eprintln!("  {}", d);
                }
                eprintln!("\nspecify a device, e.g.: ssd-flash-id {}", devices[0]);
                std::process::exit(1);
            }
            devices.into_iter().next().unwrap()
        }
    };

    if is_sata_path(&dev_path) {
        run_sata(&dev_path, &args);
    } else {
        run_nvme(&dev_path, &args);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn extracts_and_sorts_physical_drive_names() {
        let names = [
            "C:",
            "PhysicalDrive10",
            "PhysicalDrive2",
            "HarddiskVolume1",
            "PhysicalDrive0",
        ];

        assert_eq!(
            physical_drive_paths(names.into_iter()),
            vec![
                r"\\.\PhysicalDrive0",
                r"\\.\PhysicalDrive2",
                r"\\.\PhysicalDrive10",
            ]
        );
    }
}
