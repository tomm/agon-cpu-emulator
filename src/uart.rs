pub type SendFn = Box<dyn Fn(u8)>;
pub type RecvFn = Box<dyn Fn() -> Option<u8>>;

pub struct Uart {
    send_fn: Option<SendFn>,
    recv_fn: Option<RecvFn>,
    rx_buf: Option<u8>,

    // interrupt enable register
    pub ier: u8,
    // FIFO control register
    pub fctl: u8,
    // Line control register
    pub lctl: u8, // lctl & 0x80 enables access to baud rate generator
                  // registers where rbr/thr and ier are normally accessed
    pub brg_div: u16,
    // scratch pad register
    pub spr: u8,
}

impl Uart {
    pub fn new(send_fn: Option<SendFn>, recv_fn: Option<RecvFn>) -> Self {
        Uart {
            send_fn, recv_fn,
            ier: 0, fctl: 0, lctl: 0, brg_div: 2, spr: 0, rx_buf: None
        }
    }

    pub fn maybe_fill_rx_buf(&mut self) -> Option<u8> {
        if let Some(ref rx) = self.recv_fn {
            if self.rx_buf == None {
                self.rx_buf = rx();
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
        if let Some(ref tx) = self.send_fn {
            tx(value);
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
