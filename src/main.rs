use std::path::Path;
use std::fs::File;
use std::io::prelude::*;

use std::time;
use std::thread::sleep;

use emu8080::Intel8080;
use emu8080::MemoryAccess;
use emu8080::CYCLE_TIME_NANO_SECS;

#[allow(non_camel_case_types)]

const SCREEN_REFRESH_RATE_HZ: u64 = 60;
const SCREEN_WIDTH_PIXELS: u64 = 256;
const SCREEN_HEIGHT_PIXELS: u64 = 224;

enum InputPorts {
    INP0 = 0,
    INP1 = 1,
    INP2 = 2,
    SHFT_IN = 3
}

enum OutputPorts {
    SHFTAMNT = 2,
    SOUND1 = 3,
    SHFT_DATA = 4,
    SOUND2 = 5,
    WATCHDOG = 6
}

const ROM_SIZE: usize = 0x2000;
const RAM_SIZE: usize = 0x400;
const VRAM_SIZE: usize = 0x1C00;

const ROM_START: usize = 0;
const RAM_START: usize = 0x2000;
const VRAM_START: usize = 0x2400;

const ROM_END: usize = ROM_START + ROM_SIZE;
const RAM_END: usize = RAM_START + RAM_SIZE;
const VRAM_END: usize = VRAM_START + VRAM_SIZE;

struct SpaceInvadersMemory {
    rom: [u8; ROM_SIZE],
    ram: [u8; RAM_SIZE],
    vram: [u8; VRAM_SIZE]
}

impl MemoryAccess for SpaceInvadersMemory {
    fn read_byte(&self, addr: u16) -> u8 {
        let addr: usize = addr as usize;

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
        let addr: usize = addr as usize; 
        if RAM_START <= addr && addr < RAM_END {
            self.ram[addr - 0x2000] = val;
        }
        else if VRAM_START <= addr && addr < VRAM_END {
            self.vram[addr - 0x2400] = val;
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
        let ret = self.register & (0xFF << (8 - self.amount));
        let ret = ret >> self.amount + 8;
        ret as u8
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

fn main() -> Result<(), std::io::Error> {
    println!( "{:#02x}", 0xFF << 7 );
    
    let mut cpu = Intel8080::new();
    let rom = match load_rom(Path::new("test")) {
        Ok(rom) => rom,
        Err(e) => return Err(e)
    };

    let mut memory = SpaceInvadersMemory::new(rom);
    let mut shift_register = ShiftRegister::new();
    
    let mut time: u64 = 0;

    let mut input_1: u8 = 0b00001000;
    let mut input_2: u8 = 0b00000000;

    loop {
        let now = std::time::Instant::now();
        let cpu_cycles = cpu.step(&mut memory);

        if cpu.output_ready() {
            let output = cpu.read_output();
            match cpu.active_io_port() {
                2 => { shift_register.input_amount(output) }, // shift amount
                3 => {}, // sound bits
                4 => { shift_register.input_data(output) }, // shift data
                5 => {}, // sound bits
                6 => {}, // watch dog
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

        std::thread::sleep(emu_time - exec_time);
    }

    // return Ok(());
}
