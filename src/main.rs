#![allow(dead_code)]

use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc, Mutex,
};

use log::{debug, error, warn};

use modular_bitfield::prelude::*;

use pixels::{Pixels, SurfaceTexture};

use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::wayland::EventLoopBuilderExtWayland;
use winit::window::Window;

use awedio::Sound;

use emu8080::Intel8080;
use emu8080::MemoryAccess;
use emu8080::CYCLE_TIME_NANO_SECS;

#[allow(non_camel_case_types)]

const SCREEN_WIDTH_PIXELS: usize = 256;
const SCREEN_HEIGHT_PIXELS: usize = 224;

const DISPLAY_WIDTH_PIXELS: usize = SCREEN_HEIGHT_PIXELS;
const DISPLAY_HEIGHT_PIXELS: usize = SCREEN_WIDTH_PIXELS;

const SCREEN_SIZE_PIXELS: usize = SCREEN_WIDTH_PIXELS * SCREEN_HEIGHT_PIXELS;
const DISPLAY_BUFFER_SIZE: usize = SCREEN_SIZE_PIXELS * 4;

const DISPLAY_TIME_NANO_SEC: u64 = 16_666_667;

const ROM_SIZE: usize = 0x2000;
const RAM_SIZE: usize = 0x400;
const VRAM_SIZE: usize = 0x1C00;

const ROM_START: usize = 0;
const RAM_START: usize = 0x2000;
const VRAM_START: usize = 0x2400;

const ROM_END: usize = ROM_START + ROM_SIZE;
const RAM_END: usize = RAM_START + RAM_SIZE;
const VRAM_END: usize = VRAM_START + VRAM_SIZE;

const RAM_MASK: usize = 0x3FFF;

struct SpaceInvadersMemory {
    rom: [u8; ROM_SIZE],
    ram: [u8; RAM_SIZE],
    vram: std::vec::Vec<u8>,
}

impl MemoryAccess for SpaceInvadersMemory {
    fn read_byte(&self, addr: u16) -> u8 {
        let addr: usize = addr as usize & RAM_MASK;

        if addr < ROM_END {
            return self.rom[addr];
        } else if addr < RAM_END {
            return self.ram[addr - RAM_START];
        } else if addr < VRAM_END {
            return self.read_vram(addr - VRAM_START);
        }

        return 0;
    }

    fn write_byte(&mut self, addr: u16, val: u8) {
        let addr: usize = addr as usize & RAM_MASK;

        if RAM_START <= addr && addr < RAM_END {
            self.ram[addr - RAM_START] = val;
        } else if VRAM_START <= addr && addr < VRAM_END {
            self.write_vram(addr - VRAM_START, val);
        }
    }

    fn read_bytes<const C: usize>(&self, addr: u16) -> [u8; C] {
        let addr: usize = addr as usize & RAM_MASK;

        if addr < ROM_END {
            let start = addr;
            let end = start + C;
            unsafe { self.rom[start..end].try_into().unwrap_unchecked() }
        } else if RAM_START <= addr && addr < RAM_END {
            let start = addr - RAM_START;
            let end = start + C;
            unsafe { self.ram[start..end].try_into().unwrap_unchecked() }
        } else {
            let mut ret = [0x00u8; C];
            let start = addr - VRAM_START;
            for i in 0..C {
                ret[i] = self.read_vram(start + i);
            }
            ret
        }
    }

    fn write_bytes(&mut self, addr: u16, val: &[u8]) {
        let addr: usize = addr as usize & RAM_MASK;
        if addr < ROM_END {
            let start = addr;
            let end = start + val.len();
            self.rom[start..end].copy_from_slice(val);
        } else if RAM_START <= addr && addr < RAM_END {
            let start = addr - RAM_START;
            let end = start + val.len();
            self.ram[start..end].copy_from_slice(val);
        } else if VRAM_START <= addr && VRAM_END <= addr {
            for i in 0..val.len() {
                self.write_vram(addr + i, val[i]);
            }
        }
    }
}

impl SpaceInvadersMemory {
    fn new(rom: [u8; ROM_SIZE]) -> Self {
        SpaceInvadersMemory {
            rom,
            ram: [0 as u8; RAM_SIZE],
            vram: vec![0 as u8; DISPLAY_BUFFER_SIZE],
        }
    }

    fn get_display_pixel_address(&self, address: usize, pixel: u8) -> usize {
        let pixel_address = (address * 8) + pixel as usize;
        let display_col = pixel_address / SCREEN_WIDTH_PIXELS;
        let display_row = SCREEN_WIDTH_PIXELS - 1 - (pixel_address % SCREEN_WIDTH_PIXELS);

        return (display_row * DISPLAY_WIDTH_PIXELS) + display_col;
    }

    fn write_vram(&mut self, address: usize, val: u8) {
        const DISPLAY_PIXEL_SIZE: usize = 4;
        const WHITE_PIXEL: [u8; DISPLAY_PIXEL_SIZE] = [0xFF, 0xFF, 0xFF, 0xFF];
        const BLACK_PIXEL: [u8; DISPLAY_PIXEL_SIZE] = [0x00, 0x00, 0x00, 0xFF];

        // the screen is rotated 90 degrees counter-clockwise
        // so pixel address 0 is at (FRAME_HEIGHT, 0) or FRAME_HEIGHT * FRAME_WIDTH * PIXEL_DEPTH
        // pixel address 1 is at (1, FRAME_HEIGHT) or FRAME_HEIGHT * (FRAME_WIDTH - 1) * PIXEL_DEPTH
        let starting_pixel_address: usize = 8 * address as usize;
        let starting_pixel_display_col: usize = starting_pixel_address / SCREEN_WIDTH_PIXELS;
        let starting_pixel_display_row: usize =
            SCREEN_WIDTH_PIXELS - 1 - (starting_pixel_address % SCREEN_WIDTH_PIXELS);

        for i in 0..8 {
            let pixel_display_row: usize = starting_pixel_display_row - i;
            let pixel_display_address =
                pixel_display_row * DISPLAY_WIDTH_PIXELS as usize + starting_pixel_display_col;
            let byte_address = pixel_display_address * DISPLAY_PIXEL_SIZE;

            let mask: u8 = 0x1 << i;

            // pixel is white
            if mask & val != 0 {
                self.vram[byte_address..byte_address + 4].copy_from_slice(&WHITE_PIXEL);
            }
            // pixel is black
            else {
                self.vram[byte_address..byte_address + 4].copy_from_slice(&BLACK_PIXEL);
            }
        }
    }

    fn read_vram(&self, address: usize) -> u8 {
        let mut byte = 0;
        for i in 0..8 {
            let byte_address = self.get_display_pixel_address(address, i) * 4;
            let pixel = self.vram[byte_address];

            if pixel != 0 {
                byte |= 0x1 << i;
            }
        }
        byte
    }

    fn get_p1_score(&self) -> u16 {
        u16::from_le_bytes(self.read_bytes::<2>(0x20F8).try_into().unwrap())
    }

    fn get_p2_score(&self) -> u16 {
        u16::from_le_bytes(self.read_bytes::<2>(0x20FC).try_into().unwrap())
    }
}

#[bitfield]
struct SpaceInvadersInput0 {
    dip_4: bool,
    #[skip]
    __: B3,
    fire: bool,
    left: bool,
    right: bool,
    #[skip]
    __: B1,
}

#[bitfield]
struct SpaceInvadersInput1 {
    credit: bool,
    start_2p: bool,
    start_1p: bool,
    #[skip(setters)]
    always_one: bool,
    p1_shot: bool,
    p1_left: bool,
    p1_right: bool,
    #[skip]
    __: B1,
}

#[bitfield]
struct SpaceInvadersInput2 {
    #[skip(setters)]
    dip_3: bool,
    #[skip(setters)]
    dip_5: bool,
    tilt: bool,
    #[skip(setters)]
    dip_6: bool,
    p2_shot: bool,
    p2_left: bool,
    p2_right: bool,
    #[skip(setters)]
    dip_7: bool,
}

#[bitfield]
#[derive(Debug)]
#[allow(dead_code)]
struct SpaceInvadersAudioOutput1 {
    #[skip(setters)]
    ufo: bool,
    #[skip(setters)]
    shot: bool,
    #[skip(setters)]
    flash: bool,
    #[skip(setters)]
    invader_die: bool,
    #[skip(setters)]
    extended_play: bool,
    #[skip(setters)]
    _amp_enable: bool,
    #[skip]
    __: B2,
}

#[bitfield]
#[derive(Debug)]
#[allow(dead_code)]
struct SpaceInvadersAudioOutput2 {
    #[skip(setters)]
    fleet_movement_1: bool,
    #[skip(setters)]
    fleet_movement_2: bool,
    #[skip(setters)]
    fleet_movement_3: bool,
    #[skip(setters)]
    fleet_movement_4: bool,
    #[skip(setters)]
    ufo_hit: bool,
    #[skip]
    __: B3,
}

struct ShiftRegister {
    register: u16,
    amount: u8,
}

impl ShiftRegister {
    fn new() -> Self {
        ShiftRegister {
            register: 0,
            amount: 0,
        }
    }

    fn input_data(&mut self, input: u8) {
        self.register = ((input as u16) << 8) | (self.register >> 8);
    }

    fn input_amount(&mut self, amount: u8) {
        self.amount = amount & 0b00000111;
    }

    fn output(&self) -> u8 {
        (self.register >> (8 - self.amount)) as u8
    }
}

fn load_rom(file_path: &Path) -> Result<[u8; ROM_SIZE], std::io::Error> {
    let mut file = match File::open(&file_path) {
        Ok(file) => file,
        Err(e) => return Err(e),
    };

    let mut buffer = [0 as u8; ROM_SIZE];
    match file.read(&mut buffer) {
        Ok(_) => {}
        Err(e) => return Err(e),
    }
    return Ok(buffer);
}

fn emulator_loop(
    memory: &mut SpaceInvadersMemory,
    vram_mirror: Arc<Mutex<Vec<u8>>>,
    running: Arc<AtomicBool>,
    inputs: Arc<(AtomicU8, AtomicU8, AtomicU8)>,
    window: Arc<Window>,
) {
    let (ufo_sound, mut ufo_sound_controller) =
        awedio::sounds::open_file("src/assets/ufo_lowpitch.wav")
            .expect("Could not find ufo_lowpitch.wav")
            .loop_from_memory()
            .unwrap()
            .pausable()
            .controllable();
    ufo_sound_controller.set_paused(true);

    let shot_sound = awedio::sounds::open_file("src/assets/shoot.wav")
        .expect("Could not find shoot.wav")
        .into_memory_sound()
        .unwrap();

    let flash_sound = awedio::sounds::open_file("src/assets/ufo_highpitch.wav")
        .expect("Could not find ufo_highpitch.wav")
        .into_memory_sound()
        .unwrap();

    let invader_die_sound = awedio::sounds::open_file("src/assets/invaderkilled.wav")
        .expect("Could not find invaderkilled.wav")
        .into_memory_sound()
        .unwrap();

    let fleet_movement_1_sound = awedio::sounds::open_file("src/assets/fastinvader1.wav")
        .expect("Could not find fastinvader1.wav")
        .into_memory_sound()
        .unwrap();

    let fleet_movement_2_sound = awedio::sounds::open_file("src/assets/fastinvader2.wav")
        .expect("Could not find fastinvader2.wav")
        .into_memory_sound()
        .unwrap();

    let fleet_movement_3_sound = awedio::sounds::open_file("src/assets/fastinvader3.wav")
        .expect("Could not find fastinvader3.wav")
        .into_memory_sound()
        .unwrap();

    let fleet_movement_4_sound = awedio::sounds::open_file("src/assets/fastinvader4.wav")
        .expect("Could not find fastinvader4.wav")
        .into_memory_sound()
        .unwrap();

    let ufo_hit_sound = awedio::sounds::open_file("src/assets/explosion.wav")
        .expect("Could not find explosion.wav")
        .loop_from_memory()
        .unwrap();

    let (mut audio_manager, _audio_backend) = match awedio::start() {
        Ok(val) => val,
        Err(e) => {
            error!("Error starting audio backend: {}", e);
            running.store(false, Ordering::Relaxed);
            return;
        }
    };

    let mut cpu = Intel8080::new();
    let mut shift_register = ShiftRegister::new();

    let mut next_display_time: u64 = 0;
    let mut next_screen_int_time: u64 = 7_142_857;
    let mut emu_clock: u64 = 0;

    let mut last_audio1 = SpaceInvadersAudioOutput1::new();
    let mut last_audio2 = SpaceInvadersAudioOutput2::new();

    audio_manager.play(Box::new(ufo_sound));

    // run main loop
    while running.load(Ordering::Relaxed) {
        let mut total_cpu_cycles = 0;
        let now = std::time::Instant::now();

        for _ in 0..5 {
            let cpu_cycles = cpu.step(memory);
            total_cpu_cycles += cpu_cycles;

            if cpu.output_ready() {
                let output = cpu.read_output();
                match cpu.active_io_port() {
                    2 => shift_register.input_amount(output), // shift amount
                    3 => {
                        let audio1 = SpaceInvadersAudioOutput1::from_bytes([cpu.read_output()]);

                        if audio1.ufo() && !last_audio1.ufo() {
                            ufo_sound_controller.set_paused(false);
                        } else if !audio1.ufo() && last_audio1.ufo() {
                            ufo_sound_controller.set_paused(true);
                        }

                        if audio1.shot() && !last_audio1.shot() {
                            audio_manager.play(Box::new(shot_sound.clone()));
                        }

                        if audio1.flash() && !last_audio1.flash() {
                            audio_manager.play(Box::new(flash_sound.clone()));
                        }

                        if audio1.invader_die() && !last_audio1.invader_die() {
                            audio_manager.play(Box::new(invader_die_sound.clone()));
                        }

                        last_audio1 = audio1;
                    }
                    4 => shift_register.input_data(output), // shift data
                    5 => {
                        let audio2 = SpaceInvadersAudioOutput2::from_bytes([cpu.read_output()]);

                        if audio2.fleet_movement_1() && !last_audio2.fleet_movement_1() {
                            audio_manager.play(Box::new(fleet_movement_1_sound.clone()));
                        }

                        if audio2.fleet_movement_2() && !last_audio2.fleet_movement_2() {
                            audio_manager.play(Box::new(fleet_movement_2_sound.clone()));
                        }

                        if audio2.fleet_movement_3() && !last_audio2.fleet_movement_3() {
                            audio_manager.play(Box::new(fleet_movement_3_sound.clone()));
                        }

                        if audio2.fleet_movement_4() && !last_audio2.fleet_movement_4() {
                            audio_manager.play(Box::new(fleet_movement_4_sound.clone()));
                        }

                        if audio2.ufo_hit() && !last_audio2.ufo_hit() {
                            audio_manager.play(Box::new(ufo_hit_sound.clone()));
                        }

                        last_audio2 = audio2;
                    }
                    6 => { /* do nothing */ } // watch dog
                    _ => {}
                }
            } else if cpu.awaiting_input() {
                let input: u8 = match cpu.active_io_port() {
                    0 => inputs.0.load(Ordering::Relaxed), // INPUTS 0
                    1 => inputs.1.load(Ordering::Relaxed), // INPUTS 1
                    2 => inputs.2.load(Ordering::Relaxed), // INPUTS 2
                    3 => shift_register.output(),          // bit shift in
                    _ => 0,
                };

                cpu.write_input(input);
            }

            // draw screen + set interrupts if needed

            let emu_time_nano_sec: u64 = cpu_cycles * CYCLE_TIME_NANO_SECS;

            emu_clock = emu_clock.wrapping_add(emu_time_nano_sec);

            if next_display_time <= emu_clock {
                next_display_time = next_display_time.wrapping_add(DISPLAY_TIME_NANO_SEC);
                cpu.interrupt(emu8080::Instruction::RST_3);

                if let Ok(ref mut vram_mirror) = vram_mirror.try_lock() {
                    vram_mirror.copy_from_slice(memory.vram.as_slice());
                    window.request_redraw();
                }
            } else if next_screen_int_time <= emu_clock {
                next_screen_int_time = next_screen_int_time.wrapping_add(DISPLAY_TIME_NANO_SEC);
                cpu.interrupt(emu8080::Instruction::RST_2);
            }
        }

        let exec_time = now.elapsed();
        let emu_time_nano_sec: u64 = total_cpu_cycles * CYCLE_TIME_NANO_SECS;
        let emu_time = std::time::Duration::from_nanos(emu_time_nano_sec);

        if emu_time > exec_time {
            std::thread::sleep(emu_time - exec_time);
        } else {
            warn!(
                "Failed to meet cycle time!
                   Emulator: {emu_time:?}, Execution: {exec_time:?}"
            );
        }
    }
}

struct SpaceInvaders<'a> {
    memory: Option<SpaceInvadersMemory>,
    vram_mirror: Arc<Mutex<Vec<u8>>>,
    running: Arc<AtomicBool>,
    inputs: Arc<(AtomicU8, AtomicU8, AtomicU8)>,
    window: Option<Arc<Window>>,
    rendered_pixels: Option<Pixels<'a>>,
    emulator_thread: Option<std::thread::JoinHandle<()>>,
}

impl<'a> SpaceInvaders<'a> {
    fn new(memory: SpaceInvadersMemory) -> Self {
        let inputs = Arc::new((
            AtomicU8::new(0b1000_1111),
            AtomicU8::new(0b0000_1000),
            AtomicU8::new(0b0000_0000),
        ));
        let running = Arc::new(AtomicBool::new(false));
        let vram_mirror = Arc::new(Mutex::new(vec![0u8; DISPLAY_BUFFER_SIZE]));

        Self {
            memory: Some(memory),
            vram_mirror,
            running,
            inputs,
            emulator_thread: None,
            rendered_pixels: None,
            window: None,
        }
    }
}

impl winit::application::ApplicationHandler for SpaceInvaders<'_> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.running.store(true, Ordering::Relaxed);

        if self.window.is_none() {
            let window_size = winit::dpi::LogicalSize::new(
                SCREEN_WIDTH_PIXELS as f64,
                SCREEN_HEIGHT_PIXELS as f64,
            );
            let mut window_attributes = winit::window::WindowAttributes::default();
            window_attributes.blur = false;
            window_attributes.inner_size = Some(winit::dpi::Size::Logical(window_size));
            window_attributes.title = "Space Invaders".to_string();

            let window = Arc::new(event_loop.create_window(window_attributes).unwrap());
            self.window = Some(window.clone());
            let surface_texture = SurfaceTexture::new(
                window_size.width as u32,
                window_size.height as u32,
                window.clone(),
            );
            self.rendered_pixels = Some(
                Pixels::new(
                    DISPLAY_WIDTH_PIXELS as u32,
                    DISPLAY_HEIGHT_PIXELS as u32,
                    surface_texture,
                )
                .unwrap(),
            );

            let inputs_emu = self.inputs.clone();
            let running_emu = self.running.clone();
            let vram_mirror_emu = self.vram_mirror.clone();
            let window_emu = window.clone();
            let mut memory = self.memory.take().unwrap();
            self.emulator_thread = Some(std::thread::spawn(move || {
                emulator_loop(
                    &mut memory,
                    vram_mirror_emu,
                    running_emu,
                    inputs_emu,
                    window_emu,
                )
            }));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(ref mut rendered_pixels) = self.rendered_pixels {
                    if let Ok(vram_mirror) = self.vram_mirror.lock() {
                        rendered_pixels
                            .frame_mut()
                            .copy_from_slice(vram_mirror.as_slice());
                        rendered_pixels.render().unwrap();
                    }
                }
            }
            WindowEvent::CloseRequested => {
                self.running
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                if let Some(thread) = self.emulator_thread.take() {
                    if let Err(e) = thread.join() {
                        warn!("Error joining thread: {:?}", e);
                    }
                }
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                debug!("{:?} key pressed", key);
                match key.as_ref() {
                    Key::Named(NamedKey::ArrowRight) => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_p1_right(true);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_p1_left(true);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_p1_shot(true);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("c") => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_credit(true);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("1") => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_start_1p(true);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("2") => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_start_2p(true);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("w") => {
                        let mut val = SpaceInvadersInput2::from_bytes([self
                            .inputs
                            .2
                            .load(Ordering::Relaxed)]);

                        val.set_p2_shot(true);

                        self.inputs.2.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("a") => {
                        let mut val = SpaceInvadersInput2::from_bytes([self
                            .inputs
                            .2
                            .load(Ordering::Relaxed)]);

                        val.set_p2_left(true);

                        self.inputs.2.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("d") => {
                        let mut val = SpaceInvadersInput2::from_bytes([self
                            .inputs
                            .2
                            .load(Ordering::Relaxed)]);

                        val.set_p2_right(true);

                        self.inputs.2.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: key,
                        state: ElementState::Released,
                        ..
                    },
                ..
            } => {
                debug!("{:?} key released", key);
                match key.as_ref() {
                    Key::Named(NamedKey::ArrowRight) => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);
                        val.set_p1_right(false);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_p1_left(false);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_p1_shot(false);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("c") => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_credit(false);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("1") => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_start_1p(false);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("2") => {
                        let mut val = SpaceInvadersInput1::from_bytes([self
                            .inputs
                            .1
                            .load(Ordering::Relaxed)]);

                        val.set_start_2p(false);

                        self.inputs.1.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("w") => {
                        let mut val = SpaceInvadersInput2::from_bytes([self
                            .inputs
                            .2
                            .load(Ordering::Relaxed)]);

                        val.set_p2_shot(false);

                        self.inputs.2.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("a") => {
                        let mut val = SpaceInvadersInput2::from_bytes([self
                            .inputs
                            .2
                            .load(Ordering::Relaxed)]);

                        val.set_p2_left(false);

                        self.inputs.2.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    Key::Character("d") => {
                        let mut val = SpaceInvadersInput2::from_bytes([self
                            .inputs
                            .2
                            .load(Ordering::Relaxed)]);

                        val.set_p2_right(false);

                        self.inputs.2.store(val.into_bytes()[0], Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let rom = match load_rom(Path::new("src/assets/invaders.bin")) {
        Ok(rom) => rom,
        Err(e) => return Err(Box::new(e)),
    };

    let memory = SpaceInvadersMemory::new(rom);

    let mut space_invaders = SpaceInvaders::new(memory);

    let event_loop = EventLoop::builder().with_wayland().build()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut space_invaders)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ShiftRegister;

    #[test]
    fn test_shift_register() {
        let mut sr = ShiftRegister::new();
        assert_eq!(sr.amount, 0);
        assert_eq!(sr.register, 0);

        sr.input_data(0xAA);
        assert_eq!(sr.register, 0xAA00);

        sr.input_data(0xFF); // 0b11111111
        assert_eq!(sr.register, 0xFFAA);

        sr.input_data(0x12); // 0b00010010
        assert_eq!(sr.register, 0x12FF);

        sr.input_amount(0);
        assert_eq!(sr.output(), 0x12);

        sr.input_amount(2);
        assert_eq!(sr.output(), 0b01001011);

        sr.input_amount(7);
        assert_eq!(sr.output(), 0b01111111);
    }
}
