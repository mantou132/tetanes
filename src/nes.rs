//! User Interface representing the the NES Game Deck

use crate::{
    bus::Bus,
    common::Clocked,
    cpu::{Cpu, CPU_CLOCK_RATE},
    logging::{LogLevel, Loggable},
    memory,
    nes::{
        config::DEFAULT_SPEED,
        debug::{DEBUG_WIDTH, INFO_HEIGHT, INFO_WIDTH},
        menus::Message,
    },
    ppu::{RENDER_HEIGHT, RENDER_WIDTH},
    NesResult,
};
use pix_engine::{event::PixEvent, sprite::Sprite, PixEngine, PixEngineResult, State, StateData};
use std::{collections::VecDeque, fmt};

mod config;
mod debug;
mod event;
mod menus;
mod state;

pub use config::NesConfig;

const ICON_PATH: &str = "static/rustynes_icon.png";
const APP_NAME: &str = "RustyNES";
const WINDOW_WIDTH: u32 = (RENDER_WIDTH as f32 * 8.0 / 7.0 + 0.5) as u32; // for 8:7 Aspect Ratio
const WINDOW_HEIGHT: u32 = RENDER_HEIGHT;
const REWIND_START: u8 = 5;
const REWIND_SIZE: u8 = 20;
const REWIND_TIMER: f32 = 5.0;

#[derive(Clone)]
pub struct Nes {
    roms: Vec<String>,
    loaded_rom: String,
    paused: bool,
    clock: f32,
    turbo_clock: u8,
    cpu: Cpu,
    cycles_remaining: f32,
    focused_window: u32,
    lost_focus: bool,
    menu: bool,
    cpu_break: bool,
    break_instr: Option<u16>,
    should_close: bool,
    nes_window: u32,
    ppu_viewer_window: Option<u32>,
    nt_viewer_window: Option<u32>,
    ppu_viewer: bool,
    nt_viewer: bool,
    nt_scanline: u32,
    pat_scanline: u32,
    debug_sprite: Sprite,
    ppu_info_sprite: Sprite,
    nt_info_sprite: Sprite,
    active_debug: bool,
    width: u32,
    height: u32,
    speed_counter: i32,
    rewind_timer: f32,
    rewind_slot: u8,
    rewind_save: u8,
    rewind_queue: VecDeque<u8>,
    replay_frame: usize,
    recording: bool,
    playback: bool,
    replay_buffer: Vec<Vec<PixEvent>>,
    messages: Vec<Message>,
    config: NesConfig,
}

impl Nes {
    pub fn new() -> Self {
        let config = NesConfig::default();
        Self::with_config(config).unwrap()
    }

    pub fn with_config(config: NesConfig) -> PixEngineResult<Self> {
        let scale = config.scale;
        let width = scale * WINDOW_WIDTH;
        let height = scale * WINDOW_HEIGHT;
        unsafe { memory::RANDOMIZE_RAM = config.randomize_ram }
        let cpu = Cpu::init(Bus::new());
        let mut nes = Self {
            roms: Vec::new(),
            loaded_rom: String::new(),
            paused: true,
            clock: 0.0,
            turbo_clock: 0,
            cpu,
            cycles_remaining: 0.0,
            focused_window: 0,
            lost_focus: false,
            menu: false,
            cpu_break: false,
            break_instr: None,
            should_close: false,
            nes_window: 0,
            ppu_viewer_window: None,
            nt_viewer_window: None,
            ppu_viewer: false,
            nt_viewer: false,
            nt_scanline: 0,
            pat_scanline: 0,
            debug_sprite: Sprite::new(DEBUG_WIDTH, height),
            ppu_info_sprite: Sprite::rgb(INFO_WIDTH, INFO_HEIGHT),
            nt_info_sprite: Sprite::rgb(INFO_WIDTH, INFO_HEIGHT),
            active_debug: false,
            width,
            height,
            speed_counter: 0,
            rewind_timer: REWIND_TIMER,
            rewind_slot: 0,
            rewind_save: 0,
            rewind_queue: VecDeque::with_capacity(REWIND_SIZE as usize),
            replay_frame: 0,
            recording: config.record,
            playback: false,
            replay_buffer: Vec::new(),
            messages: Vec::new(),
            config,
        };
        if nes.config.replay.is_some() {
            nes.playback = true;
            nes.load_replay()?;
        }
        Ok(nes)
    }

    pub fn run(self) -> NesResult<()> {
        let width = self.width;
        let height = self.height;
        let vsync = self.config.vsync;
        let mut engine = PixEngine::new(APP_NAME, self, width, height, vsync)?;
        engine.set_icon(ICON_PATH)?;
        engine.run()?;
        Ok(())
    }

    /// Steps the console the number of instructions required to generate an entire frame
    pub fn clock_frame(&mut self) {
        while !self.cpu_break && !self.cpu.bus.ppu.frame_complete {
            let _ = self.clock();
        }
        self.cpu_break = false;
        self.cpu.bus.ppu.frame_complete = false;
    }

    pub fn clock_seconds(&mut self, seconds: f32) {
        self.cycles_remaining += CPU_CLOCK_RATE * seconds;
        while !self.cpu_break && self.cycles_remaining > 0.0 {
            self.cycles_remaining -= self.clock() as f32;
        }
        if self.cpu_break {
            self.cycles_remaining = 0.0;
        }
        self.cpu_break = false;
    }
}

impl State for Nes {
    fn on_start(&mut self, data: &mut StateData) -> PixEngineResult<bool> {
        self.nes_window = data.main_window();

        // Before rendering anything, set up our textures
        self.create_textures(data)?;

        match self.find_roms() {
            Ok(mut roms) => self.roms.append(&mut roms),
            Err(e) => error!(self, "{}", e),
        }
        if self.roms.len() == 1 {
            self.load_rom(0)?;
            self.power_on()?;

            if self.config.clear_save {
                if let Ok(save_path) = state::save_path(&self.loaded_rom, self.config.save_slot) {
                    if save_path.exists() {
                        let _ = std::fs::remove_file(&save_path);
                        self.add_message(&format!("Cleared slot {}", self.config.save_slot));
                    }
                }
            } else {
                self.load_state(self.config.save_slot);
            }

            // Clean up previous rewind states
            for slot in REWIND_START..REWIND_SIZE {
                if let Ok(save_path) = state::save_path(&self.loaded_rom, slot) {
                    if save_path.exists() {
                        let _ = std::fs::remove_file(&save_path);
                    }
                }
            }

            let codes = self.config.genie_codes.to_vec();
            for code in codes {
                if let Err(e) = self.cpu.bus.add_genie_code(&code) {
                    self.add_message(&e.to_string());
                }
            }
            self.update_title(data);
        }

        if self.config.debug {
            self.config.debug = !self.config.debug;
            self.toggle_debug(data)?;
        }
        if self.config.speed != DEFAULT_SPEED {
            self.cpu.bus.apu.set_speed(self.config.speed);
        }

        self.set_log_level(self.config.log_level, true);

        if self.config.fullscreen {
            data.fullscreen(true)?;
        }

        Ok(true)
    }

    fn on_update(&mut self, elapsed: f32, data: &mut StateData) -> PixEngineResult<bool> {
        self.poll_events(data)?;
        if self.should_close {
            return Ok(false);
        }
        self.check_focus();
        self.update_title(data);

        // Save rewind snapshot
        if self.config.rewind_enabled && self.config.save_enabled {
            self.rewind_timer -= elapsed;
            if self.rewind_timer <= 0.0 {
                self.rewind_save %= REWIND_SIZE;
                if self.rewind_save < REWIND_START {
                    self.rewind_save = REWIND_START;
                }
                self.rewind_timer = REWIND_TIMER;
                self.save_state(self.rewind_save, true);
                self.messages.pop(); // Remove saved message
                self.rewind_queue.push_back(self.rewind_save);
                self.rewind_save += 1;
                if self.rewind_queue.len() > REWIND_SIZE as usize {
                    let _ = self.rewind_queue.pop_front();
                }
                self.rewind_slot = self.rewind_queue.len() as u8;
            }
        }

        if !self.paused {
            self.clock += elapsed;
            // Frames that aren't multiples of the default render 1 more/less frames
            // every other frame
            let mut frames_to_run = 0;
            self.speed_counter += (100.0 * self.config.speed) as i32;
            while self.speed_counter > 0 {
                self.speed_counter -= 100;
                frames_to_run += 1;
            }

            // Clock NES
            if self.config.unlock_fps {
                self.clock_seconds(self.config.speed * elapsed);
            } else {
                for _ in 0..frames_to_run as usize {
                    self.clock_frame();
                    self.turbo_clock = (1 + self.turbo_clock) % 6;
                }
            }
        }
        if !self.lost_focus {
            // Update screen
            data.copy_texture(self.nes_window, "nes", &self.cpu.bus.ppu.frame())?;
            if self.menu {
                self.draw_menu(data)?;
            }

            self.draw_messages(elapsed, data)?;

            if self.config.debug {
                if self.active_debug || self.paused {
                    self.draw_debug(data);
                }
                self.copy_debug(data)?;
            }
            if self.ppu_viewer {
                self.copy_ppu_viewer(data)?;
            }
            if self.nt_viewer {
                self.copy_nt_viewer(data)?;
            }
        }

        // Enqueue sound
        if self.config.sound_enabled {
            let samples = self.cpu.bus.apu.samples();
            data.enqueue_audio(&samples);
        }
        self.cpu.bus.apu.clear_samples();
        Ok(true)
    }

    fn on_stop(&mut self, _data: &mut StateData) -> PixEngineResult<bool> {
        self.power_off()?;
        Ok(true)
    }
}

impl fmt::Debug for Nes {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        write!(f, "Nes {{\n  cpu: {:?}\n}} ", self.cpu)
    }
}

impl Default for NesConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Nes {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemRead;

    fn load(rom: &str) -> Nes {
        let mut nes = Nes::new();
        nes.roms.push(rom.to_owned());
        nes.load_rom(0).unwrap();
        nes.power_on().unwrap();
        nes
    }

    #[test]
    fn nestest() {
        let rom = "tests/cpu/nestest.nes";
        let mut nes = load(&rom);
        nes.cpu.pc = 0xC000; // Start automated tests
        let _ = nes.clock_seconds(1.0);
        assert_eq!(nes.cpu.peek(0x0000), 0x00, "{}", rom);
    }

    #[test]
    fn dummy_writes_oam() {
        let rom = "tests/cpu/dummy_writes_oam.nes";
        let mut nes = load(&rom);
        let _ = nes.clock_seconds(6.0);
        assert_eq!(nes.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn dummy_writes_ppumem() {
        let rom = "tests/cpu/dummy_writes_ppumem.nes";
        let mut nes = load(&rom);
        let _ = nes.clock_seconds(4.0);
        assert_eq!(nes.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn exec_space_ppuio() {
        let rom = "tests/cpu/exec_space_ppuio.nes";
        let mut nes = load(&rom);
        let _ = nes.clock_seconds(2.0);
        assert_eq!(nes.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn instr_timing() {
        let rom = "tests/cpu/instr_timing.nes";
        let mut nes = load(&rom);
        let _ = nes.clock_seconds(23.0);
        assert_eq!(nes.cpu.peek(0x6000), 0x00, "{}", rom);
    }

    #[test]
    fn apu_timing() {
        let mut nes = Nes::new();
        nes.power_on().unwrap();
        for _ in 0..=29840 {
            let apu = &nes.cpu.bus.apu;
            println!(
                "{}: counter: {}, step: {}, irq: {}",
                nes.cpu.cycle_count,
                apu.frame_sequencer.divider.counter,
                apu.frame_sequencer.sequencer.step,
                apu.irq_pending
            );
            nes.clock();
        }
    }
}
