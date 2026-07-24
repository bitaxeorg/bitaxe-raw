use core::sync::atomic::{AtomicU8, Ordering};

use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{with_timeout, Duration, Ticker};
use embedded_io_async::Write;
use heapless::Vec;

use super::CommandError;
use crate::bridge_protocol::{self, LeasePreparation, ProtocolError, SafetyStatus};

const UART_TIMEOUT: Duration = Duration::from_millis(1000);
const FRAME_BUF_LEN: usize = 512;
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);
const REQUEST_QUEUE_LEN: usize = 8;

static NEXT_REQUEST_ID: AtomicU8 = AtomicU8::new(0);
static REQUEST_CHANNEL: Channel<CriticalSectionRawMutex, RequestEnvelope, REQUEST_QUEUE_LEN> = Channel::new();
static REPLY_CHANNEL: Channel<CriticalSectionRawMutex, ReplyEnvelope, REQUEST_QUEUE_LEN> = Channel::new();

#[derive(defmt::Format)]
enum Request {
    Gpio { command: u8, level: Option<bool> },
    Fan { command: u8, speed: Option<u8> },
    Shutdown,
}

#[derive(defmt::Format)]
struct RequestEnvelope {
    id: u8,
    request: Request,
}

struct ReplyEnvelope {
    id: u8,
    result: Result<Vec<u8, 256>, CommandError>,
}

async fn request(request: Request) -> Result<Vec<u8, 256>, CommandError> {
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    REQUEST_CHANNEL.send(RequestEnvelope { id, request }).await;

    // The control task is the sole regular caller. IDs also make cancellation
    // safe: a reply abandoned by a disconnected USB client is skipped by the
    // next request instead of being mistaken for its result.
    loop {
        let reply = REPLY_CHANNEL.receive().await;
        if reply.id == id {
            return reply.result;
        }
    }
}

pub async fn gpio(command: u8, level: Option<bool>) -> Result<Vec<u8, 256>, CommandError> {
    request(Request::Gpio { command, level }).await
}

pub async fn fan(command: u8, speed: Option<u8>) -> Result<Vec<u8, 256>, CommandError> {
    request(Request::Fan { command, speed }).await
}

pub async fn shutdown() {
    let _ = request(Request::Shutdown).await;
}

pub struct BridgeControl {
    uart: crate::BridgeControlUart,
    rx_buf: [u8; FRAME_BUF_LEN],
    rx_len: usize,
    next_id: u8,
}

impl BridgeControl {
    pub fn new(uart: crate::BridgeControlUart) -> Self {
        Self {
            uart,
            rx_buf: [0; FRAME_BUF_LEN],
            rx_len: 0,
            next_id: 0,
        }
    }

    pub async fn transact(&mut self, page: u8, command: u8, data: &[u8]) -> Result<Vec<u8, 256>, CommandError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let mut request = [0u8; 260];
        let request_len = bridge_protocol::encode_request(id, page, command, data, &mut request).map_err(CommandError::from_protocol)?;

        self.uart.write_all(&request[..request_len]).await.map_err(|_| CommandError::Message("Bridge UART Write Error"))?;
        self.uart.flush_async().await.map_err(|_| CommandError::Message("Bridge UART Flush Error"))?;

        let result = with_timeout(UART_TIMEOUT, self.read_response(id)).await;
        match result {
            Ok(result) => result,
            Err(_) => {
                self.rx_len = 0;
                Err(CommandError::Timeout)
            }
        }
    }

    async fn read_response(&mut self, expected_id: u8) -> Result<Vec<u8, 256>, CommandError> {
        loop {
            let frame_length = match bridge_protocol::declared_frame_length(&self.rx_buf[..self.rx_len], FRAME_BUF_LEN) {
                Ok(frame_length) => frame_length,
                Err(error) => {
                    self.rx_len = 0;
                    return Err(CommandError::from_protocol(error));
                }
            };
            if let Some(frame_len) = frame_length {
                let response_id = self.rx_buf[2];
                let decoded = if response_id == expected_id {
                    bridge_protocol::decode_response(expected_id, &self.rx_buf[..frame_len]).map(Vec::from_slice)
                } else {
                    Err(ProtocolError::InvalidResponse)
                };

                let excess = self.rx_len - frame_len;
                if excess > 0 {
                    self.rx_buf.copy_within(frame_len..self.rx_len, 0);
                }
                self.rx_len = excess;

                if response_id != expected_id {
                    continue;
                }
                return decoded.map_err(CommandError::from_protocol)?.map_err(|_| CommandError::BufferOverflow);
            }

            if self.rx_len == FRAME_BUF_LEN {
                self.rx_len = 0;
                return Err(CommandError::Invalid);
            }

            let n = self.uart.read_async(&mut self.rx_buf[self.rx_len..]).await.map_err(|_| CommandError::Message("Bridge UART Read Error"))?;
            if n == 0 {
                continue;
            }
            self.rx_len += n;
        }
    }
}

struct BridgeManager {
    control: BridgeControl,
    lease_active: bool,
}

impl BridgeManager {
    fn new(uart: crate::BridgeControlUart) -> Self {
        Self {
            control: BridgeControl::new(uart),
            lease_active: false,
        }
    }

    async fn handle(&mut self, request: Request) -> Result<Vec<u8, 256>, CommandError> {
        match request {
            Request::Gpio { command, level } => self.handle_gpio(command, level).await,
            Request::Fan { command, speed } => self.handle_fan(command, speed).await,
            Request::Shutdown => {
                self.fail_safe().await;
                Ok(Vec::new())
            }
        }
    }

    async fn handle_gpio(&mut self, command: u8, level: Option<bool>) -> Result<Vec<u8, 256>, CommandError> {
        let unsafe_transition = matches!((command, level), (bridge_protocol::GPIO_5V_ENABLE, Some(true)) | (bridge_protocol::GPIO_ASIC_RESET, Some(true)));
        if unsafe_transition {
            if let Err(error) = self.ensure_controlled().await {
                self.fail_safe().await;
                return Err(error);
            }
        }

        let payload = level.map(|level| [level as u8]);
        let result = self.control.transact(bridge_protocol::PAGE_GPIO, command, payload.as_ref().map_or(&[], |value| value.as_slice())).await;

        if result.is_err() {
            self.fail_safe().await;
        } else if command == bridge_protocol::GPIO_5V_ENABLE && level == Some(false) {
            self.disarm().await;
        }
        result
    }

    async fn handle_fan(&mut self, command: u8, speed: Option<u8>) -> Result<Vec<u8, 256>, CommandError> {
        if command == bridge_protocol::FAN_SET_SPEED && speed.is_some_and(|speed| speed < 100) {
            if let Err(error) = self.ensure_controlled().await {
                self.fail_safe().await;
                return Err(error);
            }
        }

        let payload = speed.map(|speed| [speed]);
        let result = self.control.transact(bridge_protocol::PAGE_FAN, command, payload.as_ref().map_or(&[], |value| value.as_slice())).await;
        if result.is_err() {
            self.fail_safe().await;
        }
        result
    }

    async fn ensure_controlled(&mut self) -> Result<(), CommandError> {
        if self.lease_active {
            return Ok(());
        }

        let info_payload = self.control.transact(bridge_protocol::PAGE_SYSTEM, bridge_protocol::SYSTEM_GET_INFO, &[]).await?;
        let info = bridge_protocol::decode_info(&info_payload).map_err(CommandError::from_protocol)?;
        if !info.is_compatible() {
            return Err(CommandError::Denied);
        }

        let mut status = self.safety_command(bridge_protocol::SYSTEM_GET_SAFETY_STATUS).await?;
        match status.lease_preparation().map_err(CommandError::from_protocol)? {
            LeasePreparation::Adopt => {}
            LeasePreparation::Arm => {
                status = self.safety_command(bridge_protocol::SYSTEM_ARM_SAFETY_LEASE).await?;
            }
            LeasePreparation::ClearThenArm => {
                status = self.safety_command(bridge_protocol::SYSTEM_CLEAR_SAFETY_FAULT).await?;
                if status.lease_preparation().map_err(CommandError::from_protocol)? != LeasePreparation::Arm {
                    return Err(CommandError::Fault);
                }
                status = self.safety_command(bridge_protocol::SYSTEM_ARM_SAFETY_LEASE).await?;
            }
        }

        if !status.lease_valid() || !status.trip_clear() || !status.fault_clear() {
            return Err(CommandError::Fault);
        }
        self.lease_active = true;
        Ok(())
    }

    async fn heartbeat(&mut self) -> Result<(), CommandError> {
        if !self.lease_active {
            return Ok(());
        }

        let status = self.safety_command(bridge_protocol::SYSTEM_SAFETY_HEARTBEAT).await?;
        if !status.lease_valid() || !status.trip_clear() || !status.fault_clear() {
            return Err(CommandError::Fault);
        }
        Ok(())
    }

    async fn safety_command(&mut self, command: u8) -> Result<SafetyStatus, CommandError> {
        let payload = self.control.transact(bridge_protocol::PAGE_SYSTEM, command, &[]).await?;
        bridge_protocol::decode_safety_status(&payload).map_err(CommandError::from_protocol)
    }

    async fn disarm(&mut self) {
        let _ = self.safety_command(bridge_protocol::SYSTEM_DISARM_SAFETY_LEASE).await;
        self.lease_active = false;
    }

    async fn fail_safe(&mut self) {
        self.lease_active = false;
        let _ = self.control.transact(bridge_protocol::PAGE_GPIO, bridge_protocol::GPIO_ASIC_RESET, &[0]).await;
        let _ = self.control.transact(bridge_protocol::PAGE_GPIO, bridge_protocol::GPIO_5V_ENABLE, &[0]).await;
        let _ = self.control.transact(bridge_protocol::PAGE_FAN, bridge_protocol::FAN_SET_SPEED, &[100]).await;
        let _ = self.control.transact(bridge_protocol::PAGE_SYSTEM, bridge_protocol::SYSTEM_DISARM_SAFETY_LEASE, &[]).await;
    }
}

#[embassy_executor::task]
pub async fn manager_task(uart: crate::BridgeControlUart) -> ! {
    let mut manager = BridgeManager::new(uart);
    let mut heartbeat_ticker = Ticker::every(HEARTBEAT_INTERVAL);

    loop {
        match select(REQUEST_CHANNEL.receive(), heartbeat_ticker.next()).await {
            Either::First(envelope) => {
                if manager.heartbeat().await.is_err() {
                    manager.fail_safe().await;
                }
                let result = manager.handle(envelope.request).await;
                REPLY_CHANNEL.send(ReplyEnvelope { id: envelope.id, result }).await;
            }
            Either::Second(()) => {
                if manager.heartbeat().await.is_err() {
                    manager.fail_safe().await;
                }
            }
        }
    }
}
