// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant};

use crate::domain::ProjectStore;

use super::midi::MidiOutput;
use super::scheduler::Scheduler;
use super::{EngineState, LoopCommand};

struct ActiveNote {
    channel: u8,
    pitch: u8,
}

// Remaining unprocessed events at pause time with loop context for resume.
struct PauseContext {
    remaining_events: Vec<(u64, LoopEvent)>,
    loop_duration: u64,
}

#[derive(Clone)]
enum LoopEvent {
    NoteOn {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        pitch: u8,
    },
    ClockPulse,
}

impl LoopEvent {
    fn priority(&self) -> u8 {
        match self {
            LoopEvent::NoteOff { .. } => 0,
            LoopEvent::NoteOn { .. } => 1,
            LoopEvent::ClockPulse => 2,
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
    SyncContinue,
    Disconnected,
}

enum LoopOutcome {
    Complete,
    Stopped,
    Paused,
    SyncRestart,
    Disconnected,
}

enum BuildResult {
    Events(Vec<(u64, LoopEvent)>),
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
                Ok(LoopCommand::SyncContinue) => return SleepResult::SyncContinue,
                Ok(LoopCommand::SyncBpmUpdate(bpm)) => *pending_sync_bpm = Some(bpm),
                Ok(LoopCommand::Start | LoopCommand::ClockStart | LoopCommand::ClockResume) => {}
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
    scheduler: Scheduler,
    anchor: Instant,
    is_clock_mode: bool,
    pause_context: Option<PauseContext>,
    loop_duration: u64,
    carry_over: Vec<(u64, LoopEvent)>,
    next_carry_over: Vec<(u64, LoopEvent)>,
    loop_elapsed_ticks: u64,
    pending_sync_bpm: Option<u32>,
    current_tick: Arc<AtomicU64>,
    loop_duration_ticks: Arc<AtomicU64>,
}

impl PlayerLoop {
    fn new(
        receiver: mpsc::Receiver<LoopCommand>,
        store: Arc<RwLock<ProjectStore>>,
        output: Box<dyn MidiOutput>,
        shared_state: Arc<Mutex<EngineState>>,
        current_tick: Arc<AtomicU64>,
        loop_duration_ticks: Arc<AtomicU64>,
    ) -> Self {
        PlayerLoop {
            receiver,
            store,
            output,
            shared_state,
            state: EngineState::Stopped,
            active_notes: Vec::new(),
            last_instruments: HashMap::new(),
            scheduler: Scheduler::new(120),
            anchor: Instant::now(),
            is_clock_mode: false,
            pause_context: None,
            loop_duration: 480,
            carry_over: Vec::new(),
            next_carry_over: Vec::new(),
            loop_elapsed_ticks: 0,
            pending_sync_bpm: None,
            current_tick,
            loop_duration_ticks,
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
        self.current_tick.store(0, Ordering::Relaxed);
        self.flush_notes();
        self.carry_over.clear();
        self.next_carry_over.clear();
        if self.is_clock_mode {
            if let Err(e) = self.output.clock_stop() {
                eprintln!("MIDI clock_stop failed: {e}");
            }
            self.is_clock_mode = false;
        }
        self.set_state(EngineState::Stopped);
    }

    fn do_clock_stop(&mut self) {
        self.current_tick.store(0, Ordering::Relaxed);
        self.flush_notes();
        self.carry_over.clear();
        self.next_carry_over.clear();
        if let Err(e) = self.output.clock_stop() {
            eprintln!("MIDI clock_stop failed: {e}");
        }
        self.is_clock_mode = false;
        self.set_state(EngineState::Stopped);
    }

    fn do_sync_stop(&mut self) {
        self.current_tick.store(0, Ordering::Relaxed);
        self.flush_notes();
        self.carry_over.clear();
        self.next_carry_over.clear();
        self.set_state(EngineState::Stopped);
    }

    fn do_sync_restart(&mut self) {
        self.current_tick.store(0, Ordering::Relaxed);
        self.flush_notes();
        self.carry_over.clear();
        self.next_carry_over.clear();
        self.last_instruments.clear();
        self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
    }

    fn do_pause(&mut self, remaining: Vec<(u64, LoopEvent)>) {
        self.flush_notes();
        self.pause_context = Some(PauseContext {
            remaining_events: remaining,
            loop_duration: self.loop_duration,
        });
        self.set_state(EngineState::Paused);
    }

    fn do_sync_continue(&mut self) {
        // current_tick is intentionally not written here: the counter resumes
        // incrementing from its frozen value on Continue (F-10).
        if let BuildResult::Events(events) = self.build_loop_events() {
            let elapsed = self.loop_elapsed_ticks;
            let filtered: Vec<_> = events
                .into_iter()
                .filter(|(tick, _)| *tick >= elapsed)
                .collect();
            self.pause_context = Some(PauseContext {
                remaining_events: filtered,
                loop_duration: self.loop_duration,
            });
        }
    }

    fn handle_sleep_result(
        &mut self,
        result: SleepResult,
        remaining: &[(u64, LoopEvent)],
    ) -> Option<LoopOutcome> {
        match result {
            SleepResult::Elapsed => None,
            SleepResult::Stop => {
                self.do_stop();
                Some(LoopOutcome::Stopped)
            }
            SleepResult::ClockStop => {
                self.do_clock_stop();
                Some(LoopOutcome::Stopped)
            }
            SleepResult::SyncStop => {
                self.do_sync_stop();
                Some(LoopOutcome::Stopped)
            }
            SleepResult::SyncStart => {
                self.do_sync_restart();
                Some(LoopOutcome::SyncRestart)
            }
            SleepResult::SyncContinue => {
                self.do_sync_continue();
                Some(LoopOutcome::SyncRestart)
            }
            SleepResult::ClockPause => {
                self.do_pause(remaining.to_vec());
                Some(LoopOutcome::Paused)
            }
            SleepResult::Disconnected => Some(LoopOutcome::Disconnected),
        }
    }

    fn handle_mid_loop_command(
        &mut self,
        cmd: LoopCommand,
        remaining: &[(u64, LoopEvent)],
    ) -> Option<LoopOutcome> {
        match cmd {
            LoopCommand::Stop => {
                self.do_stop();
                Some(LoopOutcome::Stopped)
            }
            LoopCommand::ClockStop => {
                self.do_clock_stop();
                Some(LoopOutcome::Stopped)
            }
            LoopCommand::SyncStop => {
                self.do_sync_stop();
                Some(LoopOutcome::Stopped)
            }
            LoopCommand::SyncStart => {
                self.do_sync_restart();
                Some(LoopOutcome::SyncRestart)
            }
            LoopCommand::SyncContinue => {
                self.do_sync_continue();
                Some(LoopOutcome::SyncRestart)
            }
            LoopCommand::ClockPause => {
                self.do_pause(remaining.to_vec());
                Some(LoopOutcome::Paused)
            }
            LoopCommand::SyncBpmUpdate(bpm) => {
                self.pending_sync_bpm = Some(bpm);
                None
            }
            LoopCommand::Start | LoopCommand::ClockStart | LoopCommand::ClockResume => None,
        }
    }

    fn init_running_from_project(&mut self) {
        let store_r = self.store.read().unwrap();
        let project = store_r.active().unwrap();
        self.scheduler = Scheduler::new(project.header.bpm);
        self.loop_duration = project.header.loop_duration as u64;
    }

    fn emit_event(&mut self, event: &LoopEvent) {
        match event {
            LoopEvent::ClockPulse => {
                if let Err(e) = self.output.clock_tick() {
                    eprintln!("MIDI clock_tick failed: {e}");
                }
            }
            LoopEvent::NoteOn {
                channel,
                pitch,
                velocity,
            } => {
                if let Err(e) = self.output.note_on(*channel, *pitch, *velocity) {
                    eprintln!("MIDI note_on failed: {e}");
                }
                self.active_notes.push(ActiveNote {
                    channel: *channel,
                    pitch: *pitch,
                });
            }
            LoopEvent::NoteOff { channel, pitch } => {
                if let Err(e) = self.output.note_off(*channel, *pitch) {
                    eprintln!("MIDI note_off failed: {e}");
                }
                self.active_notes
                    .retain(|n| !(n.channel == *channel && n.pitch == *pitch));
            }
        }
    }

    fn play_events(&mut self, events: Vec<(u64, LoopEvent)>) -> LoopOutcome {
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
            if let Some(outcome) = self.handle_sleep_result(sleep_result, &events[i..]) {
                return outcome;
            }

            self.loop_elapsed_ticks = tick;
            self.current_tick.store(tick, Ordering::Relaxed);
            let event = &events[i].1;
            self.emit_event(event);

            match self.receiver.try_recv() {
                Ok(cmd) => {
                    if let Some(outcome) = self.handle_mid_loop_command(cmd, &events[i + 1..]) {
                        return outcome;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => return LoopOutcome::Disconnected,
                Err(mpsc::TryRecvError::Empty) => {}
            }

            i += 1;
        }
        LoopOutcome::Complete
    }

    // Builds the event list for the current loop pass from the store. Returns NoData when the
    // loop should be skipped (state may have changed) and Disconnected when the channel dropped.
    fn build_loop_events(&mut self) -> BuildResult {
        {
            let store_r = self.store.read().unwrap();
            if let Some(project) = store_r.active() {
                for track in &project.tracks {
                    let last = self.last_instruments.get(&track.channel).copied();
                    if last != Some(track.instrument) {
                        if let Err(e) = self.output.program_change(track.channel, track.instrument)
                        {
                            eprintln!("MIDI program_change failed: {e}");
                        }
                        self.last_instruments
                            .insert(track.channel, track.instrument);
                    }
                }
            }
        }

        let store_r = self.store.read().unwrap();
        match store_r.active() {
            Some(project) => {
                let loop_duration = project.header.loop_duration as u64;
                if loop_duration == 0 {
                    drop(store_r);
                    match self.receiver.try_recv() {
                        Ok(cmd) => {
                            if let Some(outcome) = self.handle_mid_loop_command(cmd, &[]) {
                                if matches!(outcome, LoopOutcome::Disconnected) {
                                    return BuildResult::Disconnected;
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Disconnected) => return BuildResult::Disconnected,
                        Err(mpsc::TryRecvError::Empty) => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                    return BuildResult::NoData;
                }

                self.loop_duration = loop_duration;

                // Prepend carry-over NoteOff events from the previous loop iteration.
                let carry = std::mem::take(&mut self.carry_over);
                let mut events: Vec<(u64, LoopEvent)> = carry;
                let mut overflow: Vec<(u64, LoopEvent)> = Vec::new();

                for track in &project.tracks {
                    for note in &track.notes {
                        events.push((
                            note.start_tick as u64,
                            LoopEvent::NoteOn {
                                channel: track.channel,
                                pitch: note.pitch,
                                velocity: note.velocity,
                            },
                        ));
                        let note_off_tick = note.start_tick as u64 + note.duration as u64;
                        if note_off_tick > loop_duration {
                            overflow.push((
                                note_off_tick,
                                LoopEvent::NoteOff {
                                    channel: track.channel,
                                    pitch: note.pitch,
                                },
                            ));
                        } else {
                            events.push((
                                note_off_tick,
                                LoopEvent::NoteOff {
                                    channel: track.channel,
                                    pitch: note.pitch,
                                },
                            ));
                        }
                    }
                }

                if self.is_clock_mode {
                    let mut cp = 0u64;
                    while cp < loop_duration {
                        events.push((cp, LoopEvent::ClockPulse));
                        cp += 20;
                    }
                }

                events.sort_unstable_by_key(|(tick, ev)| (*tick, ev.priority()));
                self.next_carry_over = overflow;
                BuildResult::Events(events)
            }
            None => {
                if self.is_clock_mode {
                    let ld = self.loop_duration;
                    let carry = std::mem::take(&mut self.carry_over);
                    let mut events: Vec<(u64, LoopEvent)> = carry;
                    let mut cp = 0u64;
                    while cp < ld {
                        events.push((cp, LoopEvent::ClockPulse));
                        cp += 20;
                    }
                    events.sort_unstable_by_key(|(tick, ev)| (*tick, ev.priority()));
                    BuildResult::Events(events)
                } else {
                    drop(store_r);
                    match self.receiver.try_recv() {
                        Ok(cmd) => {
                            self.handle_mid_loop_command(cmd, &[]);
                        }
                        Err(mpsc::TryRecvError::Disconnected) => return BuildResult::Disconnected,
                        Err(mpsc::TryRecvError::Empty) => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                    BuildResult::NoData
                }
            }
        }
    }

    fn advance_loop(&mut self) {
        self.anchor = self
            .scheduler
            .deadline_for_tick(self.anchor, self.loop_duration);

        self.store.write().unwrap().commit_pending();

        let loop_dur = self
            .store
            .read()
            .unwrap()
            .active()
            .map(|p| p.header.loop_duration as u64)
            .unwrap_or(0);
        self.loop_duration_ticks.store(loop_dur, Ordering::Relaxed);

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

        // Convert absolute overflow ticks to offsets relative to the next loop start.
        let ld = self.loop_duration;
        self.carry_over = self
            .next_carry_over
            .drain(..)
            .map(|(tick, event)| (tick - ld, event))
            .collect();
        self.carry_over.sort_unstable_by_key(|(offset, _)| *offset);

        self.loop_elapsed_ticks = 0;
        self.current_tick.store(0, Ordering::Relaxed);
    }

    // Returns false when the player thread should exit.
    fn handle_stopped(&mut self) -> bool {
        match self.receiver.recv() {
            Ok(LoopCommand::Start) => {
                let has_project = self.store.read().unwrap().active().is_some();
                if has_project {
                    self.last_instruments.clear();
                    self.init_running_from_project();
                    self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                    self.is_clock_mode = false;
                    self.set_state(EngineState::Running);
                } else {
                    self.set_state(EngineState::Waiting);
                }
            }
            Ok(LoopCommand::ClockStart) => {
                self.last_instruments.clear();
                self.init_running_from_project();
                self.anchor = Instant::now() + Duration::from_micros(START_LATENCY_MICROS);
                self.is_clock_mode = true;
                if let Err(e) = self.output.clock_start() {
                    eprintln!("MIDI clock_start failed: {e}");
                }
                self.set_state(EngineState::Running);
            }
            Ok(LoopCommand::SyncStart) => {
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
            Ok(
                LoopCommand::Stop
                | LoopCommand::ClockStop
                | LoopCommand::SyncStop
                | LoopCommand::ClockPause
                | LoopCommand::ClockResume,
            ) => {}
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
        let events = if let Some(ctx) = self.pause_context.take() {
            let tick_of_next = ctx.remaining_events.first().map(|(t, _)| *t).unwrap_or(0);
            self.anchor = Instant::now()
                - Duration::from_micros(tick_of_next * self.scheduler.micros_per_tick());
            self.loop_duration = ctx.loop_duration;
            ctx.remaining_events
        } else {
            match self.build_loop_events() {
                BuildResult::Events(ev) => ev,
                BuildResult::NoData => return true,
                BuildResult::Disconnected => return false,
            }
        };

        match self.play_events(events) {
            LoopOutcome::Complete => {
                self.advance_loop();
                true
            }
            LoopOutcome::Stopped | LoopOutcome::Paused | LoopOutcome::SyncRestart => true,
            LoopOutcome::Disconnected => false,
        }
    }

    fn handle_paused(&mut self) -> bool {
        match self.receiver.recv() {
            // current_tick is intentionally not written here: the counter freezes during
            // pause and resumes from its frozen value on Continue (F-5 / F-10).
            Ok(LoopCommand::ClockResume) => {
                if let Err(e) = self.output.clock_continue() {
                    eprintln!("MIDI clock_continue failed: {e}");
                }
                self.set_state(EngineState::Running);
            }
            Ok(LoopCommand::ClockStop | LoopCommand::Stop) => {
                self.current_tick.store(0, Ordering::Relaxed);
                self.pause_context = None;
                if let Err(e) = self.output.clock_stop() {
                    eprintln!("MIDI clock_stop failed: {e}");
                }
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
    current_tick: Arc<AtomicU64>,
    loop_duration_ticks: Arc<AtomicU64>,
) {
    PlayerLoop::new(
        receiver,
        store,
        output,
        shared_state,
        current_tick,
        loop_duration_ticks,
    )
    .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Header, Note, Project, ProjectStore, Track};
    use crate::loop_engine::LoopEngine;
    use crate::loop_engine::midi::{CapturingMidiOutput, MidiEvent};
    use std::sync::{Arc, Mutex, RwLock};

    fn make_store(project: Option<Project>) -> Arc<RwLock<ProjectStore>> {
        let store = Arc::new(RwLock::new(ProjectStore::new()));
        if let Some(p) = project {
            store.write().unwrap().set_pending(p).unwrap();
            store.write().unwrap().commit_pending();
        }
        store
    }

    fn make_player(
        project: Option<Project>,
    ) -> (
        PlayerLoop,
        mpsc::Sender<LoopCommand>,
        Arc<Mutex<Vec<MidiEvent>>>,
    ) {
        let (tx, rx) = mpsc::channel();
        let store = make_store(project);
        let recorded: Arc<Mutex<Vec<MidiEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let output = Box::new(CapturingMidiOutput::new(Arc::clone(&recorded)));
        let shared_state = Arc::new(Mutex::new(EngineState::Stopped));
        let current_tick = Arc::new(AtomicU64::new(0));
        let loop_duration_ticks = Arc::new(AtomicU64::new(0));
        let player = PlayerLoop::new(
            rx,
            store,
            output,
            shared_state,
            current_tick,
            loop_duration_ticks,
        );
        (player, tx, recorded)
    }

    fn make_store_with_project(project: Project) -> Arc<RwLock<ProjectStore>> {
        make_store(Some(project))
    }

    fn project_with_note(loop_duration: u32, note: Note) -> Project {
        Project {
            header: Header {
                bpm: 120,
                loop_duration,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![note],
            }],
        }
    }

    #[test]
    fn test_loop_event_priority() {
        assert_eq!(
            LoopEvent::NoteOff {
                channel: 1,
                pitch: 60
            }
            .priority(),
            0
        );
        assert_eq!(
            LoopEvent::NoteOn {
                channel: 1,
                pitch: 60,
                velocity: 80
            }
            .priority(),
            1
        );
        assert_eq!(LoopEvent::ClockPulse.priority(), 2);
    }

    #[test]
    fn test_player_loop_fields() {
        let (player, _tx, _) = make_player(None);
        assert_eq!(player.loop_duration, 480);
        assert!(player.carry_over.is_empty());
        assert_eq!(player.loop_elapsed_ticks, 0);
    }

    // start_tick + duration.
    #[test]
    fn test_build_loop_events_single_note() {
        let p = project_with_note(
            1920,
            Note {
                start_tick: 0,
                duration: 480,
                pitch: 60,
                velocity: 80,
            },
        );
        let (mut player, _tx, _) = make_player(Some(p));
        let result = player.build_loop_events();
        let BuildResult::Events(events) = result else {
            panic!("expected Events")
        };

        let on_tick = events
            .iter()
            .find(|(_, e)| matches!(e, LoopEvent::NoteOn { pitch: 60, .. }))
            .map(|(t, _)| *t);
        let off_tick = events
            .iter()
            .find(|(_, e)| matches!(e, LoopEvent::NoteOff { pitch: 60, .. }))
            .map(|(t, _)| *t);

        assert_eq!(on_tick, Some(0), "NoteOn must be at tick 0");
        assert_eq!(off_tick, Some(480), "NoteOff must be at tick 480");
    }

    #[test]
    fn test_build_loop_events_two_notes_same_tick() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![
                    Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 60,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 64,
                        velocity: 80,
                    },
                ],
            }],
        };
        let (mut player, _tx, _) = make_player(Some(p));
        let BuildResult::Events(events) = player.build_loop_events() else {
            panic!("expected Events")
        };

        let note_ons: Vec<_> = events
            .iter()
            .filter(|(t, e)| *t == 0 && matches!(e, LoopEvent::NoteOn { .. }))
            .collect();
        let note_offs: Vec<_> = events
            .iter()
            .filter(|(t, e)| *t == 480 && matches!(e, LoopEvent::NoteOff { .. }))
            .collect();

        assert_eq!(note_ons.len(), 2, "expected two NoteOn events at tick 0");
        assert_eq!(
            note_offs.len(),
            2,
            "expected two NoteOff events at tick 480"
        );
    }

    // carry_over contains one entry at the correct tick offset.
    #[test]
    fn test_carry_over_collected() {
        let p = project_with_note(
            1920,
            Note {
                start_tick: 0,
                duration: 1921,
                pitch: 60,
                velocity: 80,
            },
        );
        let (mut player, _tx, _) = make_player(Some(p));

        let BuildResult::Events(events) = player.build_loop_events() else {
            panic!("expected Events")
        };

        let has_note_off = events
            .iter()
            .any(|(_, e)| matches!(e, LoopEvent::NoteOff { pitch: 60, .. }));
        assert!(
            !has_note_off,
            "cross-loop NoteOff must not appear in main event list"
        );

        let has_note_on = events
            .iter()
            .any(|(_, e)| matches!(e, LoopEvent::NoteOn { pitch: 60, .. }));
        assert!(has_note_on, "NoteOn must still appear in main event list");

        player.advance_loop();

        assert_eq!(
            player.carry_over.len(),
            1,
            "carry_over must have one entry after advance_loop"
        );
        assert_eq!(
            player.carry_over[0].0, 1,
            "carry_over offset must be 1921 - 1920 = 1"
        );
        assert!(
            matches!(player.carry_over[0].1, LoopEvent::NoteOff { pitch: 60, .. }),
            "carry_over entry must be a NoteOff"
        );
    }

    // cleared at the start of build_loop_events().
    #[test]
    fn test_carry_over_prepended() {
        let p = project_with_note(
            1920,
            Note {
                start_tick: 480,
                duration: 480,
                pitch: 62,
                velocity: 80,
            },
        );
        let (mut player, _tx, _) = make_player(Some(p));
        player.carry_over = vec![(
            1,
            LoopEvent::NoteOff {
                channel: 1,
                pitch: 60,
            },
        )];

        let BuildResult::Events(events) = player.build_loop_events() else {
            panic!("expected Events")
        };

        assert!(
            player.carry_over.is_empty(),
            "carry_over must be cleared after build"
        );

        let has_carry = events
            .iter()
            .any(|(t, e)| *t == 1 && matches!(e, LoopEvent::NoteOff { pitch: 60, .. }));
        assert!(
            has_carry,
            "carry_over NoteOff at tick 1 must appear in events"
        );
    }

    #[test]
    fn test_advance_loop_resets_elapsed_ticks() {
        let p = project_with_note(
            1920,
            Note {
                start_tick: 0,
                duration: 480,
                pitch: 60,
                velocity: 80,
            },
        );
        let (mut player, _tx, _) = make_player(Some(p));
        player.loop_elapsed_ticks = 480;
        player.advance_loop();
        assert_eq!(player.loop_elapsed_ticks, 0);
    }

    #[test]
    fn test_advance_loop_commits_pending() {
        let initial = Project {
            header: Header {
                bpm: 120,
                loop_duration: 960,
            },
            tracks: vec![],
        };
        let store = make_store_with_project(initial);

        let pending = Project {
            header: Header {
                bpm: 140,
                loop_duration: 960,
            },
            tracks: vec![],
        };
        store.write().unwrap().set_pending(pending).unwrap();

        assert_eq!(store.read().unwrap().active().unwrap().header.bpm, 120);

        let (tx, rx) = mpsc::channel();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let mut player = PlayerLoop::new(
            rx,
            Arc::clone(&store),
            Box::new(CapturingMidiOutput::new(recorded)),
            Arc::new(Mutex::new(EngineState::Stopped)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        );
        player.loop_duration = 960;
        drop(tx);

        player.advance_loop();

        assert_eq!(
            store.read().unwrap().active().unwrap().header.bpm,
            140,
            "pending project must be committed by advance_loop"
        );
        assert_eq!(player.loop_elapsed_ticks, 0);
    }

    #[test]
    fn test_loop_duration_cached_without_project() {
        let (mut player, _tx, _) = make_player(None);
        player.loop_duration = 1920;
        player.is_clock_mode = true;

        let BuildResult::Events(events) = player.build_loop_events() else {
            panic!("expected Events")
        };

        let pulse_count = events
            .iter()
            .filter(|(_, e)| matches!(e, LoopEvent::ClockPulse))
            .count();
        assert_eq!(
            pulse_count, 96,
            "expected 96 clock pulses for cached loop_duration 1920"
        );
        let last_tick = events
            .iter()
            .filter(|(_, e)| matches!(e, LoopEvent::ClockPulse))
            .last()
            .unwrap()
            .0;
        assert_eq!(last_tick, 1900);
    }

    // 0, 20, 40, …, 1900.
    #[test]
    fn test_clock_pulses_span_loop_duration() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![],
        };
        let (mut player, _tx, _) = make_player(Some(p));
        player.is_clock_mode = true;

        let BuildResult::Events(events) = player.build_loop_events() else {
            panic!("expected Events")
        };

        let pulses: Vec<u64> = events
            .iter()
            .filter(|(_, e)| matches!(e, LoopEvent::ClockPulse))
            .map(|(t, _)| *t)
            .collect();
        assert_eq!(pulses.len(), 96, "expected exactly 96 clock pulses");
        assert_eq!(pulses[0], 0);
        assert_eq!(pulses[95], 1900);
        for w in pulses.windows(2) {
            assert_eq!(w[1] - w[0], 20, "pulses must be spaced 20 ticks apart");
        }
    }

    #[test]
    fn test_init_running_from_project_reads_loop_duration() {
        let p = Project {
            header: Header {
                bpm: 140,
                loop_duration: 3840,
            },
            tracks: vec![],
        };
        let store = make_store_with_project(p);
        let (tx, rx) = mpsc::channel();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let mut player = PlayerLoop::new(
            rx,
            Arc::clone(&store),
            Box::new(CapturingMidiOutput::new(recorded)),
            Arc::new(Mutex::new(EngineState::Stopped)),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        );
        drop(tx);

        player.init_running_from_project();

        assert_eq!(player.loop_duration, 3840);
        assert_eq!(player.scheduler.bpm(), 140);
    }

    #[test]
    fn test_sync_continue_resumes_mid_loop() {
        let p = Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![
                    Note {
                        start_tick: 0,
                        duration: 480,
                        pitch: 60,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 480,
                        duration: 480,
                        pitch: 62,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 960,
                        duration: 480,
                        pitch: 64,
                        velocity: 80,
                    },
                    Note {
                        start_tick: 1440,
                        duration: 480,
                        pitch: 65,
                        velocity: 80,
                    },
                ],
            }],
        };
        let (mut player, _tx, _) = make_player(Some(p));
        player.loop_elapsed_ticks = 480;

        player.do_sync_continue();

        let ctx = player
            .pause_context
            .as_ref()
            .expect("pause_context must be set by do_sync_continue");
        let ticks: Vec<u64> = ctx.remaining_events.iter().map(|(t, _)| *t).collect();
        assert!(!ticks.contains(&0), "tick 0 must be filtered out");
        assert!(ticks.contains(&480), "tick 480 must be retained");
        assert!(ticks.contains(&960), "tick 960 must be retained");
        assert!(ticks.contains(&1440), "tick 1440 must be retained");
    }

    #[test]
    fn test_do_stop_clears_carry_over() {
        let (mut player, _tx, _) = make_player(None);
        player.carry_over = vec![(
            1,
            LoopEvent::NoteOff {
                channel: 1,
                pitch: 60,
            },
        )];

        player.do_stop();

        assert!(
            player.carry_over.is_empty(),
            "carry_over must be empty after do_stop"
        );
    }

    #[test]
    fn test_do_sync_stop_clears_carry_over() {
        let (mut player, _tx, _) = make_player(None);
        player.carry_over = vec![(
            1,
            LoopEvent::NoteOff {
                channel: 1,
                pitch: 60,
            },
        )];

        player.do_sync_stop();

        assert!(
            player.carry_over.is_empty(),
            "carry_over must be empty after do_sync_stop"
        );
    }

    #[test]
    fn test_do_sync_restart_clears_carry_over() {
        let (mut player, _tx, _) = make_player(None);
        player.carry_over = vec![(
            1,
            LoopEvent::NoteOff {
                channel: 1,
                pitch: 60,
            },
        )];
        player.last_instruments.insert(1, 42);

        player.do_sync_restart();

        assert!(
            player.carry_over.is_empty(),
            "carry_over must be empty after do_sync_restart"
        );
        assert!(
            player.last_instruments.is_empty(),
            "last_instruments must be cleared by do_sync_restart"
        );
    }

    #[test]
    fn test_pause_stores_context() {
        let remaining = vec![
            (
                480,
                LoopEvent::NoteOff {
                    channel: 1,
                    pitch: 60,
                },
            ),
            (
                960,
                LoopEvent::NoteOn {
                    channel: 1,
                    pitch: 64,
                    velocity: 80,
                },
            ),
        ];
        let (mut player, _tx, _) = make_player(None);
        player.loop_duration = 1920;

        player.do_pause(remaining.clone());

        let ctx = player
            .pause_context
            .as_ref()
            .expect("pause_context must be set");
        assert_eq!(ctx.loop_duration, 1920);
        assert_eq!(ctx.remaining_events.len(), 2);
        assert_eq!(ctx.remaining_events[0].0, 480);
        assert_eq!(ctx.remaining_events[1].0, 960);
    }

    // Integration: player emits NoteOn and NoteOff from Note.start_tick and Note.duration.
    #[test]
    fn player_emits_note_on_off_from_start_tick_and_duration() {
        let project = Project {
            header: Header {
                bpm: 300,
                loop_duration: 960,
            },
            tracks: vec![Track {
                name: "t".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
            }],
        };
        let store = make_store_with_project(project);
        let recorded: Arc<Mutex<Vec<MidiEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = Arc::clone(&recorded);

        let engine = Arc::new(LoopEngine::new(
            Arc::clone(&store),
            Box::new(CapturingMidiOutput::new(recorded_clone)),
        ));
        engine.start();
        // Allow one loop pass to complete (loop_duration=960 ticks at BPM 300 ≈ 384ms).
        std::thread::sleep(Duration::from_millis(500));
        engine.stop();
        std::thread::sleep(Duration::from_millis(50));

        let events = recorded.lock().unwrap().clone();
        assert!(
            events.iter().any(|e| matches!(
                e,
                MidiEvent::NoteOn {
                    channel: 1,
                    pitch: 60,
                    velocity: 80
                }
            )),
            "expected NoteOn(ch=1, pitch=60, vel=80) in {:?}",
            events
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                MidiEvent::NoteOff {
                    channel: 1,
                    pitch: 60
                }
            )),
            "expected NoteOff(ch=1, pitch=60) in {:?}",
            events
        );
    }
}
