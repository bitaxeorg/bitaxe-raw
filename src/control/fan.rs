use heapless::Vec;

use super::CommandError;
use crate::bridge_protocol;

#[derive(defmt::Format)]
pub enum Command {
    SetSpeed(u8),
    GetTach,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [0x10, speed] if *speed <= 100 => Ok(Self::SetSpeed(*speed)),
            [0x20] => Ok(Self::GetTach),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            Command::SetSpeed(speed) => controller.bridge.transact(bridge_protocol::PAGE_FAN, bridge_protocol::FAN_SET_SPEED, &[*speed]).await,
            Command::GetTach => controller.bridge.transact(bridge_protocol::PAGE_FAN, bridge_protocol::FAN_GET_TACH, &[]).await,
        }
    }
}
