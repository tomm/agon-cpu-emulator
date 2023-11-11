use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;

pub struct Uart {
    chan: Option<(Sender<u8>, Receiver<u8>)>,
    rx_buf: Option<u8>,

    // interrupt enable register
    pub ier: u8,
    // FIFO control register
    pub fctl: u8,
    // Line control register
    pub lctl: u8, // lctl & 0x80 enables access to baud rate generator
                  // registers where rbr/thr and ier are normally accessed
    pub brg_div: u16,
}

impl Uart {
    pub fn new(chan: Option<(Sender<u8>, Receiver<u8>)>) -> Self {
        Uart {
            ier: 0, fctl: 0, lctl: 0, brg_div: 2, chan, rx_buf: None
        }
    }

    pub fn maybe_fill_rx_buf(&mut self) -> Option<u8> {
        if let Some((ref _tx, ref rx)) = self.chan {
            if self.rx_buf == None {
                self.rx_buf = match rx.try_recv() {
                    Ok(data) => Some(data),
                    Err(mpsc::TryRecvError::Disconnected) => panic!(),
                    Err(mpsc::TryRecvError::Empty) => None
                }
            }
            self.rx_buf
        } else {
            None
        }
    }

    pub fn receive_byte(&mut self) -> u8 {
        // uart0 receive
        self.maybe_fill_rx_buf();

        let maybe_data = self.rx_buf;
        self.rx_buf = None;

        match maybe_data {
            Some(data) => data,
            None => 0
        }
    }

    pub fn send_byte(&mut self, value: u8) {
        if let Some((ref tx, ref _rx)) = self.chan {
            tx.send(value).unwrap();
        }
    }

    /*
    pub fn get_baud_rate(&self) -> u32 {
        18_432_000 / (self.brg_div as u32 * 16)
    }
    */

    pub fn is_access_brg_registers(&self) -> bool {
        self.lctl & 0x80 != 0
    }

    pub fn is_rx_interrupt_enabled(&self) -> bool {
        self.ier & 1 != 0
    }
}
