/// Interface for a debugger
///
use ez80::Environment;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use crate::AgonMachine;

pub struct DebuggerConnection {
    pub tx: Sender<DebugResp>,
    pub rx: Receiver<DebugCmd>
}

#[derive(Debug)]
pub enum DebugCmd {
    Ping,
    Pause,
    Continue,
    SetBreak(u32),
    DeleteBreak(u32),
    ListBreaks,
    Step,
    GetMemory { start: u32, len: u32 },
    GetRegisters,
    GetState,
    DisassemblePc { adl: Option<bool> },
    Disassemble { adl: Option<bool>, start: u32, end: u32 },
}

#[derive(Debug)]
pub enum DebugResp {
    Pong,
    HitBreakpoint, // a breakpoint encountered
    Registers(ez80::Registers),
    State {
        registers: ez80::Registers,
        instructions_executed: u64,
        stack: [u8; 16],
        pc_instruction: String,
    },
    Memory {
        start: u32,
        data: Vec<u8>
    },
    // (start, disasm, bytes)
    Disassembly {
        adl: bool,
        disasm: Vec<ez80::disassembler::Disasm>
    },
    Breakpoints(Vec<BreakPoint>),
}

#[derive(Debug,Clone)]
pub struct BreakPoint(pub u32);

pub struct DebuggerServer {
    con: DebuggerConnection,
    breaks: Vec<BreakPoint>,
}

impl DebuggerServer {
    pub fn new(con: DebuggerConnection) -> Self {
        DebuggerServer { con, breaks: vec![] }
    }

    /// Called before each instruction is executed
    pub fn tick(&mut self, machine: &mut AgonMachine, cpu: &mut ez80::Cpu) {

        if !cpu.is_halted() {
            for b in &self.breaks {
                if b.0 == cpu.state.pc() {
                    cpu.state.halted = true;
                    self.con.tx.send(DebugResp::HitBreakpoint).unwrap();
                    self.send_state(machine, cpu);
                    break;
                }
            }
        }

        loop {
            match self.con.rx.try_recv() {
                Ok(cmd) => {
                    //println!("agon-cpu-emulator received {:?}", cmd);
                    match cmd {
                        DebugCmd::ListBreaks => {
                            self.con.tx.send(DebugResp::Breakpoints(self.breaks.clone())).unwrap();
                        }
                        DebugCmd::DisassemblePc { adl } => {
                            let start = cpu.state.pc();
                            let end = start + 0x20;
                            self.send_disassembly(machine, cpu, adl, start, end);
                        }
                        DebugCmd::Disassemble { adl, start, mut end } => {
                            if end <= start { end = start + 0x20 };
                            self.send_disassembly(machine, cpu, adl, start, end);
                        }
                        DebugCmd::Step => {
                            cpu.state.halted = false;
                            machine.execute_instruction(cpu);
                            cpu.state.halted = true;
                            self.send_state(machine, cpu);
                        }
                        DebugCmd::Pause => {
                            cpu.state.halted = true;
                            self.con.tx.send(DebugResp::Pong).unwrap();
                        }
                        DebugCmd::Continue => {
                            cpu.state.halted = false;
                            // force one instruction to be executed, just to
                            // get over any breakpoint on the current PC
                            machine.execute_instruction(cpu);
                            self.con.tx.send(DebugResp::Pong).unwrap();
                        }
                        DebugCmd::SetBreak(addr) => {
                            self.breaks.push(BreakPoint(addr));
                            self.con.tx.send(DebugResp::Pong).unwrap();
                        }
                        DebugCmd::DeleteBreak(addr) => {
                            self.breaks.retain(|b| b.0 != addr);
                            self.con.tx.send(DebugResp::Pong).unwrap();
                        }
                        DebugCmd::Ping => self.con.tx.send(DebugResp::Pong).unwrap(),
                        DebugCmd::GetRegisters => self.send_registers(cpu),
                        DebugCmd::GetState => self.send_state(machine, cpu),
                        DebugCmd::GetMemory { start, len } => {
                            self.send_mem(machine, cpu, start, len);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => return
            }
        }
    }

    fn send_disassembly(&self, machine: &mut AgonMachine, cpu: &mut ez80::Cpu, adl_override: Option<bool>, start: u32, end: u32) {
        let dis = ez80::disassembler::disassemble(machine, cpu, adl_override, start, end);

        self.con.tx.send(DebugResp::Disassembly {
            adl: cpu.state.reg.adl,
            disasm: dis
        }).unwrap();
    }

    fn send_mem(&self, machine: &mut AgonMachine, cpu: &mut ez80::Cpu, start: u32, len: u32) {
        let env = Environment::new(&mut cpu.state, machine);
        let mut data = vec![];

        for i in start..start+len {
            data.push(env.peek(i));
        }

        self.con.tx.send(DebugResp::Memory { start, data }).unwrap();
    }

    fn send_state(&self, machine: &mut AgonMachine, cpu: &mut ez80::Cpu) {
        let mut stack: [u8; 16] = [0; 16];
        let pc_instruction: String;

        let sp: u32 = if cpu.registers().adl {
            cpu.registers().get24(ez80::Reg16::SP)
        } else {
            cpu.state.reg.get16_mbase(ez80::Reg16::SP)
        };

        {
            let env = Environment::new(&mut cpu.state, machine);
            for i in 0..16 {
                stack[i] = env.peek(sp + i as u32);
            }

            // iz80 (which ez80 is based on) doesn't allow disassembling
            // without advancing the PC, so we hack around this
            let pc = cpu.state.pc();
            pc_instruction = cpu.disasm_instruction(machine);
            cpu.state.set_pc(pc);
        }

        self.con.tx.send(DebugResp::State {
            registers: cpu.registers().clone(),
            instructions_executed: cpu.state.instructions_executed,
            stack,
            pc_instruction,
        }).unwrap();
    }

    fn send_registers(&self, cpu: &mut ez80::Cpu) {
        self.con.tx.send(DebugResp::Registers(cpu.registers().clone())).unwrap();
    }
}
