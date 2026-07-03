use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    gpio::Input,
    ledc::{
        channel::{Channel, ChannelHW},
        LowSpeed,
    },
};
use heapless::Vec;

use super::CommandError;

const TACH_SAMPLE_MS: u64 = 1000;
const TACH_PULSES_PER_REVOLUTION: u32 = 2;

pub struct Pins<'d> {
    pub pwm: Channel<'d, LowSpeed>,
    pub tach: Input<'d>,
    pub duty: u8,
}

#[derive(defmt::Format)]
pub enum Command {
    SetDuty { duty: u8 }, // 0x00
    ReadRpm,              // 0x01
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [0x00, duty] => Ok(Self::SetDuty { duty: *duty }),
            [0x01] => Ok(Self::ReadRpm),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl Pins<'_> {
    async fn read_rpm(&mut self) -> u32 {
        let start = Instant::now();
        let sample = Duration::from_millis(TACH_SAMPLE_MS);
        let mut pulses = 0u32;
        let mut was_high = self.tach.is_high();

        while Instant::now() - start < sample {
            let is_high = self.tach.is_high();

            if was_high && !is_high {
                pulses = pulses.saturating_add(1);
            }

            was_high = is_high;
            Timer::after_micros(250).await;
        }

        pulses * 60_000 / (TACH_SAMPLE_MS as u32 * TACH_PULSES_PER_REVOLUTION)
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            Command::SetDuty { duty } => {
                controller.fan.pwm.set_duty_hw(*duty as u32);
                controller.fan.duty = *duty;
                Ok(Vec::from_slice(&[*duty]).unwrap())
            }
            Command::ReadRpm => {
                let rpm = controller.fan.read_rpm().await;
                Ok(Vec::from_slice(&rpm.to_le_bytes()).unwrap())
            }
        }
    }
}
