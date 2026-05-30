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

// T-4 (EP-5): remaining unprocessed events at pause time, bar context for resume
struct PauseContext {
    remaining_events: Vec<(u64, BarEvent)>,
    bar_index: usize,
    bar_ticks: u64,
}

#[derive(Clone)]
enum BarEvent {
    NoteOn { channel: u8, pitch: u8, velocity: u8 },
    NoteOff { channel: u8, pitch: u8 },
    ClockPulse,
}

impl BarEvent {
    fn priority(&self) -> u8 {
        match self {
            BarEvent::NoteOff { .. } => 0,
            BarEvent::NoteOn { .. } => 1,
            BarEvent::ClockPulse => 2,
        }
    }
}

enum SleepResult {
    Elapsed,
    Stop,
    ClockPause,
    ClockStop,
    SyncStop,
    SyncStart,
    Disconnected,
}

// Sleep until deadline, checking for commands every ~2ms.
fn sleep_until_with_poll(
    deadline: Instant,
    receiver: &mpsc::Receiver<LoopCommand>,
    scheduler: &Scheduler,
    pending_sync_bpm: &mut Option<u32>,
) -> SleepResult {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return SleepResult::Elapsed;
        }
        let remaining = deadline - now;
        if remaining > Duration::from_millis(2) {
            std::thread::sleep(Duration::from_millis(1));
            match receiver.try_recv() {
                Ok(LoopCommand::Stop) => return SleepResult::Stop,
                Ok(LoopCommand::ClockStop) => return SleepResult::ClockStop,
                Ok(LoopCommand::ClockPause) => return SleepResult::ClockPause,
                Ok(LoopCommand::SyncStop) => return SleepResult::SyncStop,
                Ok(LoopCommand::SyncStart) => return SleepResult::SyncStart,
                Ok(LoopCommand::SyncBpmUpdate(bpm)) => *pending_sync_bpm = Some(bpm),
                Ok(LoopCommand::Start
                    | LoopCommand::ClockStart
                    | LoopCommand::ClockResume
                    | LoopCommand::SyncContinue) => {}
                Err(mpsc::TryRecvError::Disconnected) => return SleepResult::Disconnected,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        } else {
            scheduler.sleep_until(deadline);
            return SleepResult::Elapsed;
        }
    }
}

// Microseconds to wait before the first event on startup so that program changes and
// Clock Start (0xFA) reach the MIDI device before any NoteOn.
const START_LATENCY_MICROS: u64 = 20_000;

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
    // EP-5 clock mode state
    let mut is_clock_mode = false;
    let mut pause_context: Option<PauseContext> = None;
    let mut last_bar_ticks: u64 = 480;
    // EP-6 sync mode state
    let mut pending_sync_bpm: Option<u32> = None;

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
                            anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                            is_clock_mode = false;
                            state = EngineState::Running;
                            *shared_state.lock().unwrap() = EngineState::Running;
                        } else {
                            state = EngineState::Waiting;
                            *shared_state.lock().unwrap() = EngineState::Waiting;
                        }
                    }
                    // T-8 (EP-5): ClockStart — IPC layer guarantees active project exists
                    Ok(LoopCommand::ClockStart) => {
                        bar_index = 0;
                        last_instruments.clear();
                        {
                            let store_r = store.read().unwrap();
                            let project = store_r.active().unwrap();
                            scheduler = Scheduler::new(project.header.bpm);
                            last_bar_ticks = project.header.time_signature.bar_ticks() as u64;
                        }
                        anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                        is_clock_mode = true;
                        output.clock_start();
                        state = EngineState::Running;
                        *shared_state.lock().unwrap() = EngineState::Running;
                    }
                    // T-13 (EP-6): SyncStart — start from bar 0 if project present, else Waiting
                    Ok(LoopCommand::SyncStart) => {
                        let has_project = store.read().unwrap().active().is_some();
                        if has_project {
                            bar_index = 0;
                            last_instruments.clear();
                            {
                                let store_r = store.read().unwrap();
                                let project = store_r.active().unwrap();
                                scheduler = Scheduler::new(project.header.bpm);
                            }
                            anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                            state = EngineState::Running;
                            *shared_state.lock().unwrap() = EngineState::Running;
                        } else {
                            state = EngineState::Waiting;
                            *shared_state.lock().unwrap() = EngineState::Waiting;
                        }
                    }
                    // T-17 (EP-6): SyncContinue — continue from current bar_index if project present
                    Ok(LoopCommand::SyncContinue) => {
                        let has_project = store.read().unwrap().active().is_some();
                        if has_project {
                            last_instruments.clear();
                            {
                                let store_r = store.read().unwrap();
                                let project = store_r.active().unwrap();
                                scheduler = Scheduler::new(project.header.bpm);
                            }
                            anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                            state = EngineState::Running;
                            *shared_state.lock().unwrap() = EngineState::Running;
                        } else {
                            state = EngineState::Waiting;
                            *shared_state.lock().unwrap() = EngineState::Waiting;
                        }
                    }
                    Ok(LoopCommand::SyncBpmUpdate(bpm)) => {
                        pending_sync_bpm = Some(bpm);
                    }
                    Ok(LoopCommand::Stop
                        | LoopCommand::ClockStop
                        | LoopCommand::SyncStop
                        | LoopCommand::ClockPause
                        | LoopCommand::ClockResume) => {}
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
                        anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                        state = EngineState::Running;
                        *shared_state.lock().unwrap() = EngineState::Running;
                        continue;
                    }
                }
                match receiver.try_recv() {
                    Ok(LoopCommand::Stop | LoopCommand::SyncStop) => {
                        state = EngineState::Stopped;
                        *shared_state.lock().unwrap() = EngineState::Stopped;
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Disconnected) => return,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }

            EngineState::Running => {
                // Check if we are resuming from a pause (ClockResume path)
                let resuming = pause_context.take();

                let bar_data: Option<(Vec<(u64, BarEvent)>, u64)>;

                if let Some(ctx) = resuming {
                    // Resume from pause: restore anchor so next event fires immediately
                    let tick_of_next = ctx.remaining_events.first().map(|(t, _)| *t).unwrap_or(0);
                    anchor = Instant::now()
                        - Duration::from_micros(tick_of_next * scheduler.micros_per_tick());
                    bar_index = ctx.bar_index;
                    bar_data = Some((ctx.remaining_events, ctx.bar_ticks));
                } else {
                    // Normal bar: emit program changes then build event list
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

                    let mut events: Vec<(u64, BarEvent)> = Vec::new();
                    let mut maybe_bar_ticks: Option<u64> = None;

                    {
                        let store_r = store.read().unwrap();
                        match store_r.active() {
                            Some(project) => {
                                let cycle_len = project.cycle_length();
                                if cycle_len == 0 {
                                    drop(store_r);
                                    match receiver.try_recv() {
                                        Ok(LoopCommand::Stop) => {
                                            flush_active_notes(&mut active_notes, &mut output);
                                            state = EngineState::Stopped;
                                            *shared_state.lock().unwrap() = EngineState::Stopped;
                                        }
                                        Ok(LoopCommand::ClockStop) => {
                                            flush_active_notes(&mut active_notes, &mut output);
                                            output.clock_stop();
                                            bar_index = 0;
                                            state = EngineState::Stopped;
                                            *shared_state.lock().unwrap() = EngineState::Stopped;
                                        }
                                        Ok(LoopCommand::SyncStop) => {
                                            flush_active_notes(&mut active_notes, &mut output);
                                            bar_index = 0;
                                            state = EngineState::Stopped;
                                            *shared_state.lock().unwrap() = EngineState::Stopped;
                                        }
                                        Ok(_) => {}
                                        Err(mpsc::TryRecvError::Disconnected) => return,
                                        Err(mpsc::TryRecvError::Empty) => {
                                            std::thread::sleep(Duration::from_millis(10));
                                        }
                                    }
                                    bar_data = None;
                                } else {
                                    let bt = project.header.time_signature.bar_ticks() as u64;
                                    last_bar_ticks = bt;
                                    maybe_bar_ticks = Some(bt);

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
                                                    events.push((
                                                        tick + note.duration_ticks as u64,
                                                        BarEvent::NoteOff {
                                                            channel: track.channel,
                                                            pitch: *pitch,
                                                        },
                                                    ));
                                                }
                                                NoteEvent::Rest => {}
                                            }
                                            tick += note.duration_ticks as u64;
                                        }
                                    }

                                    // T-10 (EP-5): insert ClockPulse every 20 ticks in clock mode
                                    if is_clock_mode {
                                        let mut cp = 0u64;
                                        while cp < bt {
                                            events.push((cp, BarEvent::ClockPulse));
                                            cp += 20;
                                        }
                                    }

                                    events.sort_by_key(|(tick, ev)| (*tick, ev.priority()));
                                    bar_data = Some((events, bt));
                                }
                            }
                            None => {
                                // T-28 (EP-5): no project in clock mode → ClockPulse-only bar
                                if is_clock_mode {
                                    let bt = last_bar_ticks;
                                    let mut cp = 0u64;
                                    while cp < bt {
                                        events.push((cp, BarEvent::ClockPulse));
                                        cp += 20;
                                    }
                                    bar_data = Some((events, bt));
                                } else {
                                    // Non-clock, no project: idle
                                    drop(store_r);
                                    match receiver.try_recv() {
                                        Ok(LoopCommand::Stop) => {
                                            flush_active_notes(&mut active_notes, &mut output);
                                            state = EngineState::Stopped;
                                            *shared_state.lock().unwrap() = EngineState::Stopped;
                                        }
                                        Ok(_) => {}
                                        Err(mpsc::TryRecvError::Disconnected) => return,
                                        Err(mpsc::TryRecvError::Empty) => {
                                            std::thread::sleep(Duration::from_millis(10));
                                        }
                                    }
                                    bar_data = None;
                                }
                            }
                        }
                    }

                    if maybe_bar_ticks.is_none() && !matches!(bar_data, Some(_)) {
                        continue;
                    }
                };

                let (events, bar_ticks) = match bar_data {
                    Some(bd) => bd,
                    None => continue,
                };

                // Walk the event list
                let mut stopped = false;
                let mut paused = false;
                let n = events.len();
                let mut i = 0;

                while i < n {
                    let (tick, event) = &events[i];
                    let deadline = scheduler.deadline_for_tick(anchor, *tick);

                    match sleep_until_with_poll(deadline, &receiver, &scheduler, &mut pending_sync_bpm) {
                        SleepResult::Elapsed => {}
                        SleepResult::Stop => {
                            flush_active_notes(&mut active_notes, &mut output);
                            if is_clock_mode {
                                output.clock_stop();
                                is_clock_mode = false;
                            }
                            bar_index = 0;
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        SleepResult::ClockStop => {
                            flush_active_notes(&mut active_notes, &mut output);
                            output.clock_stop();
                            bar_index = 0;
                            is_clock_mode = false;
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        // T-19 (EP-6): SyncStop — flush notes, go Stopped (no 0xFC output)
                        SleepResult::SyncStop => {
                            flush_active_notes(&mut active_notes, &mut output);
                            bar_index = 0;
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        // T-15 (EP-6): SyncStart while Running — flush, reset to bar 0, restart
                        SleepResult::SyncStart => {
                            flush_active_notes(&mut active_notes, &mut output);
                            bar_index = 0;
                            last_instruments.clear();
                            anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                            // State stays Running; break current bar to restart from bar 0
                            stopped = true;
                            break;
                        }
                        // T-16 (EP-5): ClockPause — flush notes, capture remaining events
                        SleepResult::ClockPause => {
                            flush_active_notes(&mut active_notes, &mut output);
                            pause_context = Some(PauseContext {
                                remaining_events: events[i..].to_vec(),
                                bar_index,
                                bar_ticks,
                            });
                            state = EngineState::Paused;
                            *shared_state.lock().unwrap() = EngineState::Paused;
                            paused = true;
                            break;
                        }
                        SleepResult::Disconnected => return,
                    }

                    // Emit the event
                    match event {
                        BarEvent::ClockPulse => output.clock_tick(),
                        BarEvent::NoteOn { channel, pitch, velocity } => {
                            output.note_on(*channel, *pitch, *velocity);
                            active_notes.push(ActiveNote { channel: *channel, pitch: *pitch });
                        }
                        BarEvent::NoteOff { channel, pitch } => {
                            output.note_off(*channel, *pitch);
                            active_notes.retain(|n| !(n.channel == *channel && n.pitch == *pitch));
                        }
                    }

                    // Post-event command check
                    match receiver.try_recv() {
                        Ok(LoopCommand::Stop) => {
                            flush_active_notes(&mut active_notes, &mut output);
                            if is_clock_mode {
                                output.clock_stop();
                                is_clock_mode = false;
                            }
                            bar_index = 0;
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        Ok(LoopCommand::ClockStop) => {
                            flush_active_notes(&mut active_notes, &mut output);
                            output.clock_stop();
                            bar_index = 0;
                            is_clock_mode = false;
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        Ok(LoopCommand::SyncStop) => {
                            flush_active_notes(&mut active_notes, &mut output);
                            bar_index = 0;
                            state = EngineState::Stopped;
                            *shared_state.lock().unwrap() = EngineState::Stopped;
                            stopped = true;
                            break;
                        }
                        Ok(LoopCommand::SyncStart) => {
                            flush_active_notes(&mut active_notes, &mut output);
                            bar_index = 0;
                            last_instruments.clear();
                            anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                            stopped = true;
                            break;
                        }
                        Ok(LoopCommand::SyncBpmUpdate(bpm)) => {
                            pending_sync_bpm = Some(bpm);
                        }
                        Ok(LoopCommand::ClockPause) => {
                            flush_active_notes(&mut active_notes, &mut output);
                            pause_context = Some(PauseContext {
                                remaining_events: events[i + 1..].to_vec(),
                                bar_index,
                                bar_ticks,
                            });
                            state = EngineState::Paused;
                            *shared_state.lock().unwrap() = EngineState::Paused;
                            paused = true;
                            break;
                        }
                        Ok(LoopCommand::Start
                            | LoopCommand::ClockStart
                            | LoopCommand::ClockResume
                            | LoopCommand::SyncContinue) => {}
                        Err(mpsc::TryRecvError::Disconnected) => return,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }

                    i += 1;
                }

                // SyncStart while Running: state is still Running but stopped=true (restart bar 0)
                // We need to distinguish "true stop" from "SyncStart restart"
                if stopped && state == EngineState::Running {
                    // SyncStart triggered — restart from bar 0 (anchor already updated above)
                    continue;
                }
                if stopped || paused {
                    continue;
                }

                // Advance anchor to end of this bar
                anchor = scheduler.deadline_for_tick(anchor, bar_ticks);

                // Bar boundary: commit pending, apply pending_sync_bpm or project BPM
                {
                    store.write().unwrap().commit_pending();
                    if let Some(sync_bpm) = pending_sync_bpm.take() {
                        if sync_bpm != scheduler.bpm() {
                            scheduler.update_bpm(sync_bpm);
                            anchor = Instant::now();
                        }
                    } else {
                        let store_r = store.read().unwrap();
                        if let Some(project) = store_r.active() {
                            let new_bpm = project.header.bpm;
                            if new_bpm != scheduler.bpm() {
                                scheduler.update_bpm(new_bpm);
                                anchor = Instant::now();
                            }
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

            // T-24 (EP-5): Paused state — block waiting for ClockResume or ClockStop
            EngineState::Paused => {
                match receiver.recv() {
                    Ok(LoopCommand::ClockResume) => {
                        output.clock_continue();
                        // pause_context holds the remaining events; Running state will consume it
                        state = EngineState::Running;
                        *shared_state.lock().unwrap() = EngineState::Running;
                    }
                    Ok(LoopCommand::ClockStop | LoopCommand::Stop) => {
                        pause_context = None;
                        output.clock_stop();
                        bar_index = 0;
                        is_clock_mode = false;
                        state = EngineState::Stopped;
                        *shared_state.lock().unwrap() = EngineState::Stopped;
                    }
                    Ok(_) => {}
                    Err(_) => return,
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
