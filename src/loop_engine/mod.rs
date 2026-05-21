use std::sync::{Arc, Mutex, mpsc};

#[derive(Debug, Clone, PartialEq)]
pub enum EngineState {
    Stopped,
    Waiting,
    Running,
}

enum LoopCommand {
    Start,
    Stop,
}

pub struct LoopEngine {
    sender: mpsc::Sender<LoopCommand>,
    state: Arc<Mutex<EngineState>>,
}

impl LoopEngine {
    pub fn new() -> LoopEngine {
        let (sender, receiver) = mpsc::channel::<LoopCommand>();
        let state = Arc::new(Mutex::new(EngineState::Stopped));
        let state_clone = Arc::clone(&state);

        std::thread::spawn(move || {
            loop {
                match receiver.recv() {
                    Ok(LoopCommand::Start) => {
                        let mut s = state_clone.lock().unwrap();
                        if *s == EngineState::Stopped {
                            *s = EngineState::Running;
                        }
                    }
                    Ok(LoopCommand::Stop) => {
                        let mut s = state_clone.lock().unwrap();
                        *s = EngineState::Stopped;
                    }
                    Err(_) => break,
                }
            }
        });

        LoopEngine { sender, state }
    }

    pub fn start(&self) {
        let _ = self.sender.send(LoopCommand::Start);
    }

    pub fn stop(&self) {
        let _ = self.sender.send(LoopCommand::Stop);
    }

    pub fn state(&self) -> EngineState {
        self.state.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_state_is_stopped() {
        let engine = LoopEngine::new();
        assert_eq!(engine.state(), EngineState::Stopped);
    }

    #[test]
    fn start_transitions_to_running() {
        let engine = LoopEngine::new();
        engine.start();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(engine.state(), EngineState::Running);
    }

    #[test]
    fn stop_transitions_to_stopped() {
        let engine = LoopEngine::new();
        engine.start();
        std::thread::sleep(std::time::Duration::from_millis(20));
        engine.stop();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(engine.state(), EngineState::Stopped);
    }
}
