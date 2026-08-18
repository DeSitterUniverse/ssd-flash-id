# ssd-flash-id

Windows port of
[pseudolabel/ssd-flash-id](https://github.com/pseudolabel/ssd-flash-id), the
Linux open-source equivalent of
[VLO's SSD Flash ID tools](http://vlo.name:3000/ssdtool/). Identifies NAND flash
chips on NVMe and SATA SSDs via vendor-specific commands, reporting flash type
(QLC/TLC/MLC/SLC), manufacturer, and technology node for each NAND bank.

The fork keeps the original Linux implementation and controller/NAND decoding
code, with a native Windows storage transport added underneath it.

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
- Administrator terminal for raw physical-drive access

```
git clone https://github.com/DeSitterUniverse/ssd-flash-id.git
cd ssd-flash-id
cargo build --release
```

Run:

```powershell
.\target\release\ssd-flash-id.exe --list
.\target\release\ssd-flash-id.exe \\.\PhysicalDrive0
```

### Linux

Install the upstream crate:

```
cargo install ssd-flash-id
```

Or build this fork:

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

### Windows notes

Use `--list` first to confirm the target `\\.\PhysicalDriveN` path and model.
Raw device access requires an elevated process.

For an unknown controller, `--no-probe` avoids controller-family probe commands
where possible. A flash-ID read still requires the vendor command for the
selected controller.

USB bridges and RAID/VMD stacks can hide the underlying protocol. Use
`--device-type nvme` or `--device-type ata` only after confirming the drive
type. These flags select the transport; they do not bypass drivers or bridges
that block pass-through commands.

A forced `--controller` bypasses auto-detection and should only be used when the
controller family is known.

## Windows port

The port keeps controller detection, vendor-command definitions, response
parsing, NAND-bank extraction, and NAND ID decoding shared with Linux. Platform
specific code is limited to device enumeration and raw command transport.

```
controller / NAND logic
        |
        +-- NvmeDevice
        |     +-- Linux: NVME_IOCTL_ADMIN_CMD
        |     `-- Windows: IOCTL_STORAGE_PROTOCOL_COMMAND
        |
        `-- AtaDevice
              +-- Linux: SG_IO + ATA PASS-THROUGH(16)
              `-- Windows: IOCTL_ATA_PASS_THROUGH
```

The split is implemented in:

```
src/nvme.rs
  src/nvme/linux.rs
  src/nvme/windows.rs

src/ata.rs
  src/ata/linux.rs
  src/ata/windows.rs
```

### NVMe transport

Linux sends admin commands directly with `NVME_IOCTL_ADMIN_CMD`.

Windows opens `\\.\PhysicalDriveN` with `CreateFileW` and uses:

- `IOCTL_STORAGE_QUERY_PROPERTY` for standard protocol data such as Identify
  Controller;
- `IOCTL_STORAGE_PROTOCOL_COMMAND` for vendor-specific NVMe admin commands;
- `IOCTL_SCSI_GET_ADDRESS` to resolve a storage-adapter handle when required.

The Windows transport serializes the existing opcode, NSID, and CDW10-CDW15
arguments into a 64-byte NVMe command and packs it with the protocol header,
error-information buffer, and optional transfer buffer in one aligned request.
Returned protocol status, NVMe status, transfer lengths, and response extents
are validated before data is exposed to controller code.

`STORAGE_PROTOCOL_COMMAND` has a trailing `Command[ANYSIZE_ARRAY]` in the
Windows SDK. The request therefore reports the complete SDK structure length
while the embedded NVMe command starts at the structure's command offset. These
are not the same byte value and are handled separately.

Some storage stacks reject `IOCTL_STORAGE_PROTOCOL_COMMAND` on the physical
-drive handle with `ERROR_INVALID_PARAMETER`. In that case the request is
restored and retried through the matching `\\.\ScsiN:` adapter handle.

### StorNVMe vendor opcodes

Microsoft StorNVMe can reject vendor-specific admin opcodes that are not marked
supported by the drive.

Before a vendor command is submitted, the Windows transport queries NVMe
Command Supported and Effects Log page `0x05` and checks the opcode's `CSUPP`
bit. The log is cached for the lifetime of the device handle.

This means a controller/vendor command that works through the Linux ioctl path
may still fail on Windows depending on firmware and the active storage stack.
Controller-family support alone is not sufficient to guarantee Windows
pass-through support.

`AVSCC.CommandFormatInSpec` is reported as diagnostic information only. It is
not used as a compatibility gate.

### ATA transport

Linux issues ATA PASS-THROUGH(16) CDBs through `SG_IO`.

Windows uses `IOCTL_ATA_PASS_THROUGH` with `ATA_PASS_THROUGH_EX`. The existing
ATA command abstraction is mapped to Windows task-file registers and flags,
including data direction, DMA, and 48-bit command handling. For 48-bit
commands, high-order register bytes are placed in `PreviousTaskFile` and
low-order bytes in `CurrentTaskFile`.

The pass-through header and data buffer share one aligned allocation. Returned
buffer lengths, transfer counts, ATA status, error, and device-fault state are
checked before returning data.

### Device discovery and safety

Windows-specific support also includes:

- `QueryDosDeviceW` enumeration of `PhysicalDriveN` devices;
- `STORAGE_DEVICE_DESCRIPTOR` bus-type detection;
- explicit handling for ambiguous USB/RAID devices;
- UAC elevation checks with `OpenProcessToken` / `TokenElevation`;
- configurable command timeouts;
- Windows error text through `FormatMessageW`;
- `--no-probe` and forced-controller warnings;
- validation tests for Windows protocol-packet layout and simulated responses.

Windows compatibility remains firmware-, driver-, and topology-dependent.
USB bridges, RAID/VMD layers, and vendor storage drivers may reject low-level
commands even when the SSD controller itself is supported.

## Requirements

### Windows

- Windows 10 or later
- Administrator privileges
- storage stack that permits the required NVMe/ATA pass-through commands

### Linux

- Linux with NVMe ioctl and `SG_IO` support
- root privileges (`sudo`)

## Credits

Original project:
[pseudolabel/ssd-flash-id](https://github.com/pseudolabel/ssd-flash-id).

Controller command support is based on the vendor-specific command research
from [VLO's SSD tools](http://vlo.name:3000/ssdtool/) for Windows.

## License

MIT
