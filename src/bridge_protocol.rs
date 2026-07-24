pub const INFO_SCHEMA_VERSION: u8 = 1;
pub const PROTOCOL_MAJOR: u8 = 1;
pub const VERSION_MAX_LENGTH: usize = 63;

pub const SAFETY_STATUS_SCHEMA_VERSION: u8 = 1;
pub const SAFETY_STATUS_LENGTH: usize = 17;

pub const PAGE_SYSTEM: u8 = 0x00;
pub const SYSTEM_GET_INFO: u8 = 0x01;
pub const SYSTEM_GET_RX_STATS: u8 = 0x02;
pub const SYSTEM_GET_SAFETY_STATUS: u8 = 0x10;
pub const SYSTEM_ARM_SAFETY_LEASE: u8 = 0x11;
pub const SYSTEM_SAFETY_HEARTBEAT: u8 = 0x12;
pub const SYSTEM_CLEAR_SAFETY_FAULT: u8 = 0x13;
pub const SYSTEM_DISARM_SAFETY_LEASE: u8 = 0x14;

pub const PAGE_GPIO: u8 = 0x06;
pub const GPIO_5V_ENABLE: u8 = 0x01;
pub const GPIO_ASIC_RESET: u8 = 0x02;
pub const GPIO_ASIC_TRIP: u8 = 0x03;

pub const PAGE_FAN: u8 = 0x09;
pub const FAN_SET_SPEED: u8 = 0x10;
pub const FAN_GET_TACH: u8 = 0x20;

pub const EVIDENCE_OUTPUTS_SAFE: u16 = 1 << 0;
pub const EVIDENCE_LEASE_VALID: u16 = 1 << 1;
pub const EVIDENCE_TRIP_CLEAR: u16 = 1 << 2;
pub const EVIDENCE_FAULT_CLEAR: u16 = 1 << 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    BufferTooSmall,
    InvalidFrame,
    InvalidResponse,
    Timeout,
    InvalidCommand,
    Denied,
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeInfo<'a> {
    pub protocol_major: u8,
    pub protocol_minor: u8,
    pub version: &'a str,
}

impl BridgeInfo<'_> {
    pub const fn is_compatible(self) -> bool {
        self.protocol_major == PROTOCOL_MAJOR
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SafetyState {
    SafeOff = 0,
    Controlled = 1,
    FaultLatched = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SafetyFault {
    None = 0,
    LeaseExpired = 1,
    AsicTrip = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SafetyStatus {
    pub stage: u8,
    pub state: SafetyState,
    pub fault: SafetyFault,
    pub runtime_verdict: u8,
    pub production_verdict: u8,
    pub capabilities: u16,
    pub evidence: u16,
    pub lease_remaining_ms: u32,
    pub five_volt_enabled: bool,
    pub asic_reset_asserted: bool,
    pub fan_full: bool,
    pub fan_percent: u8,
    pub trip_input_asserted: bool,
}

impl SafetyStatus {
    pub const fn outputs_safe(self) -> bool {
        !self.five_volt_enabled && self.asic_reset_asserted && self.fan_percent == 100
    }

    pub const fn trip_clear(self) -> bool {
        !self.trip_input_asserted && (self.evidence & EVIDENCE_TRIP_CLEAR) != 0
    }

    pub fn fault_clear(self) -> bool {
        self.fault == SafetyFault::None && (self.evidence & EVIDENCE_FAULT_CLEAR) != 0
    }

    pub fn lease_valid(self) -> bool {
        self.state == SafetyState::Controlled && self.lease_remaining_ms > 0 && (self.evidence & EVIDENCE_LEASE_VALID) != 0
    }
}

pub fn encode_request(id: u8, page: u8, command: u8, payload: &[u8], output: &mut [u8]) -> Result<usize, ProtocolError> {
    let frame_length = payload.len().checked_add(6).ok_or(ProtocolError::BufferTooSmall)?;
    if frame_length > u16::MAX as usize || frame_length > output.len() {
        return Err(ProtocolError::BufferTooSmall);
    }

    output[0..2].copy_from_slice(&(frame_length as u16).to_le_bytes());
    output[2] = id;
    output[3] = 0;
    output[4] = page;
    output[5] = command;
    output[6..frame_length].copy_from_slice(payload);
    Ok(frame_length)
}

pub fn declared_frame_length(frame: &[u8], maximum: usize) -> Result<Option<usize>, ProtocolError> {
    if frame.len() < 2 {
        return Ok(None);
    }

    let frame_length = u16::from_le_bytes([frame[0], frame[1]]) as usize;
    if !(3..=maximum).contains(&frame_length) {
        return Err(ProtocolError::InvalidFrame);
    }
    if frame.len() < frame_length {
        return Ok(None);
    }
    Ok(Some(frame_length))
}

pub fn decode_response(expected_id: u8, frame: &[u8]) -> Result<&[u8], ProtocolError> {
    let declared = declared_frame_length(frame, frame.len())?.ok_or(ProtocolError::InvalidFrame)?;
    if declared != frame.len() || frame[2] != expected_id {
        return Err(ProtocolError::InvalidResponse);
    }

    let payload = &frame[3..];
    if payload.len() == 1 {
        match payload[0] {
            0x10 => return Err(ProtocolError::Timeout),
            0x11 => return Err(ProtocolError::InvalidCommand),
            0x12 => return Err(ProtocolError::Denied),
            0x13 => return Err(ProtocolError::Fault),
            _ => {}
        }
    }
    Ok(payload)
}

pub fn decode_info(payload: &[u8]) -> Result<BridgeInfo<'_>, ProtocolError> {
    if payload.len() < 4 || payload[0] != INFO_SCHEMA_VERSION || payload[3] == 0 || payload[3] as usize > VERSION_MAX_LENGTH || payload.len() != 4 + payload[3] as usize {
        return Err(ProtocolError::InvalidResponse);
    }

    let version = core::str::from_utf8(&payload[4..]).map_err(|_| ProtocolError::InvalidResponse)?;
    if !version.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(ProtocolError::InvalidResponse);
    }

    Ok(BridgeInfo {
        protocol_major: payload[1],
        protocol_minor: payload[2],
        version,
    })
}

pub fn decode_safety_status(payload: &[u8]) -> Result<SafetyStatus, ProtocolError> {
    if payload.len() != SAFETY_STATUS_LENGTH
        || payload[0] != SAFETY_STATUS_SCHEMA_VERSION
        || payload[1] > 2
        || payload[2] > SafetyState::FaultLatched as u8
        || payload[3] > SafetyFault::AsicTrip as u8
        || payload[14] & !0x07 != 0
        || payload[15] > 100
        || payload[16] > 1
    {
        return Err(ProtocolError::InvalidResponse);
    }

    let state = match payload[2] {
        0 => SafetyState::SafeOff,
        1 => SafetyState::Controlled,
        2 => SafetyState::FaultLatched,
        _ => return Err(ProtocolError::InvalidResponse),
    };
    let fault = match payload[3] {
        0 => SafetyFault::None,
        1 => SafetyFault::LeaseExpired,
        2 => SafetyFault::AsicTrip,
        _ => return Err(ProtocolError::InvalidResponse),
    };
    let capabilities = u16::from_le_bytes([payload[6], payload[7]]);
    let evidence = u16::from_le_bytes([payload[8], payload[9]]);
    let lease_remaining_ms = u32::from_le_bytes([payload[10], payload[11], payload[12], payload[13]]);
    let five_volt_enabled = payload[14] & 0x01 != 0;
    let asic_reset_asserted = payload[14] & 0x02 != 0;
    let fan_full = payload[14] & 0x04 != 0;
    let fan_percent = payload[15];
    let trip_input_asserted = payload[16] != 0;

    if fan_full != (fan_percent == 100)
        || ((evidence & EVIDENCE_OUTPUTS_SAFE) != 0) != (!five_volt_enabled && asic_reset_asserted && fan_percent == 100)
        || ((evidence & EVIDENCE_TRIP_CLEAR) != 0) == trip_input_asserted
        || ((evidence & EVIDENCE_FAULT_CLEAR) != 0) != (fault == SafetyFault::None)
        || (state == SafetyState::FaultLatched) != (fault != SafetyFault::None)
        || (state != SafetyState::Controlled && lease_remaining_ms != 0)
    {
        return Err(ProtocolError::InvalidResponse);
    }

    Ok(SafetyStatus {
        stage: payload[1],
        state,
        fault,
        runtime_verdict: payload[4],
        production_verdict: payload[5],
        capabilities,
        evidence,
        lease_remaining_ms,
        five_volt_enabled,
        asic_reset_asserted,
        fan_full,
        fan_percent,
        trip_input_asserted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe_status_payload() -> [u8; SAFETY_STATUS_LENGTH] {
        [1, 2, 0, 0, 0, 0x81, 0x0f, 0, 0x0d, 0, 0, 0, 0, 0, 0x06, 100, 0]
    }

    #[test]
    fn request_encoding_matches_bridge_wire_format() {
        let mut output = [0u8; 16];
        let length = encode_request(0x2a, PAGE_SYSTEM, SYSTEM_GET_INFO, &[], &mut output).unwrap();
        assert_eq!(length, 6);
        assert_eq!(&output[..length], &[6, 0, 0x2a, 0, 0, 1]);
    }

    #[test]
    fn fragmented_and_complete_lengths_are_distinguished() {
        assert_eq!(declared_frame_length(&[5], 260), Ok(None));
        assert_eq!(declared_frame_length(&[5, 0, 1], 260), Ok(None));
        assert_eq!(declared_frame_length(&[5, 0, 1, 2, 3], 260), Ok(Some(5)));
        assert_eq!(declared_frame_length(&[2, 0], 260), Err(ProtocolError::InvalidFrame));
    }

    #[test]
    fn response_errors_are_not_mistaken_for_payloads() {
        assert_eq!(decode_response(7, &[4, 0, 7, 0x12]), Err(ProtocolError::Denied));
        assert_eq!(decode_response(7, &[4, 0, 7, 0x13]), Err(ProtocolError::Fault));
        assert_eq!(decode_response(8, &[4, 0, 7, 0]), Err(ProtocolError::InvalidResponse));
    }

    #[test]
    fn info_requires_protocol_schema_and_printable_version() {
        let payload = [1, 1, 0, 5, b'1', b'.', b'2', b'.', b'3'];
        let info = decode_info(&payload).unwrap();
        assert_eq!(info.protocol_major, 1);
        assert_eq!(info.protocol_minor, 0);
        assert_eq!(info.version, "1.2.3");
        assert!(info.is_compatible());

        let invalid = [1, 1, 0, 1, b'\n'];
        assert_eq!(decode_info(&invalid), Err(ProtocolError::InvalidResponse));
    }

    #[test]
    fn coherent_safe_and_controlled_statuses_decode() {
        let safe = decode_safety_status(&safe_status_payload()).unwrap();
        assert!(safe.outputs_safe());
        assert!(safe.trip_clear());
        assert!(safe.fault_clear());
        assert!(!safe.lease_valid());

        let mut controlled = safe_status_payload();
        controlled[2] = 1;
        controlled[4] = 1;
        controlled[8] = EVIDENCE_LEASE_VALID as u8 | EVIDENCE_TRIP_CLEAR as u8 | EVIDENCE_FAULT_CLEAR as u8;
        controlled[10..14].copy_from_slice(&1_750u32.to_le_bytes());
        controlled[14] = 0x04;
        let status = decode_safety_status(&controlled).unwrap();
        assert!(status.lease_valid());
        assert!(!status.outputs_safe());
    }

    #[test]
    fn incoherent_safety_evidence_is_rejected() {
        let mut payload = safe_status_payload();
        payload[8] &= !EVIDENCE_OUTPUTS_SAFE as u8;
        assert_eq!(decode_safety_status(&payload), Err(ProtocolError::InvalidResponse));

        let mut payload = safe_status_payload();
        payload[2] = SafetyState::FaultLatched as u8;
        assert_eq!(decode_safety_status(&payload), Err(ProtocolError::InvalidResponse));
    }
}
