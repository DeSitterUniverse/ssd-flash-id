# ssd-flash-id

Windows port of
[pseudolabel/ssd-flash-id](https://github.com/pseudolabel/ssd-flash-id), the
Linux open-source equivalent of
[VLO's SSD Flash ID tools](http://vlo.name:3000/ssdtool/). It identifies NAND
flash chips on NVMe and SATA SSDs via vendor-specific commands, reporting flash
type (QLC/TLC/MLC/SLC), manufacturer, and technology node for each NAND bank.

The Windows port retains the original Linux implementation and controller
support.

```
$ sudo ssd-flash-id /dev/nvme0
Model      : KINGSTON SNV2S1000G
Firmware   : SBM02106
Controller : SM2267XT (Silicon Motion)

Bank00: 0x89,0xd3,0xac,0x32,0xc6,0x00,0x00,0x00 - Intel 144L(N38A) QLC
Bank01: 0x89,0xd3,0xac,0x32,0xc6,0x00,0x00,0x00 - Intel 144L(N38A) QLC
Bank02: 0x89,0xd3,0xac,0x32,0xc6,0x00,0x00,0x00 - Intel 144L(N38A) QLC
Bank03: 0x89,0xd3,0xac,0x32,0xc6,0x00,0x00,0x00 - Intel 144L(N38A) QLC
```

## Build

### Windows

Requirements:

- Windows 10 or later
- Rust stable with the MSVC toolchain
- An Administrator PowerShell or Command Prompt for accessing physical drives

Build the `windows-port` branch:

```
git clone https://github.com/DeSitterUniverse/ssd-flash-id.git
cd ssd-flash-id
git switch windows-port
cargo build --release
```

Run from an elevated terminal:

```
.\target\release\ssd-flash-id.exe --list
.\target\release\ssd-flash-id.exe \\.\PhysicalDrive0
```

### Linux

Install the upstream crate:

```
cargo install ssd-flash-id
```

Or build this branch from source:

```
cargo build --release
sudo ./target/release/ssd-flash-id --list
```

## Supported Controllers

### NVMe

| Family | Controllers |
|--------|------------|
| Silicon Motion | SM2260, SM2262, SM2263, SM2264, SM2265, SM2267, SM2268, SM2269, SM2270, SM2508, SM8366 |
| Realtek | RTS5762, RTS5763, RTS5765, RTS5766, RTS5772 |
| Phison | PS5012 (E12), PS5016 (E16), PS5018 (E18), PS5019 (E19T), PS5021 (E21T), PS5026 (E26), PS5027 (E27T) |
| Maxio | MAP1001, MAP1002, MAP1003, MAP1201, MAP1202, MAP1601, MAP1602 |
| Marvell | 88NV1160, 88NV1140 |
| Innogrit | IG5208, IG5216, IG5220, IG5236, IG5266 |
| Tenafe | TC2200, TC2201 |

### SATA

| Family | Controllers |
|--------|------------|
| JMicron/Maxio | MAS1102, MAS0902, MK8115, JMF605-JMF670 |
| Silicon Motion | SM2246, SM2256, SM2258, SM2259 |
| SandForce | SF-2281, SF-2282 |
| Yeestor/SiliconGo | YS9082, YS9085 |
| Realtek | RTS5732, RTS5733, RTS5735 |

## NAND Identification

Recognizes flash from Micron, Intel, Spectek, Samsung, SK Hynix, Toshiba/Kioxia,
YMTC, SanDisk, and others. Reports technology node (e.g. 176L, 232L, BiCS5,
3dv7-176L), cell type (SLC/MLC/TLC/QLC), and page size where available.

## Usage

### Windows

#### 1. Build

Install [Rust](https://www.rust-lang.org/tools/install) with the stable MSVC
toolchain, then run:

```powershell
git clone https://github.com/DeSitterUniverse/ssd-flash-id.git
cd ssd-flash-id
git switch windows-port
cargo build --release
```

The executable is created at:

```text
target\release\ssd-flash-id.exe
```

#### 2. Open an Administrator terminal

Raw physical drives require Administrator privileges:

1. Open the Start menu.
2. Search for **PowerShell** or **Windows Terminal**.
3. Select **Run as administrator**.
4. Change to the cloned repository directory.

#### 3. List detected drives

```powershell
.\target\release\ssd-flash-id.exe --list
```

The command prints paths such as `\\.\PhysicalDrive0`. Confirm the model and
serial number before running vendor commands against a drive.

#### 4. Inspect a drive

```powershell
.\target\release\ssd-flash-id.exe \\.\PhysicalDrive0
```

Start with metadata-only controller detection if the drive or controller is
unknown:

```powershell
.\target\release\ssd-flash-id.exe --no-probe \\.\PhysicalDrive0
```

`--no-probe` prevents controller-family probing, but reading the flash ID still
requires the vendor command for the controller selected from metadata.

If a USB bridge or RAID driver makes the protocol ambiguous, specify it
explicitly:

```powershell
.\target\release\ssd-flash-id.exe --device-type nvme \\.\PhysicalDrive2
.\target\release\ssd-flash-id.exe --device-type ata \\.\PhysicalDrive2
```

Some USB bridges, RAID drivers, and vendor storage drivers block pass-through
commands entirely. `--device-type` cannot bypass that driver limitation.

Force a controller only when its family is already known:

```powershell
.\target\release\ssd-flash-id.exe --controller phison \\.\PhysicalDrive0
.\target\release\ssd-flash-id.exe --controller smi-sata \\.\PhysicalDrive1
```

Increase the default ten-second command timeout when required:

```powershell
.\target\release\ssd-flash-id.exe --timeout-seconds 30 \\.\PhysicalDrive0
```

#### 5. Interpret common errors

- `administrator privileges required`: reopen the terminal as Administrator.
- `could not determine the storage protocol`: add `--device-type nvme` or
  `--device-type ata` after confirming the drive type.
- `drive does not mark it supported in Command Effects Log page 0x05`: the
  Windows NVMe driver will not allow that vendor opcode.
- `AVSCC.CommandFormatInSpec=0`: diagnostic controller metadata. It does not
  by itself prove that StorNVMe will reject a vendor command.
- `pass-through failed`: the drive, bridge, RAID layer, or storage driver
  rejected the low-level command.
- `could not auto-detect controller type`: retry with `--no-probe`, or use
  `--controller` only if the controller family is known.

Do not experiment with forced controller families on a drive containing
important data. Vendor-specific commands operate below the file-system layer.

### Linux

```bash
sudo ssd-flash-id --list
sudo ssd-flash-id /dev/nvme0
sudo ssd-flash-id /dev/sda
```

### Options

```
ssd-flash-id [options] [device]

options:
    -l, --list          list NVMe and SATA devices
    --device-type       force physical-drive protocol: nvme or ata
    -c, --controller    force controller type:
                        nvme: smi, rtl, phison, maxio, marvell, innogrit, tenafe
                        sata: jm, smi-sata, yeestor, sandforce, rtl-sata
    --rtl-variant       force Realtek NVMe variant: v1 or v2
    --raw               dump raw flash ID bytes without decoding
    --no-probe          avoid controller-family probe commands
    --timeout-seconds   command timeout in seconds (default: 10)
```

`--no-probe` prevents controller-family probing on both NVMe and SATA. A forced
`--controller` selection prints a warning because it bypasses auto-detection.

## Platform Notes

### Windows

- Uses `IOCTL_STORAGE_PROTOCOL_COMMAND` for NVMe vendor commands.
- Reports the complete SDK `STORAGE_PROTOCOL_COMMAND` structure length while
  keeping the embedded NVMe command at its documented byte offset.
- If a physical-drive request fails with error 87, retries it through the
  drive's SCSI-port adapter handle.
- Uses `IOCTL_ATA_PASS_THROUGH` for SATA commands.
- Requires Administrator privileges.
- StorNVMe accepts a vendor-specific opcode only when the drive advertises it
  as supported in the NVMe Command Supported and Effects log. The Windows
  transport checks that log before sending each vendor opcode.
- After a failed vendor command, the CLI reports
  `AVSCC.CommandFormatInSpec=0` as diagnostic context rather than a
  compatibility gate.
- USB bridges, RAID drivers, and vendor storage drivers may block pass-through
  commands.

#### Windows compatibility limits

Windows compatibility is firmware- and driver-dependent, not just
controller-family-dependent. This Windows transport cannot use vendor commands
when:

- Command Effects Log page `0x05` cannot be queried, or the required opcode
  has `CSUPP=0`;
- it is behind a USB bridge or RAID/VMD layer that blocks protocol commands;
- its installed storage driver does not implement
  `IOCTL_STORAGE_PROTOCOL_COMMAND`.

`AVSCC.CommandFormatInSpec=0` is not sufficient to classify a device as
unsupported. All controller families must be evaluated from their actual
pass-through result and Command Effects data; model or family names alone are
insufficient.

### Linux

- Uses NVMe ioctl and ATA PASS-THROUGH via `SG_IO`.
- Requires root privileges (`sudo`).

## Windows API References

The Windows implementation directly uses or encodes the APIs and structures
documented in these Microsoft sources:

### NVMe and storage protocols

- [IOCTL_STORAGE_PROTOCOL_COMMAND](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-ioctl_storage_protocol_command)
- [STORAGE_PROTOCOL_COMMAND](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-storage_protocol_command)
- [IOCTL_STORAGE_QUERY_PROPERTY](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-ioctl_storage_query_property)
- [STORAGE_PROPERTY_QUERY](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-storage_property_query)
- [STORAGE_PROTOCOL_SPECIFIC_DATA](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-storage_protocol_specific_data)
- [STORAGE_PROTOCOL_DATA_DESCRIPTOR](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-storage_protocol_data_descriptor)
- [STORAGE_DEVICE_DESCRIPTOR](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-storage_device_descriptor)
- [STORAGE_BUS_TYPE](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ne-winioctl-storage_bus_type)
- [NVME_IDENTIFY_CONTROLLER_DATA](https://learn.microsoft.com/en-us/windows/win32/api/nvme/ns-nvme-nvme_identify_controller_data)
- [StorNVMe command set support](https://learn.microsoft.com/en-us/windows-hardware/drivers/storage/stornvme-command-set-support)
- [IOCTL_SCSI_GET_ADDRESS](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddscsi/ni-ntddscsi-ioctl_scsi_get_address)
- [SCSI_ADDRESS](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddscsi/ns-ntddscsi-_scsi_address)

### ATA pass-through

- [IOCTL_ATA_PASS_THROUGH](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddscsi/ni-ntddscsi-ioctl_ata_pass_through)
- [ATA_PASS_THROUGH_EX](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddscsi/ns-ntddscsi-_ata_pass_through_ex)

### Device access, enumeration, and errors

- [DeviceIoControl](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-deviceiocontrol)
- [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
- [QueryDosDeviceW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-querydosdevicew)
- [GetTokenInformation](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation)
- [TOKEN_ELEVATION](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_elevation)
- [FormatMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-formatmessagew)
- [GetLastError](https://learn.microsoft.com/en-us/windows/win32/api/errhandlingapi/nf-errhandlingapi-getlasterror)

## Credits

Original project:
[pseudolabel/ssd-flash-id](https://github.com/pseudolabel/ssd-flash-id).

Controller command support is based on the vendor-specific command research
from [VLO's SSD tools](http://vlo.name:3000/ssdtool/) for Windows.

## License

MIT
