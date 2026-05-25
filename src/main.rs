#![no_std]
#![no_main]
#![feature(trivial_bounds)]

use defmt::unwrap;
use panic_rtt_target as _;

esp_bootloader_esp_idf::esp_app_desc!();

use embassy_executor::Spawner;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use esp_hal::{analog::adc, clock::CpuClock, i2c, timer::systimer::SystemTimer};
use static_cell::StaticCell;

mod control;
mod uart;

pub type AsicUart = esp_hal::peripherals::UART1<'static>;
pub type BridgeControlUart = esp_hal::uart::Uart<'static, esp_hal::Async>;
pub type I2cDriver = i2c::master::I2c<'static, esp_hal::Async>;
pub type UsbDriver = esp_hal::otg_fs::asynch::Driver<'static>;
pub type UsbDevice = embassy_usb::UsbDevice<'static, UsbDriver>;

const VERSION: u16 = 0x0001;

static MANUFACTURER: &str = "OSMU";
static PRODUCT: &str = "BitaxeBonanza";

/// Return a unique serial number for this device by hashing its MAC address
fn serial_number() -> &'static str {
    let mac_address = esp_hal::efuse::Efuse::mac_address();
    static SERIAL_NUMBER_BUF: StaticCell<[u8; 8]> = StaticCell::new();
    let sn = const_murmur3::murmur3_32(&mac_address, 0);
    let buf = SERIAL_NUMBER_BUF.init([0; 8]);
    hex::encode_to_slice(sn.to_le_bytes(), &mut buf[..]).unwrap();
    unsafe { core::str::from_utf8_unchecked(buf) }
}

static USB_EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let p = esp_hal::init(config);

    let timer0 = SystemTimer::new(p.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    let usb = esp_hal::otg_fs::Usb::new(p.USB0, p.GPIO20, p.GPIO19);
    let usb_config = esp_hal::otg_fs::asynch::Config::default();
    let usb_driver = UsbDriver::new(usb, USB_EP_OUT_BUFFER.init([0u8; 1024]), usb_config);

    //esp_alloc::heap_allocator!(size: 72 * 1024);

    let usb_config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.device_release = VERSION;
        config.manufacturer = Some(MANUFACTURER);
        config.product = Some(PRODUCT);
        config.serial_number = Some(serial_number());
        config.max_power = 100;
        config.max_packet_size_0 = 64;
        config.device_class = 0xef;
        config.device_sub_class = 0x02;
        config.device_protocol = 0x01;
        config.composite_with_iads = true;
        config
    };

    let mut builder = {
        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        embassy_usb::Builder::new(usb_driver, usb_config, CONFIG_DESCRIPTOR.init([0; 256]), BOS_DESCRIPTOR.init([0; 256]), &mut [], CONTROL_BUF.init([0; 64]))
    };

    let control_class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    let asic_uart_class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    let asic_uart = {
        let config = esp_hal::uart::Config::default().with_baudrate(115200).with_rx(esp_hal::uart::RxConfig::default().with_fifo_full_threshold(64));
        esp_hal::uart::Uart::new(p.UART1, config).unwrap().with_rx(p.GPIO18).with_tx(p.GPIO17).into_async()
    };

    let bridge_control_uart = {
        let config = esp_hal::uart::Config::default().with_baudrate(115200).with_rx(esp_hal::uart::RxConfig::default().with_fifo_full_threshold(64));
        esp_hal::uart::Uart::new(p.UART0, config).unwrap().with_rx(p.GPIO44).with_tx(p.GPIO43).into_async()
    };

    let i2c = {
        let config = esp_hal::i2c::master::Config::default();
        let sda = p.GPIO47;
        let scl = p.GPIO48;
        i2c::master::I2c::new(p.I2C0, config).unwrap().with_sda(sda).with_scl(scl).into_async()
    };

    let mut adc_config = adc::AdcConfig::default();
    let vdd = adc_config.enable_pin(p.GPIO2, adc::Attenuation::_11dB);
    let adc = adc::Adc::new(p.ADC1, Default::default());
    let adc_pins = control::adc::Pins { adc, vdd: vdd };

    unwrap!(spawner.spawn(usb_task(builder.build())));
    unwrap!(spawner.spawn(control::usb_task(control_class, i2c, bridge_control_uart, adc_pins)));
    unwrap!(spawner.spawn(uart::usb_task(asic_uart_class, asic_uart)));
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice) -> ! {
    usb.run().await
}
