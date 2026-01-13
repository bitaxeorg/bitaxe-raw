# bitaxe-raw usbserial Firmware

bitaxe-raw is firmware for the ESP32S3 on the bitaxe series boards. It will pass through ASIC UART, Board I2C, GPIO and ADC over usbserial. This can be used for research, testing and debugging.

## Developing

Install Rust:

`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### Install espup

`RUSTUP_TOOLCHAIN=stable cargo install espup --locked`

`espup install`

Note: `espup install` doesn't seem to be working right now. If it gives you an error, try:

instead install the binary from https://github.com/esp-rs/espup/releases make sure to get the right binary link for your system and replace the URL and run

`curl -L https://github.com/esp-rs/espup/releases/download/v0.15.1/espup-x86_64-apple-darwin -o espup`

`chmod a+x espup`

`./espup install`

### Install flashing tools
`cargo install cargo-espflash espflash --locked`

For building and flashing over USB:

`. $HOME/export-esp.sh`

### Build the latest firmware:
`cargo build --release`

### Flash the device:
`espflash flash --chip esp32s3 target/xtensa-esp32s3-none-elf/release/bitaxe-raw`

Or use cargo-espflash:
`cargo espflash flash --release --chip esp32s3`

After programming bitaxe-raw to your Bitaxe, if you ever want to change the firmware again you'll need to put the ESP32 into the bootloader. This can be done by holding the `BOOT` button as you attach power.

## Running
When connected, this usbserial firmware will create two serial ports. Usually the first serial port is "control serial" like I2C, GPIO, and ADC. The second serial port is "data serial" and is pass through UART.

After startup, the ASIC is held in reset by GPIO RST_N in order to minimize heat and power until the host device is connected and ready to use the ASIC. Enable it by setting RST_N High via the control serial port.

### Data Serial
- Second serial port
- All data is passed through, both directions.
- usbserial baudrate is mirrored on the output.


### Control Serial
- First serial port
- baudrate does not matter

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
5. command 
	- varies by command page. See below
6. data
	- data to write. variable length. See below
```

**I2C**

Commands:

- write: 0x20
- read: 0x30
- readwrite: 0x40

Data:

- [I2C address, (bytes to write), (number of bytes to read)]

Example:

- write 0xDE to addr 0x4F: `08 00 01 00 05 20 4F DE`
- read one byte from addr 0x4C: `08 00 01 00 05 30 4C 01`
- readwrite two bytes from addr 0x32, reg 0xFE: `09 00 01 00 05 40 32 FE 02`

**GPIO**

Commands:

- RST_N: 0x00

Data:

- [pin level]

Example

- Set pin 1 Low: `07 00 00 00 06 01 00`

**ADC**

Commands:

- read VDD: 0x50
- read VIN: 0x51

Example:

- read VDD Pin: `06 00 00 00 07 50`

