//! MIDI port enumeration and monitoring.
//!
//! Wraps midir to provide a list of available MIDI input/output ports
//! and detect when controllers are connected/disconnected.

use anyhow::Result;
use tracing;

/// Information about an available MIDI port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiPortInfo {
    /// Human-readable port name (e.g., "Akai MPK Mini MIDI 1").
    pub name: String,
    /// Index in the midir port list (changes on rescan).
    pub index: usize,
    /// Whether this is an input or output port.
    pub direction: PortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
}

/// Scan for available MIDI input ports.
pub fn list_input_ports() -> Result<Vec<MidiPortInfo>> {
    let midi_in = midir::MidiInput::new("phosphor-scan")?;
    let ports = midi_in.ports();

    let mut result = Vec::with_capacity(ports.len());
    for (i, port) in ports.iter().enumerate() {
        let name = midi_in
            .port_name(port)
            .unwrap_or_else(|_| format!("MIDI Input {i}"));
        result.push(MidiPortInfo {
            name,
            index: i,
            direction: PortDirection::Input,
        });
    }

    tracing::debug!("Found {} MIDI input ports", result.len());
    Ok(result)
}

/// Scan for available MIDI output ports.
pub fn list_output_ports() -> Result<Vec<MidiPortInfo>> {
    let midi_out = midir::MidiOutput::new("phosphor-scan")?;
    let ports = midi_out.ports();

    let mut result = Vec::with_capacity(ports.len());
    for (i, port) in ports.iter().enumerate() {
        let name = midi_out
            .port_name(port)
            .unwrap_or_else(|_| format!("MIDI Output {i}"));
        result.push(MidiPortInfo {
            name,
            index: i,
            direction: PortDirection::Output,
        });
    }

    tracing::debug!("Found {} MIDI output ports", result.len());
    Ok(result)
}

/// Detect changes between two port scans.
pub fn diff_ports(old: &[MidiPortInfo], new: &[MidiPortInfo]) -> PortDiff {
    let added: Vec<MidiPortInfo> = new
        .iter()
        .filter(|p| !old.iter().any(|o| o.name == p.name))
        .cloned()
        .collect();

    let removed: Vec<MidiPortInfo> = old
        .iter()
        .filter(|p| !new.iter().any(|n| n.name == p.name))
        .cloned()
        .collect();

    PortDiff { added, removed }
}

/// Result of comparing two port scans.
#[derive(Debug, Clone)]
pub struct PortDiff {
    pub added: Vec<MidiPortInfo>,
    pub removed: Vec<MidiPortInfo>,
}

impl PortDiff {
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.removed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_detects_added_port() {
        let old = vec![];
        let new = vec![MidiPortInfo {
            name: "Controller A".into(),
            index: 0,
            direction: PortDirection::Input,
        }];
        let diff = diff_ports(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].name, "Controller A");
        assert!(diff.removed.is_empty());
        assert!(diff.has_changes());
    }

    #[test]
    fn diff_detects_removed_port() {
        let old = vec![MidiPortInfo {
            name: "Controller A".into(),
            index: 0,
            direction: PortDirection::Input,
        }];
        let new = vec![];
        let diff = diff_ports(&old, &new);
        assert!(diff.added.is_empty());
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].name, "Controller A");
    }

    #[test]
    fn diff_no_changes() {
        let ports = vec![MidiPortInfo {
            name: "Controller A".into(),
            index: 0,
            direction: PortDirection::Input,
        }];
        let diff = diff_ports(&ports, &ports);
        assert!(!diff.has_changes());
    }

    #[test]
    fn diff_simultaneous_add_and_remove() {
        let old = vec![MidiPortInfo {
            name: "Old Controller".into(),
            index: 0,
            direction: PortDirection::Input,
        }];
        let new = vec![MidiPortInfo {
            name: "New Controller".into(),
            index: 0,
            direction: PortDirection::Input,
        }];
        let diff = diff_ports(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.added[0].name, "New Controller");
        assert_eq!(diff.removed[0].name, "Old Controller");
    }

    // Note: list_input_ports() and list_output_ports() are integration tests
    // that require actual MIDI hardware or virtual ports. They're tested
    // in CI only on machines with MIDI support, and manually during development.
    // We test the diff logic (pure functions) exhaustively here.
}
