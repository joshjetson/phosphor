//! Play a scripted performance through the deck's logic and print the MIDI
//! it sends, one message per line as three hex bytes, with `#` lines naming
//! each step. Raw readings go in exactly as the hardware would hand them
//! over, so piping this into a virtual MIDI port named "Phosphor Deck" tests
//! the firmware's decisions against the real app with no board at all.
//!
//!   cargo run --example scenario

use deck_logic::deck::{Config, Deck};
use deck_logic::table::DIGITAL;
use deck_logic::{Input, CHIPS};

const IDLE: [u8; CHIPS] = [0xFF; CHIPS];

fn with(closed: &[usize]) -> [u8; CHIPS] {
    let mut b = IDLE;
    for &i in closed {
        b[i / 8] &= !(1 << (i % 8));
    }
    b
}

fn button(note: u8) -> usize {
    DIGITAL.iter().position(|i| matches!(*i, Input::Button { note: n, .. } if n == note)).expect("a button with that note")
}

fn encoder_pins(encoder: usize) -> (usize, usize) {
    let a = DIGITAL.iter().position(|i| *i == Input::EncoderA { encoder }).expect("encoder A");
    (a, a + 1)
}

fn print(p: [u8; 4]) {
    println!("{:02x} {:02x} {:02x}", p[1], p[2], p[3]);
}

fn main() {
    let config = Config { settle: 10, flip_encoders: false, pad: deck_logic::pad::DEFAULT };
    let mut deck = Deck::new(&IDLE, config);
    let scan = |deck: &mut Deck, bytes: [u8; CHIPS], scans: usize| {
        for _ in 0..scans {
            deck.digital(&bytes, &mut print);
        }
    };

    println!("# press PLAY, with contact bounce");
    for bytes in [with(&[0]), IDLE, with(&[0]), IDLE] {
        scan(&mut deck, bytes, 1);
    }
    scan(&mut deck, with(&[0]), 20);
    scan(&mut deck, IDLE, 20);

    println!("# TEMPO, then three clicks clockwise on FUNCTION");
    scan(&mut deck, with(&[button(9)]), 20);
    scan(&mut deck, IDLE, 20);
    let (a, b) = encoder_pins(0);
    for _ in 0..3 {
        for bytes in [with(&[a]), with(&[a, b]), with(&[b]), IDLE] {
            scan(&mut deck, bytes, 2);
        }
    }

    println!("# fader 1 from the bottom to the top");
    let mut second = [0u16; 16];
    for raw in (0..=4095).step_by(16) {
        second[0] = raw;
        deck.second_mux(&second, &mut print);
    }

    println!("# a firm hit on pad 9 (bottom-left)");
    let mut pads = [0u16; 16];
    for raw in [500, 1600, 2500, 2300, 2000, 1800, 1700, 1700, 40] {
        pads[8] = raw;
        deck.pads(&pads, &mut print);
    }
}
