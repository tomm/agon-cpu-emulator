/// Interface for a debugger
///
use ez80::{ Environment, Machine };
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use crate::AgonMachine;

pub struct DebuggerConnection {
    pub tx: Sender<DebugResp>,
    pub rx: Receiver<DebugCmd>
}

#[derive(Debug, Clone)]
pub enum DebugCmd {
    Ping,
    Pause,
    Continue,
    Step,
    StepOver,
    Message(String),
    AddTrigger(Trigger),
    DeleteTrigger(u32),
    ListTriggers,
    GetMemory { start: u32, len: u32 },
    GetRegisters,
    GetState,
    DisassemblePc { adl: Option<bool> },
    Disassemble { adl: Option<bool>, start: u32, end: u32 },
}

#[derive(Debug)]
pub enum DebugResp {
    IsPaused(bool),
    Pong,
    Message(String),
    TriggerRan(String),
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
    Triggers(Vec<Trigger>),
}

#[derive(Debug,Clone)]
pub struct Trigger {
    pub address: u32,
    pub once: bool,
    pub msg: String,
    pub actions: Vec<DebugCmd>
}

pub struct DebuggerServer {
    con: DebuggerConnection,
    triggers: Vec<Trigger>,
}

impl DebuggerServer {
    pub fn new(con: DebuggerConnection) -> Self {
        DebuggerServer { con, triggers: vec![] }
    }

    /// Called before each instruction is executed
    pub fn tick(&mut self, machine: &mut AgonMachine, cpu: &mut ez80::Cpu) {
        let pc = cpu.state.pc();

        // check triggers
        if !cpu.is_halted() {
            for b in &self.triggers {
                if b.address == pc {
                    cpu.state.halted = true;
                    self.con.tx.send(DebugResp::TriggerRan(b.msg.clone())).unwrap();
                    self.send_state(machine, cpu);
                    break;
                }
            }
        }

        // delete triggers that are only to execute once
        self.triggers.retain(|b| !(b.address == pc && b.once));

        loop {
            match self.con.rx.try_recv() {
                Ok(cmd) => self.handle_debug_cmd(cmd, machine, cpu),
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => return
            }
        }
    }

    fn handle_debug_cmd(&mut self, cmd: DebugCmd, machine: &mut AgonMachine, cpu: &mut ez80::Cpu) {
        let pc = cpu.state.pc();

        match cmd {
            DebugCmd::Message(s) => {
                self.con.tx.send(DebugResp::Message(s)).unwrap()
            }
            DebugCmd::ListTriggers => {
                self.con.tx.send(DebugResp::Triggers(self.triggers.clone())).unwrap();
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
            DebugCmd::StepOver => {
                // if the opcode at PC is a call, set a 'once' breakpoint on the
                // instruction after it
                let prefix_override = match machine.peek(pc) {
                    0x52 | 0x5b => Some(true),
                    0x40 | 0x49 => Some(false),
                    _ => None
                };
                match machine.peek(if prefix_override.is_some() { pc+1 } else { pc }) {
                    // CALL instruction at (pc)
                    0xc4 | 0xd4 | 0xe4 | 0xf4 | 0xcc | 0xcd | 0xdc | 0xec | 0xfc => {
                        let addr_next = pc + match prefix_override {
                            Some(true) => 5, // opcode + prefix byte + 3 byte immediate
                            Some(false) => 4,
                            _ => if cpu.state.reg.adl { 4 } else { 3 }
                        };
                        self.triggers.push(Trigger {
                            address: addr_next,
                            once: true,
                            msg: "Stepped over CALL".to_string(),
                            actions: vec![]
                        });
                        cpu.state.halted = false;
                        self.con.tx.send(DebugResp::IsPaused(false)).unwrap();
                    }
                    // other instructions. just step
                    _ => {
                        cpu.state.halted = false;
                        machine.execute_instruction(cpu);
                        cpu.state.halted = true;
                        self.send_state(machine, cpu);
                    }
                }

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
                self.con.tx.send(DebugResp::IsPaused(false)).unwrap();
            }
            DebugCmd::AddTrigger(t) => {
                self.triggers.push(t);
                self.con.tx.send(DebugResp::Pong).unwrap();
            }
            DebugCmd::DeleteTrigger(addr) => {
                self.triggers.retain(|b| b.address != addr);
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
