use super::CommandError;
use heapless::Vec;

pub struct Pins<'d> {
    pub asic_resetn: esp_hal::gpio::Output<'d>,
    pub vddio_5v_en: esp_hal::gpio::Output<'d>,
    pub tps546_en: esp_hal::gpio::Output<'d>,
}

#[derive(defmt::Format)]
pub enum Command {
    SetAsicResetn { level: bool },
    GetAsicResetn,
    SetVddio5vEn { level: bool },
    GetVddio5vEn,
    SetTps546En { level: bool },
    GetTps546En,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            // Get ASIC Reset (Active Low)
            [0x00] => Ok(Self::GetAsicResetn),
            // Set ASIC Reset (Active Low)
            [0x00, level] => Ok(Self::SetAsicResetn { level: *level > 0 }),
            // Get VDDIO 5V Enable
            [0x01] => Ok(Self::GetVddio5vEn),
            // Set VDDIO 5V Enable
            [0x01, level] => Ok(Self::SetVddio5vEn { level: *level > 0 }),
            // Get TPS546 Enable
            [0x02] => Ok(Self::GetTps546En),
            // Set TPS546 Enable
            [0x02, level] => Ok(Self::SetTps546En { level: *level > 0 }),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        let level = match self {
            Command::GetAsicResetn => bool::from(controller.gpio.asic_resetn.output_level()),
            Command::SetAsicResetn { level } => {
                controller.gpio.asic_resetn.set_level((*level).into());
                *level
            }
            Command::GetVddio5vEn => bool::from(controller.gpio.vddio_5v_en.output_level()),
            Command::SetVddio5vEn { level } => {
                controller.gpio.vddio_5v_en.set_level((*level).into());
                *level
            }
            Command::GetTps546En => bool::from(controller.gpio.tps546_en.output_level()),
            Command::SetTps546En { level } => {
                controller.gpio.tps546_en.set_level((*level).into());
                *level
            }
        };

        Ok(Vec::from_slice(&[level as u8]).unwrap())
    }
}
