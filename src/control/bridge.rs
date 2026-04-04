use embassy_time::{with_timeout, Duration};
use embedded_io_async::Write;
use heapless::Vec;

use super::CommandError;

const UART_TIMEOUT: Duration = Duration::from_millis(1000);
const FRAME_BUF_LEN: usize = 512;

pub struct BridgeControl {
    uart: crate::BridgeControlUart,
    rx_buf: [u8; FRAME_BUF_LEN],
    rx_len: usize,
}

impl BridgeControl {
    pub fn new(uart: crate::BridgeControlUart) -> Self {
        Self { uart, rx_buf: [0; FRAME_BUF_LEN], rx_len: 0 }
    }

    pub async fn transact(&mut self, id: u8, bus: u8, page: u8, command: u8, data: &[u8]) -> Result<Vec<u8, 256>, CommandError> {
        let mut request = Vec::<u8, 260>::new();
        request.extend_from_slice(&[0, 0, id, bus, page, command]).map_err(|_| CommandError::BufferOverflow)?;
        request.extend_from_slice(data).map_err(|_| CommandError::BufferOverflow)?;

        let len = (request.len() as u16).to_le_bytes();
        request[0..2].copy_from_slice(&len);

        self.uart.write_all(&request).await.map_err(|_| CommandError::Message("Bridge UART Write Error"))?;
        self.uart.flush_async().await.map_err(|_| CommandError::Message("Bridge UART Flush Error"))?;

        let response = with_timeout(UART_TIMEOUT, self.read_frame()).await.map_err(|_| CommandError::Timeout)??;
        if response.len() < 3 || response[2] != id {
            return Err(CommandError::Message("Bridge UART Response Error"));
        }

        Vec::from_slice(&response[3..]).map_err(|_| CommandError::BufferOverflow)
    }

    async fn read_frame(&mut self) -> Result<Vec<u8, 260>, CommandError> {
        loop {
            if let Some(frame_len) = try_extract_frame_len(&self.rx_buf[..self.rx_len])? {
                let frame = Vec::from_slice(&self.rx_buf[..frame_len]).map_err(|_| CommandError::BufferOverflow)?;
                let excess = self.rx_len - frame_len;
                if excess > 0 {
                    self.rx_buf.copy_within(frame_len..self.rx_len, 0);
                }
                self.rx_len = excess;
                return Ok(frame);
            }

            let n = self.uart.read_async(&mut self.rx_buf[self.rx_len..]).await.map_err(|_| CommandError::Message("Bridge UART Read Error"))?;
            if n == 0 {
                continue;
            }
            self.rx_len += n;
        }
    }
}

fn try_extract_frame_len(buf: &[u8]) -> Result<Option<usize>, CommandError> {
    if buf.len() < 2 {
        return Ok(None);
    }

    let frame_len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    if !(3..=FRAME_BUF_LEN).contains(&frame_len) {
        return Err(CommandError::Invalid);
    }

    if buf.len() < frame_len {
        return Ok(None);
    }

    Ok(Some(frame_len))
}
