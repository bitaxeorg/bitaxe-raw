pub enum UartTaskError {
    Disconnected,
    UartError,
}

use embassy_futures::select::{select, Either};
use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, Receiver, Sender},
    driver::EndpointError,
};
use embedded_io_async::Write;
use esp_hal::{
    uart::{RxError, TxError, Uart, UartRx, UartTx},
    Async,
};

impl From<EndpointError> for UartTaskError {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => UartTaskError::Disconnected {},
        }
    }
}

impl From<TxError> for UartTaskError {
    fn from(val: TxError) -> Self {
        match val {
            _ => UartTaskError::UartError,
        }
    }
}

impl From<RxError> for UartTaskError {
    fn from(val: RxError) -> Self {
        match val {
            _ => UartTaskError::UartError,
        }
    }
}

#[embassy_executor::task]
pub async fn usb_task(class: CdcAcmClass<'static, super::UsbDriver>, uart: Uart<'static, Async>) -> ! {
    let (mut tx, mut rx, mut _ctrl) = class.split_with_control();

    let (mut uart_rx, mut uart_tx) = uart.split();

    loop {
        rx.wait_connection().await;
        let _ = pipe_uart(&mut tx, &mut rx, &mut uart_rx, &mut uart_tx).await;
    }
}

/// Handle ASIC UART <-> BMC USB TTY forwarding.
///
/// USB CDC line coding is intentionally ignored. The physical ESP-to-Bridge
/// UART must remain at the Bridge protocol's fixed baud rate.
pub async fn pipe_uart<'d>(usb_tx: &mut Sender<'static, super::UsbDriver>, usb_rx: &mut Receiver<'static, super::UsbDriver>, uart_rx: &mut UartRx<'d, Async>, uart_tx: &mut UartTx<'d, Async>) -> Result<(), UartTaskError> {
    let mut usb_buf = [0; 64];
    let mut uart_buf = [0; 1024];

    loop {
        let usb_read = usb_rx.read_packet(&mut usb_buf);
        let uart_read = uart_rx.read_async(&mut uart_buf);

        match select(usb_read, uart_read).await {
            // Forward data from the USB host to the UART
            Either::First(n) => {
                let data = &usb_buf[..n?];
                uart_tx.write_all(data).await?;
            }
            // Forward data from the UART back to the USB host
            Either::Second(n) => {
                let data = &uart_buf[..n?];
                for packet in data.chunks(64) {
                    usb_tx.write_packet(packet).await?;
                }
            }
        }
    }
}
