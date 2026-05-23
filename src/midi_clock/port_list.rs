pub fn list_midi_input_ports() -> Vec<String> {
    let input = match midir::MidiInput::new("propeller-list-in") {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };
    let ports = input.ports();
    ports.iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-9: list_midi_input_ports() returns Vec<String> and never panics
    #[test]
    fn list_midi_input_ports_returns_vec_no_panic() {
        let ports = list_midi_input_ports();
        // May be empty on CI/headless; must not panic
        for p in &ports {
            assert!(!p.is_empty(), "port name should not be empty string");
        }
        let _ = ports; // confirm Vec<String>
    }
}
