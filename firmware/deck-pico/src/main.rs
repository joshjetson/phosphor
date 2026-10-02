//! The Phosphor Deck's firmware for the Raspberry Pi Pico 2.
//!
//! The hardware around `deck-logic`, and nothing else: every 500 µs it
//! latches and clocks in the twelve shift registers, steps the two analog
//! multiplexers through their sixteen channels, and hands the readings to a
//! [`Deck`], which says what to send. The messages go out as a USB MIDI
//! device named "Phosphor Deck", which is the name Phosphor recognises it by.
//! Whatever Phosphor sends back sets the LEDs.
//!
//! Pins, as on the schematics:
//!   GP2  SPI0 SCK  → every 74HC165's CLK
//!   GP4  SPI0 RX   ← U1's QH
//!   GP5            → every 74HC165's SH/LD
//!   GP6–GP9        → both 74HC4067s' S0–S3
//!   GP16           → the LED chain, through the 74AHCT125
//!   GP26 ADC0      ← U13 (pads),  GP27 ADC1 ← U14 (faders, expression)
#![no_std]
#![no_main]

use deck_logic::deck::{Config, Deck};
use deck_logic::midi::Packet;
use deck_logic::table::LIGHTS;
use deck_logic::CHIPS;
use embassy_executor::Spawner;
use embassy_futures::join::{join, join4};
use embassy_rp::adc::{self, Adc, Channel as AdcChannel};
use embassy_rp::gpio::{Level, Output, Pull};
use embassy_rp::peripherals::{DMA_CH0, PIO0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::spi::{self, Spi};
use embassy_rp::{bind_interrupts, dma, usb};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{block_for, Duration, Ticker};
use embassy_usb::class::midi::MidiClass;
use smart_leds::RGB8;
use {embassy_rp as _, panic_halt as _};

/// Tells the RP2350's boot ROM this is a program to run.
#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: embassy_rp::block::ImageDef = embassy_rp::block::ImageDef::secure_exe();

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

/// How often everything is read.
const SCAN: Duration = Duration::from_micros(500);
/// Scans a switch must hold a new level: 5 ms.
const SETTLE: u8 = 10;
/// Set if the board's encoders count backwards.
const FLIP_ENCODERS: bool = false;
/// How long a multiplexer's output takes to settle after its select lines
/// change, with the ADC's input capacitance to charge.
const MUX_SETTLE: Duration = Duration::from_micros(4);
/// LED frames: every 32 scans, about 60 a second.
const LED_EVERY: u32 = 32;

/// Messages for Phosphor, from the scan loop to USB.
static OUT: Channel<CriticalSectionRawMutex, Packet, 128> = Channel::new();
/// Messages from Phosphor, from USB to the scan loop.
static IN: Channel<CriticalSectionRawMutex, Packet, 32> = Channel::new();
/// The newest LED frame, from the scan loop to the LED chain.
static FRAME: Signal<CriticalSectionRawMutex, [RGB8; LIGHTS]> = Signal::new();

/// The twelve shift registers.
struct Chain<'d> {
    spi: Spi<'d, embassy_rp::peripherals::SPI0, spi::Blocking>,
    load: Output<'d>,
}

impl Chain<'_> {
    fn read(&mut self) -> [u8; CHIPS] {
        // A low pulse on SH/LD latches every input at once.
        self.load.set_low();
        block_for(Duration::from_micros(1));
        self.load.set_high();
        let mut bytes = [0u8; CHIPS];
        // The first bit out of U1 is its H, so with the most significant bit
        // first each byte reads H..A as bit 7..0: bit n is input n.
        let _ = self.spi.blocking_read(&mut bytes);
        bytes
    }
}

/// The two multiplexers and the ADC behind them.
struct Mux<'d> {
    adc: Adc<'d, adc::Blocking>,
    select: [Output<'d>; 4],
    pads: AdcChannel<'d>,
    second: AdcChannel<'d>,
}

impl Mux<'_> {
    fn read(&mut self) -> ([u16; 16], [u16; 16]) {
        let mut pads = [0u16; 16];
        let mut second = [0u16; 16];
        for ch in 0..16 {
            for (bit, pin) in self.select.iter_mut().enumerate() {
                pin.set_level(if ch & (1 << bit) != 0 { Level::High } else { Level::Low });
            }
            block_for(MUX_SETTLE);
            pads[ch] = self.adc.blocking_read(&mut self.pads).unwrap_or(0);
            second[ch] = self.adc.blocking_read(&mut self.second).unwrap_or(0);
        }
        (pads, second)
    }
}

fn send(packet: Packet) {
    // A full queue means USB is not being read; dropping beats stalling
    // the scan.
    let _ = OUT.try_send(packet);
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // ── USB MIDI ──
    let driver = usb::Driver::new(p.USB, Irqs);
    // 0x1209/0x0001 is pid.codes' test ID, for prototypes: a deck made in
    // numbers needs its own.
    let mut config = embassy_usb::Config::new(0x1209, 0x0001);
    config.manufacturer = Some("Phosphor");
    config.product = Some("Phosphor Deck");
    config.serial_number = Some("0001");
    config.max_power = 500;
    config.max_packet_size_0 = 64;
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 0];
    let mut control_buf = [0; 64];
    let mut builder = embassy_usb::Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );
    let class = MidiClass::new(&mut builder, 1, 1, 64);
    let mut usb = builder.build();
    let (mut sender, mut receiver) = class.split();

    // ── Inputs ──
    let mut spi_config = spi::Config::default();
    spi_config.frequency = 1_000_000;
    let spi = Spi::new_blocking_rxonly(p.SPI0, p.PIN_2, p.PIN_4, spi_config);
    let mut chain = Chain { spi, load: Output::new(p.PIN_5, Level::High) };
    let mut mux = Mux {
        adc: Adc::new_blocking(p.ADC, adc::Config::default()),
        select: [
            Output::new(p.PIN_6, Level::Low),
            Output::new(p.PIN_7, Level::Low),
            Output::new(p.PIN_8, Level::Low),
            Output::new(p.PIN_9, Level::Low),
        ],
        pads: AdcChannel::new_pin(p.PIN_26, Pull::None),
        second: AdcChannel::new_pin(p.PIN_27, Pull::None),
    };

    // ── LEDs ──
    let Pio { mut common, sm0, .. } = Pio::new(p.PIO0, Irqs);
    let program = PioWs2812Program::new(&mut common);
    let mut leds: PioWs2812<'_, PIO0, 0, LIGHTS, _> =
        PioWs2812::new(&mut common, sm0, p.DMA_CH0, Irqs, p.PIN_16, &program);

    let first = chain.read();
    let mut deck = Deck::new(
        &first,
        Config { settle: SETTLE, flip_encoders: FLIP_ENCODERS, pad: deck_logic::pad::DEFAULT },
    );

    let scan = async {
        let mut ticker = Ticker::every(SCAN);
        let mut tick: u32 = 0;
        loop {
            let bytes = chain.read();
            deck.digital(&bytes, &mut send);
            let (pads, second) = mux.read();
            deck.pads(&pads, &mut send);
            deck.second_mux(&second, &mut send);
            while let Ok(packet) = IN.try_receive() {
                deck.midi_in(packet);
            }
            tick = tick.wrapping_add(1);
            if tick % LED_EVERY == 0 {
                FRAME.signal(deck.lights().frame().map(|c| RGB8 { r: c.r, g: c.g, b: c.b }));
            }
            ticker.next().await;
        }
    };

    let midi_out = async {
        loop {
            sender.wait_connection().await;
            loop {
                let packet = OUT.receive().await;
                if sender.write_packet(&packet).await.is_err() {
                    break;
                }
            }
        }
    };

    let midi_in = async {
        let mut buf = [0u8; 64];
        loop {
            receiver.wait_connection().await;
            while let Ok(n) = receiver.read_packet(&mut buf).await {
                for chunk in buf[..n].chunks_exact(4) {
                    let _ = IN.try_send([chunk[0], chunk[1], chunk[2], chunk[3]]);
                }
            }
        }
    };

    let lights = async {
        loop {
            let frame = FRAME.wait().await;
            leds.write(&frame).await;
        }
    };

    join(usb.run(), join4(scan, midi_out, midi_in, lights)).await;
}
