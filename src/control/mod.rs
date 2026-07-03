use defmt::info;

use embassy_futures::join::join;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, TimeoutError};
use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, Receiver, Sender},
    driver::EndpointError,
};
use heapless::Vec;

pub mod i2c;
const I2C_COMMAND: u8 = 5;

pub mod gpio;
const GPIO_COMMAND: u8 = 6;

pub mod adc;
const ADC_COMMAND: u8 = 7;

pub mod fan;
const FAN_COMMAND: u8 = 8;

#[derive(defmt::Format)]
struct Command {
    id: i8,
    bus: u8,
    inner: CommandInner,
}

#[derive(defmt::Format)]
enum CommandInner {
    I2c(i2c::Command),
    Gpio(gpio::Command),
    Adc(adc::Command),
    Fan(fan::Command),
    Error(CommandError),
}

impl Command {
    fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        let id = buf[0] as i8;
        match buf[2] {
            I2C_COMMAND => Ok(Self {
                id,
                bus: buf[1],
                inner: CommandInner::I2c(i2c::Command::from_bytes(&buf[3..])?),
            }),
            GPIO_COMMAND => Ok(Self {
                id,
                bus: buf[1],
                inner: CommandInner::Gpio(gpio::Command::from_bytes(&buf[3..])?),
            }),
            ADC_COMMAND => Ok(Self {
                id,
                bus: buf[1],
                inner: CommandInner::Adc(adc::Command::from_bytes(&buf[3..])?),
            }),
            FAN_COMMAND => Ok(Self {
                id,
                bus: buf[1],
                inner: CommandInner::Fan(fan::Command::from_bytes(&buf[3..])?),
            }),
            _ => Err(CommandError::Invalid),
        }
    }
}

#[derive(defmt::Format)]
pub enum CommandError {
    Timeout,               // 0x10
    Invalid,               // 0x11
    BufferOverflow,        // 0x12
    Message(&'static str), // 0xff
}

impl CommandError {
    fn to_bytes(&self) -> Vec<u8, 260> {
        let mut buf = Vec::<u8, 260>::new();
        buf.extend_from_slice(&[0x00, 0x00, 0xff]).unwrap();

        match self {
            CommandError::Timeout => {
                buf.push(0x10).unwrap();
            }
            CommandError::Invalid => {
                buf.push(0x11).unwrap();
            }
            CommandError::BufferOverflow => {
                buf.push(0x12).unwrap();
            }
            CommandError::Message(msg) => {
                buf.push(0xff).unwrap();
                buf.extend_from_slice(msg.as_bytes()).unwrap();
            }
        }

        let len = (buf.len() as u16).to_le_bytes();
        buf[0..2].clone_from_slice(&len);
        buf
    }
}

static COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, Command, 8> = Channel::new();

pub struct Controller {
    tx: Sender<'static, super::UsbDriver>,
    i2c: super::I2cDriver,
    gpio: gpio::Pins<'static>,
    adc: adc::Pins<'static>,
    fan: fan::Pins<'static>,
}

pub trait ControllerCommand {
    async fn handle(&self, controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError>;
}

impl Controller {
    pub async fn run(&mut self) {
        loop {
            let cmd = COMMAND_CHANNEL.receive().await;
            let res = match cmd.inner {
                CommandInner::I2c(cmd) => cmd.handle(self).await,
                CommandInner::Gpio(cmd) => cmd.handle(self).await,
                CommandInner::Adc(cmd) => cmd.handle(self).await,
                CommandInner::Fan(cmd) => cmd.handle(self).await,
                CommandInner::Error(err) => Err(err),
            };

            let buf = match res {
                Ok(res) => {
                    let mut buf = Vec::<u8, 260>::new();
                    buf.extend_from_slice(&(res.len() as u16).to_le_bytes()).unwrap();
                    buf.push(cmd.id as u8).unwrap();
                    buf.extend_from_slice(&res).unwrap();
                    buf
                }
                Err(err) => {
                    let mut buf = err.to_bytes();
                    buf[2] = cmd.id as u8;
                    buf
                }
            };

            let _ = self.tx.write_packet(&buf).await;
        }
    }
}

#[embassy_executor::task]
pub async fn usb_task(class: CdcAcmClass<'static, super::UsbDriver>, i2c: super::I2cDriver, gpio: gpio::Pins<'static>, adc: adc::Pins<'static>, fan: fan::Pins<'static>) -> ! {
    let (tx, mut rx, mut _ctrl) = class.split_with_control();
    let mut controller = Controller { tx, i2c, gpio, adc, fan };

    loop {
        rx.wait_connection().await;
        info!("Control: Connected");
        let _ = join(pipe_usb_read(&mut rx), controller.run()).await;
        info!("Control: Disconnected");
    }
}

enum ControlTaskError {
    Disconnected,
}

impl From<EndpointError> for ControlTaskError {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => ControlTaskError::Disconnected {},
        }
    }
}

async fn pipe_usb_read(rx: &mut Receiver<'static, super::UsbDriver>) -> Result<(), ControlTaskError> {
    let mut buf = [0; 4098];

    loop {
        let mut num_read: usize = 0;

        'read: loop {
            let read = rx.read_packet(&mut buf[num_read..]);

            match embassy_time::with_timeout(Duration::from_millis(4), read).await {
                Ok(Ok(n)) => {
                    num_read += n;

                    if num_read >= 5 {
                        let to_read = u16::from_le_bytes(buf[0..2].try_into().unwrap()) as usize;

                        if num_read >= to_read {
                            let excess = num_read - to_read;

                            match Command::from_bytes(&buf[2..to_read]) {
                                Ok(cmd) => COMMAND_CHANNEL.send(cmd).await,
                                Err(err) => COMMAND_CHANNEL.send(Command { id: -1, bus: 0, inner: CommandInner::Error(err) }).await,
                            }

                            let mut new_buf = [0; 4098];
                            new_buf[..excess].clone_from_slice(&buf[to_read..to_read + excess]);

                            num_read = excess;
                            buf = new_buf;
                        }
                    }
                }

                Ok(Err(err)) => {
                    return Err(err.into());
                }

                Err(TimeoutError) => {
                    let _error = CommandError::Timeout;
                    break 'read;
                }
            }
        }
    }
}
