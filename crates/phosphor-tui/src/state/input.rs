//! Input state — NumberBuffer for multi-digit clip jump.

use std::time::Instant;

#[derive(Debug)]
pub struct NumberBuffer {
    digits: String,
    last_input: Option<Instant>,
    timeout_ms: u128,
}

impl NumberBuffer {
    pub fn new() -> Self { Self { digits: String::new(), last_input: None, timeout_ms: 500 } }

    pub fn push_digit(&mut self, ch: char) -> Option<usize> {
        if self.is_timed_out() { self.digits.clear(); }
        self.digits.push(ch);
        self.last_input = Some(Instant::now());
        None
    }

    pub fn check_timeout(&mut self) -> Option<usize> {
        if self.digits.is_empty() { return None; }
        if self.is_timed_out() {
            let num = self.digits.parse::<usize>().ok();
            self.digits.clear();
            self.last_input = None;
            num
        } else { None }
    }

    fn is_timed_out(&self) -> bool {
        self.last_input.map(|t| t.elapsed().as_millis() >= self.timeout_ms).unwrap_or(true)
    }

    pub fn display(&self) -> &str {
        if self.is_timed_out() { "" } else { &self.digits }
    }

    pub fn commit(&mut self) -> Option<usize> {
        if self.digits.is_empty() { return None; }
        let num = self.digits.parse::<usize>().ok();
        self.digits.clear();
        self.last_input = None;
        num
    }
}
