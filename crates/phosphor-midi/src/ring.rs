//! Lock-free MIDI ring buffer for audio↔MIDI thread communication.
//!
//! Uses a fixed-capacity SPSC ring buffer. The MIDI callback thread
//! pushes messages, the audio thread drains them. No allocations,
//! no locks, no contention.

use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer, Split};

use crate::message::MidiMessage;

/// Fixed capacity for the MIDI ring buffer.
/// 4096 events is enough for ~85ms of continuous MIDI at the densest
/// possible rate (one event per sample at 48kHz). In practice, even
/// the most aggressive controller use produces < 100 events/buffer.
const RING_CAPACITY: usize = 4096;

/// Producer side — owned by the MIDI callback thread.
pub struct MidiRingSender {
    producer: ringbuf::HeapProd<MidiMessage>,
}

/// Consumer side — owned by the audio thread.
pub struct MidiRingReceiver {
    consumer: ringbuf::HeapCons<MidiMessage>,
}

/// Create a linked sender/receiver pair.
pub fn midi_ring_buffer() -> (MidiRingSender, MidiRingReceiver) {
    let rb = HeapRb::<MidiMessage>::new(RING_CAPACITY);
    let (producer, consumer) = rb.split();
    (
        MidiRingSender { producer },
        MidiRingReceiver { consumer },
    )
}

impl MidiRingSender {
    /// Push a MIDI message. Returns false if the buffer is full
    /// (message is dropped — better than blocking the MIDI thread).
    pub fn push(&mut self, msg: MidiMessage) -> bool {
        self.producer.try_push(msg).is_ok()
    }

    /// Number of messages currently in the buffer.
    pub fn len(&self) -> usize {
        self.producer.occupied_len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl MidiRingReceiver {
    /// Drain all available messages into the provided vec.
    /// Clears the vec first. This is the audio-thread read path.
    pub fn drain_into(&mut self, out: &mut Vec<MidiMessage>) {
        out.clear();
        while let Some(msg) = self.consumer.try_pop() {
            out.push(msg);
        }
    }

    /// Number of messages available to read.
    pub fn len(&self) -> usize {
        self.consumer.occupied_len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A combined ring buffer for simpler use cases (e.g., testing).
pub struct MidiRingBuffer {
    pub sender: MidiRingSender,
    pub receiver: MidiRingReceiver,
}

impl MidiRingBuffer {
    pub fn new() -> Self {
        let (sender, receiver) = midi_ring_buffer();
        Self { sender, receiver }
    }
}

impl Default for MidiRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MidiMessageType;

    fn make_note_on(note: u8) -> MidiMessage {
        MidiMessage {
            timestamp: Some(0),
            message_type: MidiMessageType::NoteOn {
                channel: 0,
                note,
                velocity: 100,
            },
            raw: [0x90, note, 100],
            len: 3,
        }
    }

    #[test]
    fn push_and_drain() {
        let (mut tx, mut rx) = midi_ring_buffer();
        assert!(tx.push(make_note_on(60)));
        assert!(tx.push(make_note_on(64)));
        assert!(tx.push(make_note_on(67)));

        let mut out = Vec::new();
        rx.drain_into(&mut out);
        assert_eq!(out.len(), 3);

        // Verify ordering preserved
        for (i, note) in [60, 64, 67].iter().enumerate() {
            if let MidiMessageType::NoteOn { note: n, .. } = out[i].message_type {
                assert_eq!(n, *note);
            } else {
                panic!("Expected NoteOn");
            }
        }
    }

    #[test]
    fn drain_empty_buffer() {
        let (_tx, mut rx) = midi_ring_buffer();
        let mut out = Vec::new();
        rx.drain_into(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn drain_clears_output_vec() {
        let (mut tx, mut rx) = midi_ring_buffer();
        let mut out = vec![make_note_on(99)]; // pre-existing data

        tx.push(make_note_on(60));
        rx.drain_into(&mut out);

        assert_eq!(out.len(), 1); // old data cleared, only new message
    }

    #[test]
    fn buffer_full_drops_message() {
        let (mut tx, _rx) = midi_ring_buffer();

        // Fill the buffer
        for i in 0..RING_CAPACITY {
            assert!(tx.push(make_note_on((i % 128) as u8)), "Push {i} failed");
        }

        // Next push should fail (drop the message, don't block)
        assert!(!tx.push(make_note_on(0)), "Should fail when buffer is full");
    }

    #[test]
    fn sender_receiver_are_send() {
        // Compile-time test: these must be Send to cross thread boundaries
        fn assert_send<T: Send>() {}
        assert_send::<MidiRingSender>();
        assert_send::<MidiRingReceiver>();
    }

    #[test]
    fn high_throughput_no_loss() {
        let (mut tx, mut rx) = midi_ring_buffer();
        let mut out = Vec::new();

        // Simulate rapid push/drain cycles like a real audio callback
        for cycle in 0..1000 {
            // Push a few messages (simulating MIDI input)
            for j in 0..5 {
                let note = ((cycle * 5 + j) % 128) as u8;
                assert!(tx.push(make_note_on(note)));
            }

            // Drain (simulating audio callback)
            rx.drain_into(&mut out);
            assert_eq!(out.len(), 5, "Cycle {cycle}: expected 5 messages");
        }
    }
}
