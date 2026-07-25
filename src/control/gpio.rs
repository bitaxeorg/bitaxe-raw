use super::CommandError;
use crate::bridge_protocol;
use esp_hal::gpio::{Input, Level, Output};
use heapless::Vec;

pub struct Pins<'d> {
    pub vr_en: Output<'d>,
    pub vr_pgood: Input<'d>,
}

#[derive(defmt::Format)]
pub enum Command {
    SetAsicResetn { level: bool },
    GetAsicResetn,
    Set5vEn { level: bool },
    Get5vEn,
    SetAsicRst { level: bool },
    GetAsicRst,
    GetAsicTrip,
    SetVrEn { level: bool },
    GetVrEn,
    GetVrPgood,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            // Get ASIC Reset (Active Low)
            [0x00] => Ok(Self::GetAsicResetn),
            // Set ASIC Reset (Active Low)
            [0x00, level] => Ok(Self::SetAsicResetn { level: *level > 0 }),
            [0x01] => Ok(Self::Get5vEn),
            [0x01, level] => Ok(Self::Set5vEn { level: *level > 0 }),
            [0x02] => Ok(Self::GetAsicRst),
            [0x02, level] => Ok(Self::SetAsicRst { level: *level > 0 }),
            [0x03] => Ok(Self::GetAsicTrip),
            [0x04] => Ok(Self::GetVrEn),
            [0x04, level] => Ok(Self::SetVrEn { level: *level > 0 }),
            [0x05] => Ok(Self::GetVrPgood),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            // Preserve the original bitaxe-raw-bonanza RST_N command ID and semantics.
            Command::GetAsicResetn => super::bridge::gpio(bridge_protocol::GPIO_ASIC_RESET, None).await,
            Command::SetAsicResetn { level } => super::bridge::gpio(bridge_protocol::GPIO_ASIC_RESET, Some(*level)).await,
            Command::Get5vEn => super::bridge::gpio(bridge_protocol::GPIO_5V_ENABLE, None).await,
            Command::Set5vEn { level } => super::bridge::gpio(bridge_protocol::GPIO_5V_ENABLE, Some(*level)).await,
            Command::GetAsicRst => super::bridge::gpio(bridge_protocol::GPIO_ASIC_RESET, None).await,
            Command::SetAsicRst { level } => super::bridge::gpio(bridge_protocol::GPIO_ASIC_RESET, Some(*level)).await,
            Command::GetAsicTrip => super::bridge::gpio(bridge_protocol::GPIO_ASIC_TRIP, None).await,
            Command::GetVrEn => Ok(Vec::from_slice(&[controller.gpio.vr_en.is_set_high() as u8]).unwrap()),
            Command::SetVrEn { level } => {
                controller.gpio.vr_en.set_level(Level::from(*level));
                Ok(Vec::from_slice(&[*level as u8]).unwrap())
            }
            Command::GetVrPgood => Ok(Vec::from_slice(&[controller.gpio.vr_pgood.is_high() as u8]).unwrap()),
        }
    }
}
