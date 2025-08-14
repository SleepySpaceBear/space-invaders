use std::path::Path;
use std::fs::File;
use std::io::prelude::*;
use std::error::Error;

use log::{info, debug, warn};

use pixels::{SurfaceTexture, Pixels};

use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use winit_input_helper::WinitInputHelper;

use emu8080::Intel8080;
use emu8080::MemoryAccess;
use emu8080::CYCLE_TIME_NANO_SECS;

#[allow(non_camel_case_types)]

const SCREEN_REFRESH_RATE_HZ: usize = 60;
const SCREEN_WIDTH_PIXELS: usize = 256;
const SCREEN_HEIGHT_PIXELS: usize = 224;

const SCREEN_SIZE_PIXELS: usize = SCREEN_WIDTH_PIXELS * SCREEN_HEIGHT_PIXELS; 
const FRAME_BUFFER_SIZE: usize = SCREEN_SIZE_PIXELS * 4;

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
    vram: [u8; VRAM_SIZE]
}

impl MemoryAccess for SpaceInvadersMemory {
    fn read_byte(&self, addr: u16) -> u8 {
        let addr: usize = addr as usize & RAM_MASK;

        if addr < ROM_END {
            return self.rom[addr];
        }
        else if addr < RAM_END {
            return self.ram[addr - RAM_START];
        }
        else if addr < VRAM_END {
            return self.vram[addr - VRAM_START];
        }

        return 0;
    }

    fn write_byte(&mut self, addr: u16, val: u8) {
        let addr: usize = addr as usize & RAM_MASK; 

        if RAM_START <= addr && addr < RAM_END {
            self.ram[addr - RAM_START] = val;
        }
        else if VRAM_START <= addr && addr < VRAM_END {
            self.vram[addr - VRAM_START] = val;
        }

    }

    fn read_bytes(&self, addr: u16, count: u16) -> &[u8] {
        let addr: usize = addr as usize & RAM_MASK;
    
        if addr < ROM_END {
            let start = addr;
            let end = start + count as usize;
            return &self.rom[start..end]
        }
        else if RAM_START <= addr && addr < RAM_END {
            let start = addr - RAM_START;
            let end = start + count as usize;
            return &self.ram[start..end]

        }
        else if VRAM_START <= addr && VRAM_END <= addr {
            let start = addr - VRAM_START;
            let end = start + count as usize;
            return &self.vram[start..end];
        }

        return &[]
    }

    fn write_bytes(&mut self, addr: u16, val: &[u8]) {
        let addr: usize = addr as usize & RAM_MASK;
        if addr < ROM_END {
            let start = addr;
            let end = start + val.len();
            self.rom[start..end].copy_from_slice(val);
        }
        else if RAM_START <= addr && addr < RAM_END {
            let start = addr - RAM_START;
            let end = start + val.len();
            self.ram[start..end].copy_from_slice(val);

        }
        else if VRAM_START <= addr && VRAM_END <= addr {
            let start = addr as usize;
            let end = start + val.len() as usize;
            self.vram[start..end].copy_from_slice(val);
        }
    }
}

impl SpaceInvadersMemory {
    fn new(rom: [u8; ROM_SIZE]) -> Self {
        let ret = SpaceInvadersMemory { 
            rom,
            ram: [0 as u8; RAM_SIZE],
            vram: [0 as u8; VRAM_SIZE]
        };

        return ret;
    }
}

struct ShiftRegister {
    register: u16,
    amount: u8,
}

impl ShiftRegister {
    fn new() -> Self {
        ShiftRegister {
            register: 0,
            amount: 0
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
        Err(e) => return Err(e)
    };
    
    let mut buffer = [0 as u8; ROM_SIZE];
    match file.read(&mut buffer) {
        Ok(_) => { },
        Err(e) => return Err(e),
    }
    return Ok(buffer);
}

fn convert_framebuffer(vram: &[u8; VRAM_SIZE], frame_buffer: &mut [u8]) {
    let width = SCREEN_HEIGHT_PIXELS;
    let height = SCREEN_WIDTH_PIXELS;
    
    let pixel_size = 4;
    let mut byte_dst: usize = (height - 1) * width * pixel_size;
    let mut pixel_index: usize = 0;

    for val in vram.iter() {
        let mut mask: u8 = 0x01;

        for _ in 0..8 {
            let color: u8 = if mask & val != 0 {0xFF} else {0x00};
            
            for i in 0..pixel_size {
                frame_buffer[byte_dst + i] = color;
            }

            pixel_index += 1;
            if pixel_index % height == 0 {
                byte_dst = (((height - 1) * width) + (pixel_index / height)) * pixel_size;
            }
            else {
                byte_dst -= width * pixel_size;
            }
            mask <<= 1;
        }
    
    }
}


fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let mut cpu = Intel8080::new();
    let rom = match load_rom(Path::new("src/assets/invaders.bin")) {
        Ok(rom) => rom,
        Err(e) => return Err(Box::new(e))
    };

    let mut memory = SpaceInvadersMemory::new(rom);
    let mut shift_register = ShiftRegister::new();
    
    let mut emu_clock: u64 = 0;
    let mut next_display_time: u64 = 0;
    let mut next_screen_int_time: u64 = 7_142_857;

    let mut input_1: u8 = 0b00001000;
    let mut input_2: u8 = 0b00000000;

    let event_loop = EventLoop::new();
    let mut input = WinitInputHelper::new();
    let window = {
        let size = LogicalSize::new(SCREEN_HEIGHT_PIXELS as f64, SCREEN_WIDTH_PIXELS as f64);
        WindowBuilder::new()
            .with_title("Space Invaders")
            .with_inner_size(size)
            .with_min_inner_size(size)
            .build(&event_loop)
            .unwrap()
    };

    let mut rendered_pixels = {
        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        Pixels::new(SCREEN_HEIGHT_PIXELS as u32, SCREEN_WIDTH_PIXELS as u32, surface_texture)?
    };
    
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        
        if let Event::RedrawRequested(_) = event {
            if let Err(_) = rendered_pixels.render() {
                *control_flow = ControlFlow::Exit;
                return;
            }
        }

        let now = std::time::Instant::now();
        let cpu_cycles = cpu.step(&mut memory);

        if cpu.output_ready() {
            let output = cpu.read_output();
            match cpu.active_io_port() {
                2 => { shift_register.input_amount(output) }, // shift amount
                3 => {}, // sound bits
                4 => { shift_register.input_data(output) }, // shift data
                5 => {}, // sound bits
                6 => { /* do nothing */ }, // watch dog
                _ => {}
            }
        }
        else if cpu.awaiting_input() {
            let input: u8 = match cpu.active_io_port() {
                1 => { input_1 }, // INPUTS 1
                2 => { input_2 }, // INPUTS 2
                3 => { shift_register.output() }, // bit shift in
                _ => { 0 }
            };

            cpu.write_input(input);
        }

        // draw screen + set interrupts if needed

        let emu_time_nano_sec: u64 = cpu_cycles * CYCLE_TIME_NANO_SECS;
        let emu_time = std::time::Duration::from_nanos(emu_time_nano_sec);
        let exec_time = now.elapsed();

        emu_clock = emu_clock.wrapping_add(emu_time_nano_sec);

        if next_display_time <= emu_clock {
            next_display_time = next_display_time.wrapping_add(DISPLAY_TIME_NANO_SEC);
            cpu.interrupt(emu8080::Instruction::RST_3);
            
            convert_framebuffer(&memory.vram, &mut rendered_pixels.frame_mut());
            window.request_redraw();
        }

        if next_screen_int_time <= emu_clock {
            next_screen_int_time = next_screen_int_time.wrapping_add(DISPLAY_TIME_NANO_SEC);
            cpu.interrupt(emu8080::Instruction::RST_2);
        }

        if emu_time > exec_time {
             std::thread::sleep(emu_time - exec_time);
        }
        else {
            warn!("Failed to meet cycle time - Emulator: {emu_time:?}, Execution: {exec_time:?}");
        }
    });
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
