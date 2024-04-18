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
    let mut cpu = Intel8080::new();
    let rom = match load_rom(Path::new("test")) {
        Ok(rom) => rom,
        Err(e) => return Err(e)
    };
    let mut memory = SpaceInvadersMemory::new(rom);
    let mut time: u64 = 0;

    loop {
        let now = std::time::Instant::now();
        let cpu_cycles = cpu.step(&mut memory);
        
        let cpu_time_nano_sec: u64 = cpu_cycles * CYCLE_TIME_NANO_SECS;
        let cpu_time = std::time::Duration::from_nanos(cpu_time_nano_sec);
        let exec_time = now.elapsed();
        
        std::thread::sleep(cpu_time - exec_time);
    }

    // return Ok(());
}
