# bitaxe-raw-bonanza usbserial Firmware

bitaxe-raw-bonanza is firmware for the ESP32-S3 on bitaxeBonanza boards used with the [bonanza-bridge-fw](https://github.com/bitaxeorg/bonanza-bridge-fw) RP2040 bridge. It exposes two USB serial ports to the host: a control port for board peripherals and a data port for ASIC serial traffic. Board I2C and ADC are handled directly by the ESP32-S3, while fan control and ASIC control signals are proxied to the RP2040 bridge. This firmware is intended for research, testing, and debugging.

## Developing

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Install espup

```bash
RUSTUP_TOOLCHAIN=stable cargo install espup --locked
```

```bash
espup install
```

### Install flashing tools
```bash
cargo install cargo-espflash espflash --locked
```

For building and flashing over USB:

```bash
. $HOME/export-esp.sh
```

### Build the latest firmware:
```bash
cargo build --release
```

### Flash the device:
```bash
cargo espflash flash --release --chip esp32s3
```

espflash isn't restarting the ESP32 after flashing. Press the Bitaxe `RESET` button to boot the newly flashed firmware.

After programming `bitaxe-raw-bonanza` to your Bitaxe, if you ever want to change the firmware again you'll need to put the ESP32 into the bootloader. This can be done by holding the `BOOT` button as you attach power.

## Running
When connected, this firmware creates two serial ports:

- `control serial`: board I2C, GPIO, ADC, and fan control commands
- `data serial`: passthrough UART path used for ASIC traffic

### Data Serial
- Second serial port
- All data is passed through in both directions.
- The USB CDC baudrate is mirrored onto ESP32 `UART1`.
- On bitaxeBonanza, this UART connects to the RP2040 bridge data UART on ESP32 `TX GPIO17` and `RX GPIO18`.
- The RP2040 bridge expects this link to run at `5000000` baud.


### Control Serial
- First serial port
- baudrate does not matter for USB
- On bitaxeBonanza, GPIO and fan commands are proxied to the RP2040 bridge over ESP32 `UART0` on `TX GPIO43` and `RX GPIO44` at `115200` baud

**Packet Format**

| 0      | 1      | 2  | 3   | 4    | 5   | 6... |
|--------|--------|----|-----|------|-----|------|
| LEN LO | LEN HI | ID | BUS | PAGE | CMD | DATA |

```
0. length low
1. length high
	- packet length is number of bytes of the whole packet. 
2. command id
	- Whatever byte you want. will be returned in the response 
3. command bus
	- always 0x00 
4. command page
	- I2C:  0x05
	- GPIO: 0x06
	- ADC:  0x07
	- Fan:  0x09
5. command 
	- varies by command page. See below
6. data
	- data to write. variable length. See below
```

**I2C**

Commands:

- set frequency: 0x10
- write: 0x20
- read: 0x30
- readwrite: 0x40

Data:

- set frequency: `[freq0, freq1, freq2, freq3]` as little-endian Hz
- write: `[I2C address, (bytes to write)]`
- read: `[I2C address, number of bytes to read]`
- readwrite: `[I2C address, (bytes to write), number of bytes to read]`

Example:

- set bus speed to 400kHz: `0A 00 01 00 05 10 80 1A 06 00`
- write 0xDE to addr 0x4F: `08 00 01 00 05 20 4F DE`
- read one byte from addr 0x4C: `08 00 01 00 05 30 4C 01`
- readwrite two bytes from addr 0x32, reg 0xFE: `09 00 01 00 05 40 32 FE 02`

**GPIO**

Commands:

- `RST_N` compatibility alias: 0x00
- `5v_en`: 0x01
- `asic_rst`: 0x02
- asic_trip (read-only): 0x03

Data:

- set commands: `[pin level]`
- get commands: no data

Example

- Set `RST_N` low through the compatibility alias: `07 00 00 00 06 00 00`
- Set `5v_en` high: `07 00 00 00 06 01 01`
- Read `asic_trip`: `06 00 00 00 06 03`

On bitaxeBonanza, these GPIO commands are forwarded to the RP2040 bridge. The `RST_N` compatibility alias is translated to the RP2040 `asic_rst` command so existing host tools can keep using command `0x00`.

**ADC**

Commands:

- read VDD: 0x50

Example:

- read VDD Pin: `06 00 00 00 07 50`

**Fan**

Commands:

- set speed: 0x10
- get tachometer: 0x20

Data:

- [speed percentage 0-100] for set speed

Example:

- Set fan speed to 50%: `07 00 00 00 09 10 32`
- Read fan tach (RPM): `06 00 00 00 09 20`
