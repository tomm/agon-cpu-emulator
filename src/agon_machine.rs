use ez80::*;
use crate::mos;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::collections::HashMap;
use std::io::{ Seek, SeekFrom, Read, Write };
use crate::{ debugger, prt_timer };
use rand::Rng;

const ROM_SIZE: usize = 0x40000; // 256 KiB (actually 128KiB of flash, and 128KiB of nothing)
const RAM_SIZE: usize = 0x80000; // 512 KiB
const MEM_SIZE: usize = ROM_SIZE + RAM_SIZE;
const ONCHIP_RAM_SIZE: u32 = 0x2000; // 8KiB

pub enum RamInit {
    Zero,
    Random
}

pub struct AgonMachine {
    mem: [u8; MEM_SIZE],
    onchip_mem: [u8; ONCHIP_RAM_SIZE as usize],  // 8K SRAM on the EZ80F92
    tx: Sender<u8>,
    rx: Receiver<u8>,
    rx_buf: Option<u8>,
    // map from MOS fatfs FIL struct ptr to rust File handle
    open_files: HashMap<u32, std::fs::File>,
    open_dirs: HashMap<u32, std::fs::ReadDir>,
    enable_hostfs: bool,
    mos_map: mos::MosMap,
    hostfs_root_dir: std::path::PathBuf,
    mos_current_dir: MosPath,
    vsync_counter: std::sync::Arc<std::sync::atomic::AtomicU32>,
    last_vsync_count: u32,
    clockspeed_hz: u64,
    prt_timers: [prt_timer::PrtTimer; 6],
    ram_init: RamInit,
    onchip_mem_enable: bool,
    onchip_mem_segment: u8,
    mos_bin: std::path::PathBuf,
    // last_pc and mem_out_of_bounds are used by the debugger
    pub last_pc: u32,
    pub mem_out_of_bounds: std::cell::Cell<Option<u32>>, // address
    pub paused: bool,
    pub cycle_counter: std::cell::Cell<u32>,
    transmit_interrupt: bool,
    lcr_dlab: bool
}

// a path relative to the hostfs_root_dir
pub struct MosPath(std::path::PathBuf);

impl Machine for AgonMachine {
    #[inline]
    fn use_cycles(&self, cycles: u32) {
        self.cycle_counter.set(self.cycle_counter.get() + cycles);
    }

    fn peek(&self, address: u32) -> u8 {
        self.use_cycles(if address < 0x40000 { 2 } else { 1 });

        if let Some(onchip_ram_offset) = self.offset_in_onchip_ram(address) {
            self.onchip_mem[onchip_ram_offset as usize]
        } else if self.is_address_mapped(address) {
            self.mem[address as usize]
        } else {
            self.mem_out_of_bounds.set(Some(address));
            0xf5
        }
    }

    fn poke(&mut self, address: u32, value: u8) {
        self.use_cycles(1);

        if let Some(onchip_ram_offset) = self.offset_in_onchip_ram(address) {
            self.onchip_mem[onchip_ram_offset as usize] = value;
        } else if self.is_address_mapped(address) {
            self.mem[address as usize] = value;
        } else {
            self.mem_out_of_bounds.set(Some(address));
        }
    }

    fn port_in(&mut self, address: u16) -> u8 {
        self.use_cycles(1);
        match address {
            0x80 => self.prt_timers[0].read_ctl(),
            0x81 => self.prt_timers[0].read_counter_low(),
            0x82 => self.prt_timers[0].read_counter_high(),

            0x83 => self.prt_timers[1].read_ctl(),
            0x84 => self.prt_timers[1].read_counter_low(),
            0x85 => self.prt_timers[1].read_counter_high(),

            0x86 => self.prt_timers[2].read_ctl(),
            0x87 => self.prt_timers[2].read_counter_low(),
            0x88 => self.prt_timers[2].read_counter_high(),

            0x89 => self.prt_timers[3].read_ctl(),
            0x8a => self.prt_timers[3].read_counter_low(),
            0x8b => self.prt_timers[3].read_counter_high(),

            0x8c => self.prt_timers[4].read_ctl(),
            0x8d => self.prt_timers[4].read_counter_low(),
            0x8e => self.prt_timers[4].read_counter_high(),

            0x8f => self.prt_timers[5].read_ctl(),
            0x90 => self.prt_timers[5].read_counter_low(),
            0x91 => self.prt_timers[5].read_counter_high(),

            // GPIO PB_DR
            0x9a => 0x0,
            0xa2 => {
                0x0 // UART0 clear to send
            }

            0xb4 => {
                if self.onchip_mem_enable { 0x80 } else { 0 }
            }

            0xb5 => {
                self.onchip_mem_segment
            }

            0xc0 => {
                if self.uart0.is_access_brg_registers() {
                    self.uart0.brg_div as u8
                } else {
                    self.uart0.receive_byte()
                }
            }
            0xc1 => {
                if self.uart0.is_access_brg_registers() {
                    (self.uart0.brg_div >> 8) as u8
                } else {
                    self.uart0.ier
                }
            }
            0xc3 => self.uart0.lctl,
            0xc2 => {
                // UART0_IIR
                if self.transmit_interrupt == true {    
                    self.transmit_interrupt=false;
                    0x02   
                }
                else {    
                    0x04    
                }
            }
            0xc5 => {
                // UART0_LSR
                self.maybe_fill_rx_buf();

                // indicate if we have characters received from VDP via DR (data ready)
                // we are always ready to transmit (TEMP and THRE)
                match self.rx_buf {
                    Some(_) => 0b01100001, //0x41, DR (data ready) + TEMT (transmit shift empty) + THRE (transmit holding empty)
                    None => 0b01100000 // 0x40, DR (data not ready) + TEMT (transmit shift empty) + THRE (transmit holding empty)
                }
                // UART_LSR_ETX		EQU 	%40 ; Transmit empty (can send)
                // UART_LSR_RDY		EQU	%01		; Data ready (can receive)
            }
            0xc6 => {
                // UART0_MSR
                0b00010000 // UART0 clear to send
            }
            /* uart1 is kindof useless in the emulator, but ... */
            0xd0 => {
                if self.uart1.is_access_brg_registers() {
                    self.uart1.brg_div as u8
                } else {
                    self.uart1.receive_byte()
                }
            }
            0xd1 => {
                if self.uart1.is_access_brg_registers() {
                    (self.uart1.brg_div >> 8) as u8
                } else {
                    self.uart1.ier
                }
            }
            0xd3 => self.uart1.lctl,
            0xd5 => {
                match self.uart1.maybe_fill_rx_buf() {
                    Some(_) => 0b01100001, //0x41, DR (data ready) + TEMT (transmit shift empty) + THRE (transmit holding empty)
                    None => 0b01100000 // 0x40, DR (data not ready) + TEMT (transmit shift empty) + THRE (transmit holding empty)
                }
            }
            0xd6 => {
                // UART0_MSR
                0b00010000 // UART0 clear to send
            }
            _ => {
                //println!("IN({:02X})", address);
                0
            }
        }
    }
    fn port_out(&mut self, address: u16, value: u8) {
        self.use_cycles(1);
        match address {
            0x80 => self.prt_timers[0].write_ctl(value),
            0x81 => self.prt_timers[0].write_reload_low(value),
            0x82 => self.prt_timers[0].write_reload_high(value),

            0x83 => self.prt_timers[1].write_ctl(value),
            0x84 => self.prt_timers[1].write_reload_low(value),
            0x85 => self.prt_timers[1].write_reload_high(value),

            0x86 => self.prt_timers[2].write_ctl(value),
            0x87 => self.prt_timers[2].write_reload_low(value),
            0x88 => self.prt_timers[2].write_reload_high(value),

            0x89 => self.prt_timers[3].write_ctl(value),
            0x8a => self.prt_timers[3].write_reload_low(value),
            0x8b => self.prt_timers[3].write_reload_high(value),

            0x8c => self.prt_timers[4].write_ctl(value),
            0x8d => self.prt_timers[4].write_reload_low(value),
            0x8e => self.prt_timers[4].write_reload_high(value),

            0x8f => self.prt_timers[5].write_ctl(value),
            0x90 => self.prt_timers[5].write_reload_low(value),
            0x91 => self.prt_timers[5].write_reload_high(value),

            // GPIO PB_DR
            0x9a => {}

            0xb4 => {
                self.onchip_mem_enable = value & 0x80 != 0;
            }

            0xb5 => {
                self.onchip_mem_segment = value;
            }
            0xc3 => {
                self.lcr_dlab = value & 0x80 != 0;
            }
            0xc1 => {
                // UART0_IER
                if value & 0x02 != 0 {
                    // request transmit interrupt
                    self.transmit_interrupt = true;
                }
            }
            /* UART0_REG_THR - send data to VDP */
            0xc0 => {
                if self.uart0.is_access_brg_registers() {
                    self.uart0.brg_div &= 0xff00;
                    self.uart0.brg_div |= value as u16;
                } else {
                    self.uart0.send_byte(value);
                }
            }
            0xc1 => {
                if self.uart0.is_access_brg_registers() {
                    self.uart0.brg_div &= 0xff;
                    self.uart0.brg_div |= (value as u16) << 8;
                } else {
                    //println!("setting uart ier: 0x{:x}", value);
                    self.uart0.ier = value;
                }
            }
            0xc2 => self.uart0.fctl = value,
            0xc3 => self.uart0.lctl = value,

            /* uart1 is kindof useless in the emulator, but ... */
            0xd0 => {
                if self.uart1.is_access_brg_registers() {
                    self.uart1.brg_div &= 0xff00;
                    self.uart1.brg_div |= value as u16;
                } else {
                    self.uart1.send_byte(value);
                }
            }
            0xd1 => {
                if self.uart1.is_access_brg_registers() {
                    self.uart1.brg_div &= 0xff;
                    self.uart1.brg_div |= (value as u16) << 8;
                } else {
                    //println!("setting uart ier: 0x{:x}", value);
                    self.uart1.ier = value;
                }
            }
            0xd2 => self.uart1.fctl = value,
            0xd3 => self.uart1.lctl = value,

            _ => {
                //println!("OUT(${:02X}) = ${:x}", address, value);
            }
        }
    }
}

pub struct AgonMachineConfig {
    pub to_vdp: Sender<u8>,
    pub from_vdp: Receiver<u8>,
    pub vsync_counter: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub clockspeed_hz: u64,
    pub ram_init: RamInit,
    pub mos_bin: std::path::PathBuf,
}

impl AgonMachine {
    pub fn new(config: AgonMachineConfig) -> Self {
        AgonMachine {
            mem: [0; MEM_SIZE],
            onchip_mem: [0; ONCHIP_RAM_SIZE as usize],
            tx: config.to_vdp,
            rx: config.from_vdp,
            rx_buf: None,
            open_files: HashMap::new(),
            open_dirs: HashMap::new(),
            enable_hostfs: true,
            mos_map: mos::MosMap::default(),
            hostfs_root_dir: std::env::current_dir().unwrap(),
            mos_current_dir: MosPath(std::path::PathBuf::new()),
            vsync_counter: config.vsync_counter,
            last_vsync_count: 0,
            clockspeed_hz: config.clockspeed_hz,
            prt_timers: [
                prt_timer::PrtTimer::new(),
                prt_timer::PrtTimer::new(),
                prt_timer::PrtTimer::new(),
                prt_timer::PrtTimer::new(),
                prt_timer::PrtTimer::new(),
                prt_timer::PrtTimer::new(),
            ],
            ram_init: config.ram_init,
            last_pc: 0,
            mem_out_of_bounds: std::cell::Cell::new(None),
            cycle_counter: std::cell::Cell::new(0),
            paused: false,
            mos_bin: config.mos_bin,
            onchip_mem_enable: true,
            onchip_mem_segment: 0xff,
            transmit_interrupt: false,
            lcr_dlab: false
        }
    }

    #[inline]
    fn is_address_mapped(&self, address: u32) -> bool {
        // TODO: not hardcoded, change to values of CS0_xxx registers and 
        address < 0x20000 || (address >= 0x40000 && address < 0xc0000)
    }

    #[inline]
    fn offset_in_onchip_ram(&self, address: u32) -> Option<u32> {
        if !self.onchip_mem_enable { return None };
        let offset = address.wrapping_sub(((self.onchip_mem_segment as u32) << 16) + 0xe000);
        if offset < ONCHIP_RAM_SIZE { Some(offset) } else { None }
    }

    pub fn set_sdcard_directory(&mut self, path: std::path::PathBuf) {
        self.hostfs_root_dir = path;
    }

    fn maybe_fill_rx_buf(&mut self) -> Option<u8> {
        if self.rx_buf == None {
            self.rx_buf = match self.rx.try_recv() {
                Ok(data) => Some(data),
                Err(mpsc::TryRecvError::Disconnected) => panic!(),
                Err(mpsc::TryRecvError::Empty) => None
            }
        }
        self.rx_buf
    }

    fn load_mos(&mut self) {
        let code = match std::fs::read(&self.mos_bin) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error opening {}: {:?}", self.mos_bin.display(), e);
                std::process::exit(-1);
            }
        };
        
        for (i, e) in code.iter().enumerate() {
            self.mem[i] = *e;
        }

        let mos_map = self.mos_bin.with_extension("map");

        match crate::symbol_map::read_zds_map_file(mos_map.to_str().unwrap()) {
            Ok(map) => {
                match mos::MosMap::from_symbol_map(map) {
                    Ok(mos_map) => {
                        self.mos_map = mos_map;
                    }
                    Err(e) => {
                        println!("Error reading {}. hostfs integration disabled: {}", mos_map.display(), e);
                        self.enable_hostfs = false;
                    }
                }
            }
            Err(e) => {
                println!("Error reading {}. hostfs integration disabled: {}", mos_map.display(), e);
                self.enable_hostfs = false;
            }
        }
    }

    fn hostfs_mos_f_getlabel(&mut self, cpu: &mut Cpu) {
        let mut buf = self._peek24(cpu.state.sp() + 6);
        let label = "hostfs";
        for b in label.bytes() {
            self.poke(buf, b);
            buf += 1;
        }
        self.poke(buf, 0);

        // success
        cpu.state.reg.set24(Reg16::HL, 0); // success

        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn hostfs_mos_f_close(&mut self, cpu: &mut Cpu) {
        let fptr = self._peek24(cpu.state.sp() + 3);
        //eprintln!("f_close(${:x})", fptr);

        // closes on Drop
        self.open_files.remove(&fptr);

        // success
        cpu.state.reg.set24(Reg16::HL, 0);

        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_gets(&mut self, cpu: &mut Cpu) {
        let mut buf = self._peek24(cpu.state.sp() + 3);
        let max_len = self._peek24(cpu.state.sp() + 6);
        let fptr = self._peek24(cpu.state.sp() + 9);
        //eprintln!("f_gets(buf: ${:x}, len: ${:x}, fptr: ${:x})", buf, max_len, fptr);

        let fresult = 'outer: {
            if let Some(mut f) = self.open_files.get(&fptr) {
                let mut line = vec![];
                let mut host_buf = vec![0; 1];
                for _ in 0..max_len {
                    let n_read = match f.read(host_buf.as_mut_slice()) {
                        Ok(n_read) => n_read,
                        Err(_) => break 'outer mos::FR_DISK_ERR
                    };

                    if n_read == 0 {
                        break 'outer mos::FR_DISK_ERR // EOF
                    }
                    line.push(host_buf[0]);

                    if host_buf[0] == 10 || host_buf[0] == 0 { break; }
                }
                // no f.tell()...
                let fpos = match f.seek(SeekFrom::Current(0)) {
                    Ok(n) => n,
                    Err(_) => break 'outer mos::FR_DISK_ERR
                };

                // save file position to FIL.fptr U32
                self._poke24(fptr + mos::FIL_MEMBER_FPTR, fpos as u32);
                for b in line {
                    self.poke(buf, b);
                    buf += 1;
                }
                self.poke(buf, 0);

                mos::FR_OK
            } else {
                mos::FR_DISK_ERR
            }
        };
        //cpu.state.reg.set24(Reg16::HL, fresult);
        if fresult==mos::FR_OK {
            cpu.state.reg.set24(Reg16::HL, buf);
        }
        else {
            cpu.state.reg.set24(Reg16::HL, 0);
        }
        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn hostfs_mos_f_putc(&mut self, cpu: &mut Cpu) {
        let ch = self._peek24(cpu.state.sp() + 3);
        let fptr = self._peek24(cpu.state.sp() + 6);

        let fresult = 'outer: {
            if let Some(mut f) = self.open_files.get(&fptr) {
                if f.write(&[ch as u8]).is_err() {
                    break 'outer mos::FR_DISK_ERR;
                }

                // no f.tell()...
                let fpos = match f.seek(SeekFrom::Current(0)) {
                    Ok(n) => n,
                    Err(_) => break 'outer mos::FR_DISK_ERR
                };
                // save file position to FIL.fptr
                self._poke24(fptr + mos::FIL_MEMBER_FPTR, fpos as u32);

                mos::FR_OK
            } else {
                mos::FR_DISK_ERR
            }
        };

        cpu.state.reg.set24(Reg16::HL, fresult);
        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn hostfs_mos_f_write(&mut self, cpu: &mut Cpu) {
        let fptr = self._peek24(cpu.state.sp() + 3);
        let buf = self._peek24(cpu.state.sp() + 6);
        let num = self._peek24(cpu.state.sp() + 9);
        let num_written_ptr = self._peek24(cpu.state.sp() + 12);
        //eprintln!("f_write(${:x}, ${:x}, {}, ${:x})", fptr, buf, num, num_written_ptr);

        let fresult = 'outer: {
            if let Some(mut f) = self.open_files.get(&fptr) {
                for i in 0..num {
                    let byte = self.peek(buf + i);
                    if f.write(&[byte]).is_err() { break 'outer mos::FR_DISK_ERR }
                }

                // no f.tell()...
                let fpos = match f.seek(SeekFrom::Current(0)) {
                    Ok(n) => n,
                    Err(_) => break 'outer mos::FR_DISK_ERR
                };
                // save file position to FIL.fptr
                self._poke24(fptr + mos::FIL_MEMBER_FPTR, fpos as u32);

                // inform caller that all bytes were written
                self._poke24(num_written_ptr, num);

                mos::FR_OK
            } else {
                mos::FR_DISK_ERR
            }
        };

        cpu.state.reg.set24(Reg16::HL, fresult);
        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn hostfs_mos_f_read(&mut self, cpu: &mut Cpu) {
        let fptr = self._peek24(cpu.state.sp() + 3);
        let mut buf = self._peek24(cpu.state.sp() + 6);
        let len = self._peek24(cpu.state.sp() + 9);
        let bytes_read_ptr = self._peek24(cpu.state.sp() + 12);
        //eprintln!("f_read(${:x}, ${:x}, ${:x}, ${:x})", fptr, buf, len, bytes_read_ptr);

        let fresult = 'outer: {
            if let Some(mut f) = self.open_files.get(&fptr) {
                let mut host_buf: Vec<u8> = vec![0; len as usize];
                if let Ok(num_bytes_read) = f.read(host_buf.as_mut_slice()) {
                    // no f.tell()...
                    let fpos = match f.seek(SeekFrom::Current(0)) {
                        Ok(fpos) => fpos,
                        Err(_) => break 'outer mos::FR_DISK_ERR
                    };
                    // copy to agon ram 
                    for b in host_buf {
                        self.poke(buf, b);
                        buf += 1;
                    }
                    // save file position to FIL.fptr
                    self._poke24(fptr + mos::FIL_MEMBER_FPTR, fpos as u32);
                    // save num bytes read
                    self._poke24(bytes_read_ptr, num_bytes_read as u32);

                    mos::FR_OK
                } else {
                    mos::FR_DISK_ERR
                }
            } else {
                mos::FR_DISK_ERR
            }
        };

        cpu.state.reg.set24(Reg16::HL, fresult);
        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn hostfs_mos_f_closedir(&mut self, cpu: &mut Cpu) {
        let dir_ptr = self._peek24(cpu.state.sp() + 3);
        // closes on Drop
        self.open_dirs.remove(&dir_ptr);

        // success
        cpu.state.reg.set24(Reg16::HL, 0); // success

        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn hostfs_set_filinfo_from_metadata(&mut self, z80_filinfo_ptr: u32, path: &std::path::PathBuf, metadata: &std::fs::Metadata) {
        // XXX to_str can fail if not utf-8
        // write file name
        z80_mem_tools::memcpy_to_z80(
            self, z80_filinfo_ptr + mos::FILINFO_MEMBER_FNAME_256BYTES,
            path.file_name().unwrap().to_str().unwrap().as_bytes()
        );

        // write file length (U32)
        self._poke24(z80_filinfo_ptr + mos::FILINFO_MEMBER_FSIZE_U32, metadata.len() as u32);
        self.poke(z80_filinfo_ptr + mos::FILINFO_MEMBER_FSIZE_U32 + 3, (metadata.len() >> 24) as u8);

        // is directory?
        if metadata.is_dir() {
            self.poke(z80_filinfo_ptr + mos::FILINFO_MEMBER_FATTRIB_U8, 0x10 /* AM_DIR */);
        }

        // TODO set fdate, ftime
    }

    fn hostfs_mos_f_readdir(&mut self, cpu: &mut Cpu) {
        let dir_ptr = self._peek24(cpu.state.sp() + 3);
        let file_info_ptr = self._peek24(cpu.state.sp() + 6);

        // clear the FILINFO struct
        z80_mem_tools::memset(self, file_info_ptr, 0, mos::SIZEOF_MOS_FILINFO_STRUCT);

        match self.open_dirs.get_mut(&dir_ptr) {
            Some(dir) => {

                match dir.next() {
                    Some(Ok(dir_entry)) => {
                        let path = dir_entry.path();
                        if let Ok(metadata) = std::fs::metadata(&path) {
                            self.hostfs_set_filinfo_from_metadata(file_info_ptr, &path, &metadata);

                            // success
                            cpu.state.reg.set24(Reg16::HL, 0);
                        } else {
                            // hm. why might std::fs::metadata fail?
                            z80_mem_tools::memcpy_to_z80(
                                self, file_info_ptr + mos::FILINFO_MEMBER_FNAME_256BYTES,
                                "<error reading file metadata>".as_bytes()
                            );
                            cpu.state.reg.set24(Reg16::HL, 0);
                        }
                    }
                    Some(Err(_)) => {
                        cpu.state.reg.set24(Reg16::HL, 1); // error
                    }
                    None => {
                        // directory has been read to the end.
                        // do nothing, since FILINFO.fname[0] == 0 indicates to MOS that
                        // the directory end has been reached

                        // success
                        cpu.state.reg.set24(Reg16::HL, 0);
                    }
                }
            }
            None => {
                cpu.state.reg.set24(Reg16::HL, 1); // error
            }
        }

        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_mkdir(&mut self, cpu: &mut Cpu) {
        let dir_name = unsafe {
            String::from_utf8_unchecked(mos::get_mos_path_string(self, self._peek24(cpu.state.sp() + 3)))
        };
        //eprintln!("f_mkdir(\"{}\")", dir_name);

        match std::fs::create_dir(self.host_path_from_mos_path_join(&dir_name)) {
            Ok(_) => {
                // success
                cpu.state.reg.set24(Reg16::HL, 0);
            }
            Err(_) => {
                // error
                cpu.state.reg.set24(Reg16::HL, 1);
            }
        }
        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_rename(&mut self, cpu: &mut Cpu) {
        let old_name = unsafe {
            String::from_utf8_unchecked(mos::get_mos_path_string(self, self._peek24(cpu.state.sp() + 3)))
        };
        let new_name = unsafe {
            String::from_utf8_unchecked(mos::get_mos_path_string(self, self._peek24(cpu.state.sp() + 6)))
        };
        //eprintln!("f_rename(\"{}\", \"{}\")", old_name, new_name);

        match std::fs::rename(self.host_path_from_mos_path_join(&old_name),
                              self.host_path_from_mos_path_join(&new_name)) {
            Ok(_) => {
                // success
                cpu.state.reg.set24(Reg16::HL, 0);
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        cpu.state.reg.set24(Reg16::HL, 4);
                    }
                    _ => {
                        cpu.state.reg.set24(Reg16::HL, 1);
                    }
                }
            }
        }
        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_chdir(&mut self, cpu: &mut Cpu) {
        let cd_to_ptr = self._peek24(cpu.state.sp() + 3);
        let cd_to = unsafe {
            // MOS filenames may not be valid utf-8
            String::from_utf8_unchecked(mos::get_mos_path_string(self, cd_to_ptr))
        };
        //eprintln!("f_chdir({})", cd_to);

        let new_path = self.mos_path_join(&cd_to);

        match std::fs::metadata(self.mos_path_to_host_path(&new_path)) {
            Ok(metadata) => {
                if metadata.is_dir() {
                    //eprintln!("setting path to {:?}", &new_path);
                    self.mos_current_dir = new_path;
                    cpu.state.reg.set24(Reg16::HL, 0);
                } else {
                    cpu.state.reg.set24(Reg16::HL, 1);
                }
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        cpu.state.reg.set24(Reg16::HL, 4);
                    }
                    _ => {
                        cpu.state.reg.set24(Reg16::HL, 1);
                    }
                }
            }
        }
        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_mount(&mut self, cpu: &mut Cpu) {
        // always success. hostfs is mounted
        cpu.state.reg.set24(Reg16::HL, 0); // ok
        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_unlink(&mut self, cpu: &mut Cpu) {
        let filename_ptr = self._peek24(cpu.state.sp() + 3);
        let filename = unsafe {
            String::from_utf8_unchecked(mos::get_mos_path_string(self, filename_ptr))
        };
        //eprintln!("f_unlink(\"{}\")", filename);

        match std::fs::remove_file(self.host_path_from_mos_path_join(&filename)) {
            Ok(()) => {
                cpu.state.reg.set24(Reg16::HL, 0); // ok
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        cpu.state.reg.set24(Reg16::HL, 4);
                    }
                    _ => {
                        cpu.state.reg.set24(Reg16::HL, 1);
                    }
                }
            }
        };

        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_opendir(&mut self, cpu: &mut Cpu) {
        //fr = f_opendir(&dir, path);
        let dir_ptr = self._peek24(cpu.state.sp() + 3);
        let path_ptr = self._peek24(cpu.state.sp() + 6);
        let path = unsafe {
            // MOS filenames may not be valid utf-8
            String::from_utf8_unchecked(mos::get_mos_path_string(self, path_ptr))
        };
        //eprintln!("f_opendir(${:x}, \"{}\")", dir_ptr, path.trim_end());

        match std::fs::read_dir(self.host_path_from_mos_path_join(&path)) {
            Ok(dir) => {
                // XXX should clear the DIR struct in z80 ram
                
                // store in map of z80 DIR ptr to rust ReadDir
                self.open_dirs.insert(dir_ptr, dir);
                cpu.state.reg.set24(Reg16::HL, 0); // ok
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        cpu.state.reg.set24(Reg16::HL, 4);
                    }
                    _ => {
                        cpu.state.reg.set24(Reg16::HL, 1);
                    }
                }
            }
        }

        cpu.state.reg.set24(Reg16::HL, 0); // ok
        let mut env = Environment::new(&mut cpu.state, self);
        env.subroutine_return();
    }

    fn mos_path_to_host_path(&mut self, path: &MosPath) -> std::path::PathBuf {
        self.hostfs_root_dir.join(&path.0)
    }

    /**
     * Return a new MosPath, `new_fragments` joined to mos_current_dir
     */
    fn mos_path_join(&mut self, new_fragments: &str) -> MosPath {
        let mut full_path = match new_fragments.get(0..1) {
            Some("/") | Some("\\") => std::path::PathBuf::new(),
            _ => self.mos_current_dir.0.clone()
        };

        for fragment in new_fragments.split(|c| c == '/' || c == '\\') {
            match fragment {
                "" | "." => {}
                ".." => {
                    full_path.pop();
                }
                "/" => {
                    full_path = std::path::PathBuf::new();
                }
                f => {
                    let abs_path = self.hostfs_root_dir.join(&full_path);
                    
                    // look for a case-insensitive match for this path fragment
                    let matched_f = match std::fs::read_dir(abs_path) {
                        Ok(dir) => {
                            if let Some(ci_f) = dir.into_iter().find(|item| {
                                match item {
                                    Ok(dir_entry) => dir_entry.file_name().to_ascii_lowercase().into_string() == Ok(f.to_ascii_lowercase()),
                                    Err(_) => false
                                }
                            }) {
                                // found a case-insensitive match
                                ci_f.unwrap().file_name()
                            } else {
                                std::ffi::OsString::from(f)
                            }
                        }
                        Err(_) => {
                            std::ffi::OsString::from(f)
                        }
                    };

                    full_path.push(matched_f);
                }
            }
        }

        MosPath(full_path)
    }

    fn host_path_from_mos_path_join(&mut self, new_fragments: &str) -> std::path::PathBuf {
        let rel_path = self.mos_path_join(new_fragments);
        self.mos_path_to_host_path(&rel_path)
    }

    fn hostfs_mos_f_lseek(&mut self, cpu: &mut Cpu) {
        let fptr = self._peek24(cpu.state.sp() + 3);
        let offset = self._peek24(cpu.state.sp() + 6);

        //eprintln!("f_lseek(${:x}, {})", fptr, offset);

        match self.open_files.get(&fptr) {
            Some(mut f) => {
                match f.seek(SeekFrom::Start(offset as u64)) {
                    Ok(pos) => {
                        // save file position to FIL.fptr
                        self._poke24(fptr + mos::FIL_MEMBER_FPTR, pos as u32);
                        // success
                        cpu.state.reg.set24(Reg16::HL, 0);
                    }
                    Err(_) => {
                        cpu.state.reg.set24(Reg16::HL, 1); // error
                    }
                }
            }
            None => {
                cpu.state.reg.set24(Reg16::HL, 1); // error
            }
        }
        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_stat(&mut self, cpu: &mut Cpu) {
        let path_str = {
            let ptr = self._peek24(cpu.state.sp() + 3);
            unsafe {
                String::from_utf8_unchecked(mos::get_mos_path_string(self, ptr))
            }
        };
        let filinfo_ptr = self._peek24(cpu.state.sp() + 6);
        let path = self.host_path_from_mos_path_join(&path_str);
        //eprintln!("f_stat(\"{}\", ${:x})", path_str, filinfo_ptr);

        match std::fs::metadata(&path) {
            Ok(metadata) => {
                // clear the FILINFO struct
                z80_mem_tools::memset(self, filinfo_ptr, 0, mos::SIZEOF_MOS_FILINFO_STRUCT);

                self.hostfs_set_filinfo_from_metadata(filinfo_ptr, &path, &metadata);

                // success
                cpu.state.reg.set24(Reg16::HL, 0);
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        cpu.state.reg.set24(Reg16::HL, 4);
                    }
                    _ => {
                        cpu.state.reg.set24(Reg16::HL, 1);
                    }
                }
            }
        }

        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    fn hostfs_mos_f_open(&mut self, cpu: &mut Cpu) {
        let fptr = self._peek24(cpu.state.sp() + 3);
        let filename = {
            let ptr = self._peek24(cpu.state.sp() + 6);
            // MOS filenames may not be valid utf-8
            unsafe {
                String::from_utf8_unchecked(mos::get_mos_path_string(self, ptr))
            }
        };
        let path = self.mos_path_join(&filename);
        let mode = self._peek24(cpu.state.sp() + 9);
        //eprintln!("f_open(${:x}, \"{}\", {})", fptr, &filename, mode);
        match std::fs::File::options()
            .read(true)
            .write(mode & mos::FA_WRITE != 0)
            .create((mode & mos::FA_CREATE_NEW != 0) || (mode & mos::FA_CREATE_ALWAYS != 0))
            .truncate(mode & mos::FA_CREATE_ALWAYS != 0)
            .open(self.mos_path_to_host_path(&path)) {
            Ok(mut f) => {
                // wipe the FIL structure
                z80_mem_tools::memset(self, fptr, 0, mos::SIZEOF_MOS_FIL_STRUCT);

                // save the size in the FIL structure
                let mut file_len = f.seek(SeekFrom::End(0)).unwrap();
                f.seek(SeekFrom::Start(0)).unwrap();

                // XXX don't support files larger than 512KiB
                file_len = file_len.min(1<<19);

                // store file len in fatfs FIL structure
                self._poke24(fptr + mos::FIL_MEMBER_OBJSIZE, file_len as u32);
                
                // store mapping from MOS *FIL to rust File
                self.open_files.insert(fptr, f);

                cpu.state.reg.set24(Reg16::HL, 0); // ok
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => cpu.state.reg.set24(Reg16::HL, 4),
                    _ => cpu.state.reg.set24(Reg16::HL, 1)
                }
            }

        }
        Environment::new(&mut cpu.state, self).subroutine_return();
    }

    #[inline]
    // returns cycles elapsed
    pub fn execute_instruction(&mut self, cpu: &mut Cpu) -> u32 {
        let pc = cpu.state.pc();
        // remember PC before instruction executes. the debugger uses this
        // when out-of-bounds memory accesses happen (since they can't be
        // trapped mid-execution)
        self.last_pc = pc;

        if self.enable_hostfs && pc < 0x40000 {
            if pc == self.mos_map.f_close { self.hostfs_mos_f_close(cpu); }
            if pc == self.mos_map.f_gets { self.hostfs_mos_f_gets(cpu); }
            if pc == self.mos_map.f_read { self.hostfs_mos_f_read(cpu); }
            if pc == self.mos_map.f_open { self.hostfs_mos_f_open(cpu); }
            if pc == self.mos_map.f_write { self.hostfs_mos_f_write(cpu); }
            if pc == self.mos_map.f_chdir { self.hostfs_mos_f_chdir(cpu); }
            if pc == self.mos_map.f_closedir { self.hostfs_mos_f_closedir(cpu); }
            if pc == self.mos_map.f_getlabel { self.hostfs_mos_f_getlabel(cpu); }
            if pc == self.mos_map.f_lseek { self.hostfs_mos_f_lseek(cpu); }
            if pc == self.mos_map.f_mkdir { self.hostfs_mos_f_mkdir(cpu); }
            if pc == self.mos_map.f_mount { self.hostfs_mos_f_mount(cpu); }
            if pc == self.mos_map.f_opendir { self.hostfs_mos_f_opendir(cpu); }
            if pc == self.mos_map.f_putc { self.hostfs_mos_f_putc(cpu); }
            if pc == self.mos_map.f_readdir { self.hostfs_mos_f_readdir(cpu); }
            if pc == self.mos_map.f_rename { self.hostfs_mos_f_rename(cpu); }
            if pc == self.mos_map.f_stat { self.hostfs_mos_f_stat(cpu); }
            if pc == self.mos_map.f_unlink { self.hostfs_mos_f_unlink(cpu); }
            // never referenced in MOS
            //if pc == self.mos_map._f_puts { eprintln!("Un-trapped fatfs call: f_puts"); }
            //if pc == self.mos_map._f_setlabel { eprintln!("Un-trapped fatfs call: f_setlabel"); }
            //if pc == self.mos_map._f_chdrive { eprintln!("Un-trapped fatfs call: f_chdrive"); }
            //if pc == self.mos_map._f_getcwd { eprintln!("Un-trapped fatfs call: f_getcwd"); }
            //if pc == self.mos_map._f_getfree { eprintln!("Un-trapped fatfs call: f_getfree"); }
            //if pc == self.mos_map._f_printf { eprintln!("Un-trapped fatfs call: f_printf"); }
            //if pc == self.mos_map._f_sync { eprintln!("Un-trapped fatfs call: f_sync"); }
            //if pc == self.mos_map._f_truncate { eprintln!("Un-trapped fatfs call: f_truncate"); }
        }

        self.cycle_counter.set(0);
        cpu.execute_instruction(self);
        let cycles_elapsed = self.cycle_counter.get();
        //println!("{:2} cycles, {:?}", cycles_elapsed, ez80::disassembler::disassemble(self, cpu, None, pc, pc+1));

        // assumes instruction took 2 cycles...
        for t in &mut self.prt_timers {
            t.apply_ticks(cycles_elapsed as u16);
        }
 
        cycles_elapsed
    }

    #[inline]
    pub fn do_interrupts(&mut self, cpu: &mut Cpu) {
        if cpu.state.instructions_executed % 64 == 0 && cpu.state.reg.get_iff1() {
            // fire uart interrupt
            if self.maybe_fill_rx_buf() != None {
                let mut env = Environment::new(&mut cpu.state, self);
                env.interrupt(0x18); // uart0_handler
            }
            if self.transmit_interrupt == true {
                let mut env = Environment::new(&mut cpu.state, self);
                env.interrupt (0x18); // uart0_handler
            }
            // fire vsync interrupt
            let cur_vsync_count = self.vsync_counter.load(std::sync::atomic::Ordering::Relaxed);
            if cur_vsync_count != self.last_vsync_count {
                self.last_vsync_count = cur_vsync_count;
                let mut env = Environment::new(&mut cpu.state, self);
                env.interrupt(0x32);
            }

            for i in 0..self.prt_timers.len() {
                if self.prt_timers[i].irq_due() {
                    Environment::new(&mut cpu.state, self).interrupt(0xa + 2*(i as u32));
                }
            }
        }
    }

    #[inline]
    fn debugger_tick(&mut self, debugger: &mut Option<debugger::DebuggerServer>, cpu: &mut Cpu) {
        if let Some(ref mut ds) = debugger {
            ds.tick(self, cpu);
        }
    }

    pub fn start(&mut self, debugger_con: Option<debugger::DebuggerConnection>) {
        let mut cpu = Cpu::new_ez80();

        let mut debugger = if debugger_con.is_some() {
            self.paused = true;
            Some(debugger::DebuggerServer::new(debugger_con.unwrap()))
        } else {
            None
        };

        match self.ram_init {
            RamInit::Random => {
                for i in 0x40000..0xc0000 {
                    self.poke(i, rand::thread_rng().gen_range(0..=255));
                }

                for i in 0..ONCHIP_RAM_SIZE {
                    self.onchip_mem[i as usize] = rand::thread_rng().gen_range(0..=255);
                }
            }
            RamInit::Zero => {}
        }

        self.load_mos();

        cpu.state.set_pc(0);

        let cycles_per_ms: u64 = self.clockspeed_hz / 1000;
        let mut timeslice_start = std::time::Instant::now();
        loop {
            // in unlimited CPU mode, this inner loop never exits
            let mut cycle: u64 = 0;
            while cycle < cycles_per_ms {
                self.debugger_tick(&mut debugger, &mut cpu);
                if self.paused { break; }
                self.do_interrupts(&mut cpu);
                cycle += self.execute_instruction(&mut cpu) as u64;
            }

            while timeslice_start.elapsed() < std::time::Duration::from_millis(1) {
                std::thread::sleep(std::time::Duration::from_micros(500));
            }
            timeslice_start = timeslice_start.checked_add(
                std::time::Duration::from_millis(1)
            ).unwrap_or(std::time::Instant::now());
        }
    }
}
