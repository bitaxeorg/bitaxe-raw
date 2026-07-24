use embassy_time::{with_timeout, Duration};
use embedded_io_async::Write;
use heapless::Vec;

use super::CommandError;
use crate::bridge_protocol::{self, ProtocolError};

const UART_TIMEOUT: Duration = Duration::from_millis(1000);
const FRAME_BUF_LEN: usize = 512;

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
