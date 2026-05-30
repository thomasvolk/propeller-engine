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

enum BarOutcome {
    Complete,
    Stopped,
    Paused,
    SyncRestart,
    Disconnected,
}

enum BuildResult {
    Events(Vec<(u64, BarEvent)>, u64),
    NoData,
    Disconnected,
}

// Microseconds to wait before the first event on startup so that program changes and
// Clock Start (0xFA) reach the MIDI device before any NoteOn.
const START_LATENCY_MICROS: u64 = 20_000;

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

struct PlayerLoop {
    receiver: mpsc::Receiver<LoopCommand>,
    store: Arc<RwLock<ProjectStore>>,
    output: Box<dyn MidiOutput>,
    shared_state: Arc<Mutex<EngineState>>,
    state: EngineState,
    active_notes: Vec<ActiveNote>,
    last_instruments: HashMap<u8, u8>,
    bar_index: usize,
    scheduler: Scheduler,
    anchor: Instant,
    is_clock_mode: bool,
    pause_context: Option<PauseContext>,
    last_bar_ticks: u64,
    pending_sync_bpm: Option<u32>,
}

impl PlayerLoop {
    fn new(
        receiver: mpsc::Receiver<LoopCommand>,
        store: Arc<RwLock<ProjectStore>>,
        output: Box<dyn MidiOutput>,
        shared_state: Arc<Mutex<EngineState>>,
    ) -> Self {
        PlayerLoop {
            receiver,
            store,
            output,
            shared_state,
            state: EngineState::Stopped,
            active_notes: Vec::new(),
            last_instruments: HashMap::new(),
            bar_index: 0,
            scheduler: Scheduler::new(120),
            anchor: Instant::now(),
            is_clock_mode: false,
            pause_context: None,
            last_bar_ticks: 480,
            pending_sync_bpm: None,
        }
    }

    fn set_state(&mut self, s: EngineState) {
        self.state = s;
        *self.shared_state.lock().unwrap() = s;
    }

    fn flush_notes(&mut self) {
        for note in self.active_notes.drain(..) {
            if let Err(e) = self.output.note_off(note.channel, note.pitch) {
                eprintln!("MIDI NoteOff failed during flush: {e}");
            }
        }
    }

    fn do_stop(&mut self) {
        self.flush_notes();
        if self.is_clock_mode {
            if let Err(e) = self.output.clock_stop() {
                eprintln!("MIDI clock_stop failed: {e}");
            }
            self.is_clock_mode = false;
        }
        self.bar_index = 0;
        self.set_state(EngineState::Stopped);
    }

    fn do_clock_stop(&mut self) {
        self.flush_notes();
        if let Err(e) = self.output.clock_stop() {
            eprintln!("MIDI clock_stop failed: {e}");
        }
        self.bar_index = 0;
        self.is_clock_mode = false;
        self.set_state(EngineState::Stopped);
    }

    fn do_sync_stop(&mut self) {
        self.flush_notes();
        self.bar_index = 0;
        self.set_state(EngineState::Stopped);
    }

    // T-15 (EP-6): SyncStart while Running — flush, reset to bar 0, restart; state stays Running
    fn do_sync_restart(&mut self) {
        self.flush_notes();
        self.bar_index = 0;
        self.last_instruments.clear();
        self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
    }

    fn do_pause(&mut self, remaining: Vec<(u64, BarEvent)>, bar_ticks: u64) {
        self.flush_notes();
        self.pause_context = Some(PauseContext {
            remaining_events: remaining,
            bar_index: self.bar_index,
            bar_ticks,
        });
        self.set_state(EngineState::Paused);
    }

    fn handle_sleep_result(
        &mut self,
        result: SleepResult,
        remaining: &[(u64, BarEvent)],
        bar_ticks: u64,
    ) -> Option<BarOutcome> {
        match result {
            SleepResult::Elapsed => None,
            SleepResult::Stop => { self.do_stop(); Some(BarOutcome::Stopped) }
            SleepResult::ClockStop => { self.do_clock_stop(); Some(BarOutcome::Stopped) }
            SleepResult::SyncStop => { self.do_sync_stop(); Some(BarOutcome::Stopped) }
            SleepResult::SyncStart => { self.do_sync_restart(); Some(BarOutcome::SyncRestart) }
            SleepResult::ClockPause => {
                self.do_pause(remaining.to_vec(), bar_ticks);
                Some(BarOutcome::Paused)
            }
            SleepResult::Disconnected => Some(BarOutcome::Disconnected),
        }
    }

    // Returns Some(BarOutcome) if the command ends the current bar, None to continue.
    fn handle_command_in_bar(
        &mut self,
        cmd: LoopCommand,
        remaining: &[(u64, BarEvent)],
        bar_ticks: u64,
    ) -> Option<BarOutcome> {
        match cmd {
            LoopCommand::Stop => { self.do_stop(); Some(BarOutcome::Stopped) }
            LoopCommand::ClockStop => { self.do_clock_stop(); Some(BarOutcome::Stopped) }
            LoopCommand::SyncStop => { self.do_sync_stop(); Some(BarOutcome::Stopped) }
            LoopCommand::SyncStart => { self.do_sync_restart(); Some(BarOutcome::SyncRestart) }
            LoopCommand::ClockPause => {
                self.do_pause(remaining.to_vec(), bar_ticks);
                Some(BarOutcome::Paused)
            }
            LoopCommand::SyncBpmUpdate(bpm) => { self.pending_sync_bpm = Some(bpm); None }
            LoopCommand::Start
            | LoopCommand::ClockStart
            | LoopCommand::ClockResume
            | LoopCommand::SyncContinue => None,
        }
    }

    fn init_running_from_project(&mut self) {
        let store_r = self.store.read().unwrap();
        let project = store_r.active().unwrap();
        self.scheduler = Scheduler::new(project.header.bpm);
        self.last_bar_ticks = project.header.time_signature.bar_ticks() as u64;
    }

    fn emit_event(&mut self, event: &BarEvent) {
        match event {
            BarEvent::ClockPulse => {
                if let Err(e) = self.output.clock_tick() {
                    eprintln!("MIDI clock_tick failed: {e}");
                }
            }
            BarEvent::NoteOn { channel, pitch, velocity } => {
                if let Err(e) = self.output.note_on(*channel, *pitch, *velocity) {
                    eprintln!("MIDI note_on failed: {e}");
                }
                self.active_notes.push(ActiveNote { channel: *channel, pitch: *pitch });
            }
            BarEvent::NoteOff { channel, pitch } => {
                if let Err(e) = self.output.note_off(*channel, *pitch) {
                    eprintln!("MIDI note_off failed: {e}");
                }
                self.active_notes.retain(|n| !(n.channel == *channel && n.pitch == *pitch));
            }
        }
    }

    fn play_events(&mut self, events: Vec<(u64, BarEvent)>, bar_ticks: u64) -> BarOutcome {
        let n = events.len();
        let mut i = 0;
        while i < n {
            let tick = events[i].0;
            let deadline = self.scheduler.deadline_for_tick(self.anchor, tick);

            let sleep_result = sleep_until_with_poll(
                deadline,
                &self.receiver,
                &self.scheduler,
                &mut self.pending_sync_bpm,
            );
            if let Some(outcome) = self.handle_sleep_result(sleep_result, &events[i..], bar_ticks) {
                return outcome;
            }

            let event = &events[i].1;
            self.emit_event(event);

            match self.receiver.try_recv() {
                Ok(cmd) => {
                    if let Some(outcome) = self.handle_command_in_bar(cmd, &events[i + 1..], bar_ticks) {
                        return outcome;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => return BarOutcome::Disconnected,
                Err(mpsc::TryRecvError::Empty) => {}
            }

            i += 1;
        }
        BarOutcome::Complete
    }

    // Builds the event list for the current bar from the store. Returns NoData when the
    // bar should be skipped (state may have changed) and Disconnected when the channel dropped.
    fn build_normal_bar(&mut self) -> BuildResult {
        {
            let store_r = self.store.read().unwrap();
            if let Some(project) = store_r.active() {
                for track in &project.tracks {
                    let last = self.last_instruments.get(&track.channel).copied();
                    if last != Some(track.instrument) {
                        if let Err(e) = self.output.program_change(track.channel, track.instrument) {
                            eprintln!("MIDI program_change failed: {e}");
                        }
                        self.last_instruments.insert(track.channel, track.instrument);
                    }
                }
            }
        }

        let store_r = self.store.read().unwrap();
        match store_r.active() {
            Some(project) => {
                let cycle_len = project.cycle_length();
                if cycle_len == 0 {
                    drop(store_r);
                    match self.receiver.try_recv() {
                        Ok(cmd) => {
                            if let Some(outcome) = self.handle_command_in_bar(cmd, &[], 0) {
                                if matches!(outcome, BarOutcome::Disconnected) {
                                    return BuildResult::Disconnected;
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Disconnected) => return BuildResult::Disconnected,
                        Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
                    }
                    return BuildResult::NoData;
                }

                let bt = project.header.time_signature.bar_ticks() as u64;
                self.last_bar_ticks = bt;
                let mut events: Vec<(u64, BarEvent)> = Vec::new();

                for track in &project.tracks {
                    let bar = track.bar_at(self.bar_index);
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
                if self.is_clock_mode {
                    let mut cp = 0u64;
                    while cp < bt {
                        events.push((cp, BarEvent::ClockPulse));
                        cp += 20;
                    }
                }

                events.sort_unstable_by_key(|(tick, ev)| (*tick, ev.priority()));
                BuildResult::Events(events, bt)
            }
            None => {
                // T-28 (EP-5): no project in clock mode → ClockPulse-only bar
                if self.is_clock_mode {
                    let bt = self.last_bar_ticks;
                    let mut events = Vec::new();
                    let mut cp = 0u64;
                    while cp < bt {
                        events.push((cp, BarEvent::ClockPulse));
                        cp += 20;
                    }
                    BuildResult::Events(events, bt)
                } else {
                    drop(store_r);
                    match self.receiver.try_recv() {
                        Ok(cmd) => { self.handle_command_in_bar(cmd, &[], 0); }
                        Err(mpsc::TryRecvError::Disconnected) => return BuildResult::Disconnected,
                        Err(mpsc::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(10)),
                    }
                    BuildResult::NoData
                }
            }
        }
    }

    fn advance_bar(&mut self, bar_ticks: u64) {
        self.anchor = self.scheduler.deadline_for_tick(self.anchor, bar_ticks);

        self.store.write().unwrap().commit_pending();

        if let Some(sync_bpm) = self.pending_sync_bpm.take() {
            if sync_bpm != self.scheduler.bpm() {
                self.scheduler.update_bpm(sync_bpm);
                self.anchor = Instant::now();
            }
        } else {
            let store_r = self.store.read().unwrap();
            if let Some(project) = store_r.active() {
                let new_bpm = project.header.bpm;
                if new_bpm != self.scheduler.bpm() {
                    self.scheduler.update_bpm(new_bpm);
                    self.anchor = Instant::now();
                }
            }
        }

        let cycle_len = {
            let store_r = self.store.read().unwrap();
            store_r.active().map(|p| p.cycle_length()).unwrap_or(1)
        };
        if cycle_len > 0 {
            self.bar_index = (self.bar_index + 1) % cycle_len;
        }
    }

    // Returns false when the player thread should exit.
    fn handle_stopped(&mut self) -> bool {
        match self.receiver.recv() {
            Ok(LoopCommand::Start) => {
                let has_project = self.store.read().unwrap().active().is_some();
                if has_project {
                    self.bar_index = 0;
                    self.last_instruments.clear();
                    self.init_running_from_project();
                    self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                    self.is_clock_mode = false;
                    self.set_state(EngineState::Running);
                } else {
                    self.set_state(EngineState::Waiting);
                }
            }
            // T-8 (EP-5): ClockStart — IPC layer guarantees active project exists
            Ok(LoopCommand::ClockStart) => {
                self.bar_index = 0;
                self.last_instruments.clear();
                self.init_running_from_project();
                self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                self.is_clock_mode = true;
                if let Err(e) = self.output.clock_start() {
                    eprintln!("MIDI clock_start failed: {e}");
                }
                self.set_state(EngineState::Running);
            }
            // T-13 (EP-6): SyncStart — start from bar 0 if project present, else Waiting
            Ok(LoopCommand::SyncStart) => {
                let has_project = self.store.read().unwrap().active().is_some();
                if has_project {
                    self.bar_index = 0;
                    self.last_instruments.clear();
                    self.init_running_from_project();
                    self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                    self.set_state(EngineState::Running);
                } else {
                    self.set_state(EngineState::Waiting);
                }
            }
            // T-17 (EP-6): SyncContinue — continue from current bar_index if project present
            Ok(LoopCommand::SyncContinue) => {
                let has_project = self.store.read().unwrap().active().is_some();
                if has_project {
                    self.last_instruments.clear();
                    self.init_running_from_project();
                    self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                    self.set_state(EngineState::Running);
                } else {
                    self.set_state(EngineState::Waiting);
                }
            }
            Ok(LoopCommand::SyncBpmUpdate(bpm)) => {
                self.pending_sync_bpm = Some(bpm);
            }
            Ok(LoopCommand::Stop
                | LoopCommand::ClockStop
                | LoopCommand::SyncStop
                | LoopCommand::ClockPause
                | LoopCommand::ClockResume) => {}
            Err(_) => return false,
        }
        true
    }

    fn handle_waiting(&mut self) -> bool {
        std::thread::sleep(Duration::from_millis(10));

        let already_active = self.store.read().unwrap().active().is_some();
        let promoted = if !already_active {
            self.store.write().unwrap().commit_pending()
        } else {
            false
        };
        let now_active = self.store.read().unwrap().active().is_some();

        if promoted || now_active {
            self.bar_index = 0;
            self.last_instruments.clear();
            self.init_running_from_project();
            self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
            self.set_state(EngineState::Running);
            return true;
        }

        match self.receiver.try_recv() {
            Ok(LoopCommand::Stop | LoopCommand::SyncStop) => {
                self.set_state(EngineState::Stopped);
            }
            Ok(_) => {}
            Err(mpsc::TryRecvError::Disconnected) => return false,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        true
    }

    fn handle_running(&mut self) -> bool {
        let (events, bar_ticks) = if let Some(ctx) = self.pause_context.take() {
            let tick_of_next = ctx.remaining_events.first().map(|(t, _)| *t).unwrap_or(0);
            self.anchor = Instant::now()
                - Duration::from_micros(tick_of_next * self.scheduler.micros_per_tick());
            self.bar_index = ctx.bar_index;
            (ctx.remaining_events, ctx.bar_ticks)
        } else {
            match self.build_normal_bar() {
                BuildResult::Events(ev, bt) => (ev, bt),
                BuildResult::NoData => return true,
                BuildResult::Disconnected => return false,
            }
        };

        match self.play_events(events, bar_ticks) {
            BarOutcome::Complete => { self.advance_bar(bar_ticks); true }
            BarOutcome::Stopped | BarOutcome::Paused | BarOutcome::SyncRestart => true,
            BarOutcome::Disconnected => false,
        }
    }

    // T-24 (EP-5): Paused state — block waiting for ClockResume or ClockStop
    fn handle_paused(&mut self) -> bool {
        match self.receiver.recv() {
            Ok(LoopCommand::ClockResume) => {
                if let Err(e) = self.output.clock_continue() {
                    eprintln!("MIDI clock_continue failed: {e}");
                }
                self.set_state(EngineState::Running);
            }
            Ok(LoopCommand::ClockStop | LoopCommand::Stop) => {
                self.pause_context = None;
                if let Err(e) = self.output.clock_stop() {
                    eprintln!("MIDI clock_stop failed: {e}");
                }
                self.bar_index = 0;
                self.is_clock_mode = false;
                self.set_state(EngineState::Stopped);
            }
            Ok(_) => {}
            Err(_) => return false,
        }
        true
    }

    fn run(mut self) {
        loop {
            let should_continue = match self.state {
                EngineState::Stopped => self.handle_stopped(),
                EngineState::Waiting => self.handle_waiting(),
                EngineState::Running => self.handle_running(),
                EngineState::Paused => self.handle_paused(),
            };
            if !should_continue {
                return;
            }
        }
    }
}

pub fn run_player_loop(
    receiver: mpsc::Receiver<LoopCommand>,
    store: Arc<RwLock<ProjectStore>>,
    output: Box<dyn MidiOutput>,
    shared_state: Arc<Mutex<EngineState>>,
) {
    PlayerLoop::new(receiver, store, output, shared_state).run();
}
