use embassy_embedded_hal::SetConfig;
use esp_hal::time::Rate;
use heapless::Vec;

use super::CommandError;

#[derive(defmt::Format)]
pub enum Command {
    SetFrequency { frequency: u32 },                         // 0x10
    Write { addr: u8, buf: Vec<u8, 256> },                   // 0x20
    Read { addr: u8, len: u8 },                              // 0x30
    WriteRead { addr: u8, buf: Vec<u8, 256>, read_len: u8 }, // 0x40
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [0x10, b1, b2, b3, b4] => Ok(Self::SetFrequency {
                frequency: u32::from_le_bytes([*b1, *b2, *b3, *b4]),
            }),
            [0x20, addr, buf @ ..] => Ok(Self::Write {
                addr: *addr,
                buf: Vec::from_slice(buf).map_err(|_| CommandError::BufferOverflow)?,
            }),
            [0x30, addr, len] => Ok(Self::Read { addr: *addr, len: *len }),
            [0x40, addr, buf @ .., read_len] => Ok(Self::WriteRead {
                addr: *addr,
                buf: Vec::from_slice(buf).map_err(|_| CommandError::BufferOverflow)?,
                read_len: *read_len,
            }),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            Command::SetFrequency { frequency } => {
                let config = esp_hal::i2c::master::Config::default().with_frequency(Rate::from_hz(*frequency));
                controller.disp.set_config(&config).map_err(|_| CommandError::Message("DISP Set Frequency Error"))?;
                Ok(Vec::from_slice(&frequency.to_le_bytes()).unwrap())
            }
            Command::Write { addr, buf } => {
                controller.disp.write_async(*addr, buf).await.map_err(|_| CommandError::Message("DISP Write Error"))?;
                Ok(Vec::from_slice(&[buf.len() as u8]).unwrap())
            }
            Command::Read { addr, len } => {
                let mut buf = Vec::new();
                let _ = buf.resize_default(*len as usize);
                controller.disp.read_async(*addr, &mut buf).await.map_err(|_| CommandError::Message("DISP Read Error"))?;
                Ok(buf)
            }
            Command::WriteRead { addr, buf, read_len } => {
                let mut read_buf = Vec::new();
                let _ = read_buf.resize_default(*read_len as usize);
                controller.disp.write_read_async(*addr, buf, &mut read_buf[0..*read_len as usize]).await.map_err(|_| CommandError::Message("DISP WriteRead Error"))?;
                Ok(read_buf)
            }
        }
    }
}
