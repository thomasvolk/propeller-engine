use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant};

use crate::domain::{NoteEvent, ProjectStore};

use super::midi::MidiOutput;
use super::scheduler::Scheduler;
use super::{EngineState, LoopCommand};

struct ActiveNote {
    channel: u8,
    pitch: u8,
}

#[derive(Clone)]
enum BarEvent {
    NoteOn { channel: u8, pitch: u8, velocity: u8 },
    NoteOff { channel: u8, pitch: u8 },
}

impl BarEvent {
    fn priority(&self) -> u8 {
        match self {
            BarEvent::NoteOff { .. } => 0,
            BarEvent::NoteOn { .. } => 1,
        }
    }
}

enum SleepResult {
    Elapsed,
    Stop,
    Disconnected,
}

// Sleep until deadline, checking for commands every ~2ms.
// This allows Stop to interrupt even a long sleep between note events.
fn sleep_until_with_poll(
    deadline: Instant,
    receiver: &mpsc::Receiver<LoopCommand>,
    scheduler: &Scheduler,
) -> SleepResult {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return SleepResult::Elapsed;
        }
        let remaining = deadline - now;
        if remaining > Duration::from_millis(2) {
            // Sleep 1ms then check for a command
            std::thread::sleep(Duration::from_millis(1));
            match receiver.try_recv() {
                Ok(LoopCommand::Stop) => return SleepResult::Stop,
                Ok(LoopCommand::Start) => {}
                Err(mpsc::TryRecvError::Disconnected) => return SleepResult::Disconnected,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        } else {
            // Hand off to scheduler's precision sleep for the last ≤2ms
            scheduler.sleep_until(deadline);
            return SleepResult::Elapsed;
        }
    }
}

pub fn run_player_loop(
    receiver: mpsc::Receiver<LoopCommand>,
    store: Arc<RwLock<ProjectStore>>,
    mut output: Box<dyn MidiOutput>,
    shared_state: Arc<Mutex<EngineState>>,
) {
    let mut state = EngineState::Stopped;
    let mut active_notes: Vec<ActiveNote> = Vec::new();
    let mut last_instruments: HashMap<u8, u8> = HashMap::new();
    let mut bar_index: usize = 0;
    let mut scheduler = Scheduler::new(120);
    let mut anchor = Instant::now();

    loop {
        match state {
            EngineState::Stopped => {
                match receiver.recv() {
                    Ok(LoopCommand::Start) => {
                        let has_project = store.read().unwrap().active().is_some();
                        if has_project {
                            bar_index = 0;
                            last_instruments.clear();
                            {
                                let store_r = store.read().unwrap();
                                let project = store_r.active().unwrap();
                                scheduler = Scheduler::new(project.header.bpm);
                            }
                            anchor = Instant::now();
                            state = EngineState::Running;
                            *shared_state.lock().unwrap() = EngineState::Running;
                        } else {
                            state = EngineState::Waiting;
                            *shared_state.lock().unwrap() = EngineState::Waiting;
                        }
                    }
                    Ok(LoopCommand::Stop) => {}
                    Err(_) => return,
                }
            }

            EngineState::Waiting => {
                std::thread::sleep(Duration::from_millis(10));
                {
                    let already_active = store.read().unwrap().active().is_some();
                    let promoted = if !already_active {
                        store.write().unwrap().commit_pending()
                    } else {
                        // commit_pending in case of update, but we were already active
                        false
                    };
                    let now_active = store.read().unwrap().active().is_some();
                    if promoted || now_active {
                        bar_index = 0;
                        last_instruments.clear();
                        {
                            let store_r = store.read().unwrap();
                            let project = store_r.active().unwrap();
                            scheduler = Scheduler::new(project.header.bpm);
                        }
                        anchor = Instant::now();
                        state = EngineState::Running;
                        *shared_state.lock().unwrap() = EngineState::Running;
                        continue;
                    }
                }
                match receiver.try_recv() {
                    Ok(LoopCommand::Stop) => {
                        state = EngineState::Stopped;
                        *shared_state.lock().unwrap() = EngineState::Stopped;
                    }
                    Ok(LoopCommand::Start) => {}
                    Err(mpsc::TryRecvError::Disconnected) => return,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }

            EngineState::Running => {
                // Emit program changes for new/changed track instruments
                {
                    let store_r = store.read().unwrap();
                    if let Some(project) = store_r.active() {
                        for track in &project.tracks {
                            let last = last_instruments.get(&track.channel).copied();
                            if last != Some(track.instrument) {
                                output.program_change(track.channel, track.instrument);
                                last_instruments.insert(track.channel, track.instrument);
                            }
                        }
                    }
                }

                // Build bar event list
                let mut events: Vec<(u64, BarEvent)> = Vec::new();
                let bar_ticks: u64;
                {
                    let store_r = store.read().unwrap();
                    let project = match store_r.active() {
                        Some(p) => p,
                        None => {
                            // No active project while Running — idle briefly
                            drop(store_r);
                            match receiver.try_recv() {
                                Ok(LoopCommand::Stop) => {
                                    flush_active_notes(&mut active_notes, &mut output);
                                    state = EngineState::Stopped;
                                    *shared_state.lock().unwrap() = EngineState::Stopped;
                                }
                                Ok(LoopCommand::Start) => {}
                                Err(mpsc::TryRecvError::Disconnected) => return,
                                Err(mpsc::TryRecvError::Empty) => {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                            }
                            continue;
                        }
                    };

                    let cycle_len = project.cycle_length();
                    if cycle_len == 0 {
                        drop(store_r);
                        match receiver.try_recv() {
                            Ok(LoopCommand::Stop) => {
                                flush_active_notes(&mut active_notes, &mut output);
                                state = EngineState::Stopped;
                                *shared_state.lock().unwrap() = EngineState::Stopped;
                            }
                            Ok(LoopCommand::Start) => {}
                            Err(mpsc::TryRecvError::Disconnected) => return,
                            Err(mpsc::TryRecvError::Empty) => {
                                std::thread::sleep(Duration::from_millis(10));
                            }
                        }
                        continue;
                    }

                    bar_ticks = project.header.time_signature.bar_ticks() as u64;

                    for track in &project.tracks {
                        let bar = track.bar_at(bar_index);
                        let mut tick: u64 = 0;
                        for note in &bar.notes {
                            match &note.event {
                                NoteEvent::Note { pitch, velocity } => {
                                    events.push((tick, BarEvent::NoteOn {
                                        channel: track.channel,
                                        pitch: *pitch,
                                        velocity: *velocity,
                                    }));
                                    events.push((tick + note.duration_ticks as u64, BarEvent::NoteOff {
                                        channel: track.channel,
                                        pitch: *pitch,
                                    }));
                                }
                                NoteEvent::Rest => {}
                            }
                            tick += note.duration_ticks as u64;
                        }
                    }
                }

                // Sort: by tick, then NoteOff (0) before NoteOn (1)
                events.sort_by_key(|(tick, ev)| (*tick, ev.priority()));

                // Walk the event list
                let mut stopped = false;
                for (tick, event) in &events {
                    let deadline = scheduler.deadline_for_tick(anchor, *tick);

                    match sleep_until_with_poll(deadline, &receiver, &scheduler) {
                        SleepResult::Elapsed => {}
                        SleepResult::Stop => {
                            flush_active_notes(&mut active_notes, &mut output);
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        SleepResult::Disconnected => return,
                    }

                    // Emit the event
                    match event {
                        BarEvent::NoteOn { channel, pitch, velocity } => {
                            output.note_on(*channel, *pitch, *velocity);
                            active_notes.push(ActiveNote { channel: *channel, pitch: *pitch });
                        }
                        BarEvent::NoteOff { channel, pitch } => {
                            output.note_off(*channel, *pitch);
                            active_notes.retain(|n| !(n.channel == *channel && n.pitch == *pitch));
                        }
                    }

                    // Check for stop/disconnect after event emission
                    match receiver.try_recv() {
                        Ok(LoopCommand::Stop) => {
                            flush_active_notes(&mut active_notes, &mut output);
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        Ok(LoopCommand::Start) => {}
                        Err(mpsc::TryRecvError::Disconnected) => return,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                }

                if stopped {
                    continue;
                }

                // Advance anchor to the end of this bar
                anchor = scheduler.deadline_for_tick(anchor, bar_ticks);

                // Bar boundary: commit pending, check BPM and instrument changes
                {
                    store.write().unwrap().commit_pending();
                    let store_r = store.read().unwrap();
                    if let Some(project) = store_r.active() {
                        let new_bpm = project.header.bpm;
                        if new_bpm != scheduler.bpm() {
                            scheduler.update_bpm(new_bpm);
                            anchor = Instant::now();
                        }
                    }
                }

                // Advance bar index
                let cycle_len = {
                    let store_r = store.read().unwrap();
                    store_r.active().map(|p| p.cycle_length()).unwrap_or(1)
                };
                if cycle_len > 0 {
                    bar_index = (bar_index + 1) % cycle_len;
                }
            }
        }
    }
}

fn flush_active_notes(active_notes: &mut Vec<ActiveNote>, output: &mut Box<dyn MidiOutput>) {
    for note in active_notes.drain(..) {
        output.note_off(note.channel, note.pitch);
    }
}
