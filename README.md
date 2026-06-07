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

Auto-detects the controller type. NVMe devices are found automatically. SATA
devices require an explicit path:

- Windows: `ssd-flash-id.exe \\.\PhysicalDrive0`
- Linux: `ssd-flash-id /dev/sda`

`--no-probe` prevents controller-family probing on both NVMe and SATA. A forced
`--controller` selection prints a warning because it bypasses auto-detection.

## Platform Notes

### Windows

- Uses `IOCTL_STORAGE_PROTOCOL_COMMAND` for NVMe vendor commands.
- Uses `IOCTL_ATA_PASS_THROUGH` for SATA commands.
- Requires Administrator privileges.
- StorNVMe accepts a vendor-specific opcode only when the drive advertises it
  as supported in the NVMe Command Supported and Effects log. The Windows
  transport checks that log before sending each vendor opcode.
- USB bridges, RAID drivers, and vendor storage drivers may block pass-through
  commands.

### Linux

- Uses NVMe ioctl and ATA PASS-THROUGH via `SG_IO`.
- Requires root privileges (`sudo`).

## Credits

Original project:
[pseudolabel/ssd-flash-id](https://github.com/pseudolabel/ssd-flash-id).

Controller command support is based on the vendor-specific command research
from [VLO's SSD tools](http://vlo.name:3000/ssdtool/) for Windows.

## License

MIT
