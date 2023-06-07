use ez80::Machine;

// FatFS struct FIL
pub const SIZEOF_MOS_FIL_STRUCT: u32 = 36;
pub const FIL_MEMBER_OBJSIZE: u32 = 11;
pub const FIL_MEMBER_FPTR: u32 = 17;
// FatFS struct FILINFO
pub const SIZEOF_MOS_FILINFO_STRUCT: u32 = 278;
pub const FILINFO_MEMBER_FSIZE_U32: u32 = 0;
//pub const FILINFO_MEMBER_FDATE_U16: u32 = 4;
//pub const FILINFO_MEMBER_FTIME_U16: u32 = 6;
pub const FILINFO_MEMBER_FATTRIB_U8: u32 = 8;
//pub const FILINFO_MEMBER_ALTNAME_13BYTES: u32 = 9;
pub const FILINFO_MEMBER_FNAME_256BYTES: u32 = 22;
// f_open mode (3rd arg)
//pub const FA_READ: u32 = 1;
pub const FA_WRITE: u32 = 2;
pub const FA_CREATE_NEW: u32 = 4;
pub const FA_CREATE_ALWAYS: u32 = 8;

pub struct MosMap {
    pub f_chdir: u32,
    pub _f_chdrive: u32,
    pub f_close: u32,
    pub f_closedir: u32,
    pub _f_getcwd: u32,
    pub _f_getfree: u32,
    pub f_getlabel: u32,
    pub f_gets: u32,
    pub f_lseek: u32,
    pub f_mkdir: u32,
    pub f_mount: u32,
    pub f_open: u32,
    pub f_opendir: u32,
    pub _f_printf: u32,
    pub f_putc: u32,
    pub _f_puts: u32,
    pub f_read: u32,
    pub f_readdir: u32,
    pub f_rename: u32,
    pub _f_setlabel: u32,
    pub f_stat: u32,
    pub _f_sync: u32,
    pub _f_truncate: u32,
    pub f_unlink: u32,
    pub f_write: u32,
}

pub static MOS_103_MAP: MosMap = MosMap {
    f_chdir    : 0x82B2,
    _f_chdrive : 0x827A,
    f_close    : 0x822B,
    f_closedir : 0x8B5B,
    _f_getcwd  : 0x8371,
    _f_getfree : 0x8CE8,
    f_getlabel : 0x9816,
    f_gets     : 0x9C91,
    f_lseek    : 0x8610,
    f_mkdir    : 0x92F6,
    f_mount    : 0x72F7,
    f_open     : 0x738C,
    f_opendir  : 0x8A52,
    _f_printf  : 0x9F11,
    f_putc     : 0x9E8E,
    _f_puts    : 0x9EC4,
    f_read     : 0x785E,
    f_readdir  : 0x8B92,
    f_rename   : 0x9561,
    _f_setlabel: 0x99DB,
    f_stat     : 0x8C55,
    _f_sync    : 0x8115,
    _f_truncate: 0x8F78,
    f_unlink   : 0x911A,
    f_write    : 0x7C10,
};

/**
 * Like z80_mem_tools::get_cstring, except \r and \n are accepted as
 * string terminators as well as \0
 */
pub fn get_mos_path_string<M: Machine>(machine: &M, address: u32) -> Vec<u8> {
    let mut s: Vec<u8> = vec![];
    let mut ptr = address;

    loop {
        match machine.peek(ptr) {
            0 | 10 | 13 => break,
            b => s.push(b)
        }
        ptr += 1;
    }
    s
}
