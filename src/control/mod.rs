use defmt::info;

use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_usb::{
    class::cdc_acm::{CdcAcmClass, Receiver, Sender},
    driver::EndpointError,
};
use heapless::Vec;

pub mod i2c;
const I2C_COMMAND: u8 = 5;

pub mod system;
const SYSTEM_COMMAND: u8 = 0;

pub mod bridge;
pub mod gpio;
const GPIO_COMMAND: u8 = 6;

pub mod adc;
const ADC_COMMAND: u8 = 7;

pub mod fan;
const FAN_COMMAND: u8 = 9;

// pub mod led;
// const LED_COMMAND: u8 = 8;

#[derive(defmt::Format)]
struct Command {
    id: u8,
    bus: u8,
    inner: CommandInner,
}

#[derive(defmt::Format)]
enum CommandInner {
    System(system::Command),
    I2c(i2c::Command),
    Gpio(gpio::Command),
    Adc(adc::Command),
    Fan(fan::Command),
    Error(CommandError),
}

impl Command {
    fn from_bytes(buf: &[u8]) -> Result<Self, CommandError> {
        let [id, bus, page, data @ ..] = buf else {
            return Err(CommandError::Invalid);
        };

        match *page {
            SYSTEM_COMMAND => Ok(Self {
                id: *id,
                bus: *bus,
                inner: CommandInner::System(system::Command::from_bytes(data)?),
            }),
            I2C_COMMAND => Ok(Self {
                id: *id,
                bus: *bus,
                inner: CommandInner::I2c(i2c::Command::from_bytes(data)?),
            }),
            GPIO_COMMAND => Ok(Self {
                id: *id,
                bus: *bus,
                inner: CommandInner::Gpio(gpio::Command::from_bytes(data)?),
            }),
            ADC_COMMAND => Ok(Self {
                id: *id,
                bus: *bus,
                inner: CommandInner::Adc(adc::Command::from_bytes(data)?),
            }),
            FAN_COMMAND => Ok(Self {
                id: *id,
                bus: *bus,
                inner: CommandInner::Fan(fan::Command::from_bytes(data)?),
            }),
            _ => Err(CommandError::Invalid),
        }
    }
}

#[derive(defmt::Format)]
pub enum CommandError {
    Timeout, // 0x10
    Invalid, // 0x11
    Denied,  // 0x12
    Fault,   // 0x13
    BufferOverflow,
    Message(&'static str), // 0xff
}

impl CommandError {
    fn from_protocol(error: crate::bridge_protocol::ProtocolError) -> Self {
        use crate::bridge_protocol::ProtocolError;

        match error {
            ProtocolError::Timeout => Self::Timeout,
            ProtocolError::InvalidCommand | ProtocolError::InvalidFrame | ProtocolError::InvalidResponse => Self::Invalid,
            ProtocolError::Denied => Self::Denied,
            ProtocolError::Fault => Self::Fault,
            ProtocolError::BufferTooSmall => Self::BufferOverflow,
        }
    }

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
            CommandError::Denied => {
                buf.push(0x12).unwrap();
            }
            CommandError::Fault => {
                buf.push(0x13).unwrap();
            }
            CommandError::BufferOverflow => {
                buf.extend_from_slice(&[0xff, b'B', b'u', b'f']).unwrap();
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
    adc: adc::Pins<'static>,
    gpio: gpio::Pins<'static>,
}

pub trait ControllerCommand {
    async fn handle(&self, controller: &mut Controller) -> Result<Vec<u8, 256>, CommandError>;
}

impl Controller {
    pub async fn run(&mut self) {
        loop {
            let cmd = COMMAND_CHANNEL.receive().await;
            let res = match cmd.inner {
                CommandInner::System(cmd) => cmd.handle(self).await,
                CommandInner::I2c(cmd) => cmd.handle(self).await,
                CommandInner::Gpio(cmd) => cmd.handle(self).await,
                CommandInner::Adc(cmd) => cmd.handle(self).await,
                CommandInner::Fan(cmd) => cmd.handle(self).await,
                CommandInner::Error(err) => Err(err),
            };

            let buf = match res {
                Ok(res) => {
                    let mut buf = Vec::<u8, 260>::new();
                    buf.extend_from_slice(&((res.len() + 3) as u16).to_le_bytes()).unwrap();
                    buf.push(cmd.id).unwrap();
                    buf.extend_from_slice(&res).unwrap();
                    buf
                }
                Err(err) => {
                    let mut buf = err.to_bytes();
                    buf[2] = cmd.id;
                    buf
                }
            };

            for packet in buf.chunks(64) {
                if self.tx.write_packet(packet).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[embassy_executor::task]
pub async fn usb_task(class: CdcAcmClass<'static, super::UsbDriver>, i2c: super::I2cDriver, adc: adc::Pins<'static>, gpio: gpio::Pins<'static>) -> ! {
    let (tx, mut rx, mut ctrl) = class.split_with_control();
    let mut controller = Controller { tx, i2c, adc, gpio };

    loop {
        rx.wait_connection().await;
        while !rx.dtr() {
            ctrl.control_changed().await;
        }
        info!("Control: Connected");
        let _ = select(pipe_usb_read(&mut rx, &mut ctrl), controller.run()).await;
        bridge::shutdown().await;
        while COMMAND_CHANNEL.try_receive().is_ok() {}
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

async fn pipe_usb_read(rx: &mut Receiver<'static, super::UsbDriver>, ctrl: &mut embassy_usb::class::cdc_acm::ControlChanged<'static>) -> Result<(), ControlTaskError> {
    let mut buf = [0; 4098];
    let mut num_read = 0usize;

    loop {
        if num_read == buf.len() {
            COMMAND_CHANNEL
                .send(Command {
                    id: buf.get(2).copied().unwrap_or(0xff),
                    bus: 0,
                    inner: CommandInner::Error(CommandError::BufferOverflow),
                })
                .await;
            num_read = 0;
        }

        let read_result = match select(rx.read_packet(&mut buf[num_read..]), ctrl.control_changed()).await {
            Either::First(result) => result,
            Either::Second(()) => {
                if !rx.dtr() {
                    return Err(ControlTaskError::Disconnected);
                }
                continue;
            }
        };
        num_read += read_result?;

        loop {
            if num_read < 2 {
                break;
            }

            let frame_len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
            if !(6..=buf.len()).contains(&frame_len) {
                COMMAND_CHANNEL
                    .send(Command {
                        id: buf.get(2).copied().unwrap_or(0xff),
                        bus: 0,
                        inner: CommandInner::Error(CommandError::Invalid),
                    })
                    .await;
                num_read = 0;
                break;
            }
            if num_read < frame_len {
                break;
            }

            let id = buf[2];
            let command = match Command::from_bytes(&buf[2..frame_len]) {
                Ok(command) => command,
                Err(error) => Command { id, bus: 0, inner: CommandInner::Error(error) },
            };
            COMMAND_CHANNEL.send(command).await;

            let remaining = num_read - frame_len;
            if remaining != 0 {
                buf.copy_within(frame_len..num_read, 0);
            }
            num_read = remaining;
        }
    }
}
