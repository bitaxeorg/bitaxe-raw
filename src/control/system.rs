use heapless::Vec;

use super::CommandError;
use crate::bridge_protocol;

#[derive(defmt::Format)]
pub enum Command {
    GetInfo,
    GetRxStats,
    GetSafetyStatus,
}

impl Command {
    pub fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        match buf {
            [bridge_protocol::SYSTEM_GET_INFO] => Ok(Self::GetInfo),
            [bridge_protocol::SYSTEM_GET_RX_STATS] => Ok(Self::GetRxStats),
            [bridge_protocol::SYSTEM_GET_SAFETY_STATUS] => Ok(Self::GetSafetyStatus),
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, _controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        let command = match self {
            Self::GetInfo => bridge_protocol::SYSTEM_GET_INFO,
            Self::GetRxStats => bridge_protocol::SYSTEM_GET_RX_STATS,
            Self::GetSafetyStatus => bridge_protocol::SYSTEM_GET_SAFETY_STATUS,
        };
        super::bridge::system(command).await
    }
}
