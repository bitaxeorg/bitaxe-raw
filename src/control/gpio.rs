use super::CommandError;
use heapless::Vec;

#[derive(defmt::Format)]
pub enum Command {
    SetAsicResetn { level: bool },
    GetAsicResetn,
    Set5vEn { level: bool },
    Get5vEn,
    SetAsicRst { level: bool },
    GetAsicRst,
    GetAsicTrip,
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
            _ => Err(CommandError::Invalid),
        }
    }
}

impl super::ControllerCommand for Command {
    async fn handle(&self, controller: &mut super::Controller) -> Result<Vec<u8, 256>, CommandError> {
        match self {
            // Preserve the original bitaxe-raw-bonanza RST_N command ID and semantics.
            Command::GetAsicResetn => controller.bridge.transact(0, 0, 0x06, 0x02, &[]).await,
            Command::SetAsicResetn { level } => controller.bridge.transact(0, 0, 0x06, 0x02, &[*level as u8]).await,
            Command::Get5vEn => controller.bridge.transact(0, 0, 0x06, 0x01, &[]).await,
            Command::Set5vEn { level } => controller.bridge.transact(0, 0, 0x06, 0x01, &[*level as u8]).await,
            Command::GetAsicRst => controller.bridge.transact(0, 0, 0x06, 0x02, &[]).await,
            Command::SetAsicRst { level } => controller.bridge.transact(0, 0, 0x06, 0x02, &[*level as u8]).await,
            Command::GetAsicTrip => controller.bridge.transact(0, 0, 0x06, 0x03, &[]).await,
        }
    }
}
