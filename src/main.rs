use emu8080::Intel8080;
use emu8080::MemoryAccess;

#[allow(non_camel_case_types)]

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
    fn new() -> Self {
        return SpaceInvadersMemory { 
            rom: [0 as u8; ROM_SIZE],
            ram: [0 as u8; RAM_SIZE],
            vram: [0 as u8; VRAM_SIZE]
        }
    }
}

fn main() {
    let cpu = Intel8080::new();
    let memory = SpaceInvadersMemory::new();
}
