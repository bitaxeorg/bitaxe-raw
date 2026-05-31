pub enum UartTaskError {
    Disconnected,
    UartError,
}

use embassy_embedded_hal::SetConfig;
use embassy_futures::select::{select3, Either3};
use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, ControlChanged, Receiver, Sender},
    driver::EndpointError,
};
use embedded_io_async::Write;
use esp_hal::{
    uart::{Config, ConfigError, RxError, TxError, Uart, UartRx, UartTx},
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

impl From<ConfigError> for UartTaskError {
    fn from(val: ConfigError) -> Self {
        match val {
            _ => UartTaskError::UartError,
        }
    }
}

#[embassy_executor::task]
pub async fn usb_task(class: CdcAcmClass<'static, super::UsbDriver>, uart: Uart<'static, Async>) -> ! {
    let (mut tx, mut rx, mut ctrl) = class.split_with_control();

    let (mut uart_rx, mut uart_tx) = uart.split();

    loop {
        rx.wait_connection().await;
        let _ = pipe_uart(&mut tx, &mut rx, &mut ctrl, &mut uart_rx, &mut uart_tx).await;
    }
}

/// Handle ASIC UART <-> BMC USB TTY forwarding and baudrate changes
pub async fn pipe_uart<'d>(
    usb_tx: &mut Sender<'static, super::UsbDriver>,
    usb_rx: &mut Receiver<'static, super::UsbDriver>,
    ctrl: &mut ControlChanged<'static>,
    uart_rx: &mut UartRx<'d, Async>,
    uart_tx: &mut UartTx<'d, Async>,
) -> Result<(), UartTaskError> {
    let mut usb_buf = [0; 64];
    let mut uart_buf = [0; 1024];

    let line_coding = usb_rx.line_coding();
    let baudrate = line_coding.data_rate();
    if baudrate != 0 {
        let config = Config::default().with_baudrate(baudrate);
        uart_tx.set_config(&config)?;
        uart_rx.set_config(&config)?;
    }

    loop {
        let usb_read = usb_rx.read_packet(&mut usb_buf);
        let uart_read = uart_rx.read_async(&mut uart_buf);

        let control_change = ctrl.control_changed();

        match select3(usb_read, uart_read, control_change).await {
            // Forward data from the USB host to the UART
            Either3::First(n) => {
                let data = &usb_buf[..n?];
                uart_tx.write_all(data).await?;
            }
            // Forward data from the UART back to the USB host
            Either3::Second(n) => {
                let data = &uart_buf[..n?];
                usb_tx.write_packet(data).await?;
            }
            // Handle baudrate changes from USB CDC control requests
            Either3::Third(()) => {
                let line_coding = usb_rx.line_coding();
                let baudrate = line_coding.data_rate();
                let config = Config::default().with_baudrate(baudrate);
                uart_tx.set_config(&config)?;
                uart_rx.set_config(&config)?;
            }
        }
    }
}
