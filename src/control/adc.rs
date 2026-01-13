use esp_hal::analog::adc::{Adc, AdcPin};
use heapless::Vec;

use super::CommandError;

pub struct Pins<'d> {
    pub adc: Adc<'d, esp_hal::peripherals::ADC1<'d>, esp_hal::Blocking>,
    pub vdd: AdcPin<esp_hal::peripherals::GPIO2<'d>, esp_hal::peripherals::ADC1<'d>>,
}

#[derive(defmt::Format)]
pub enum Command {
    ReadVdd, // 0x50
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        //defmt::println!("ADC COMMAND {:x}", buf);
        match buf {
            [0x50] => Ok(Self::ReadVdd),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        let adc = &mut controller.adc.adc;

        let value = match self {
            Command::ReadVdd => adc.read_oneshot(&mut controller.adc.vdd),
        }
        .map_err(|_| CommandError::Message("ADC Read Error"))?;

        Ok(Vec::from_slice(&value.to_le_bytes()).unwrap())
    }
}
