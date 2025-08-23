//! Frame conversion types.

pub mod fmtr;
pub mod log;

use crate::conv::log::debug;
use colored::Colorize;
use fmtr::FrameFormatter;
use iced_x86::{
    Decoder, DecoderOptions, Instruction, InstructionInfoFactory, Mnemonic, OpAccess, OpKind,
};
use itertools::Itertools;
use lief::elf::Section;
use lief::generic::Symbol;
use memchr::memmem;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub trait FrameParser {
    fn from_input(
        &self,
        filename: &PathBuf,
        clear_line: bool,
        delay: Option<u16>,
    ) -> Vec<FrameInfo>;

    fn to_frameline_names(
        &self,
        formatter: &dyn FrameFormatter,
        name: &String,
        i: usize,
        at_origin: bool,
        clear_line: bool,
    ) -> (String, String) {
        let frameline_name = if at_origin {
            formatter.to_frameline_at_origin(name, clear_line)
        } else {
            formatter.to_frameline(name)
        };

        let tmp_name = format!(
            "{}{:08x}",
            "A".repeat(if frameline_name.len() < 9 {
                1
            } else {
                frameline_name.len() - 8
            }),
            i
        );

        (frameline_name, tmp_name)
    }

    fn prepare_frame(
        &self,
        formatter: &dyn FrameFormatter,
        fn_names: Vec<String>,
        fn_idx: &mut usize,
        delay: u16,
        clear_line: bool,
    ) -> FrameInfo {
        let mut frame_info = FrameInfo {
            tmp_names: vec![],
            tmp_to_frameline: HashMap::new(),
            first_name: String::new(),
            last_name: String::new(),
            delay,
        };
        for (i, name) in fn_names.iter().rev().enumerate() {
            let (frameline_name, tmp_name) = self.to_frameline_names(
                formatter,
                name,
                *fn_idx,
                i == fn_names.len() - 1,
                clear_line,
            );
            *fn_idx += 1;

            if i == 0 {
                frame_info.first_name = tmp_name.to_owned();
            }
            frame_info.tmp_names.push(tmp_name.to_owned());
            frame_info
                .tmp_to_frameline
                .insert(tmp_name.to_owned(), frameline_name);

            frame_info.last_name = tmp_name;
        }

        frame_info
    }
}

pub struct GifFrameParser<'a> {
    pub formatter: &'a dyn FrameFormatter,
}

pub struct CustomFrameParser<'a> {
    pub formatter: &'a dyn FrameFormatter,
    pub height: u16,
    pub width: u16,
}

impl GifFrameParser<'_> {
    fn prepare_names(&self, frame: &gif::Frame, w: u16, h: u16) -> Vec<String> {
        let rgba_chunks: Vec<_> = frame.buffer.chunks(4).map(|c| c.to_vec()).collect();
        let lines: Vec<_> = rgba_chunks
            .chunks(frame.width.into())
            .map(|c| c.to_vec())
            .collect();
        let mut lines_out: Vec<_> = vec![];
        for _ in 0..frame.top {
            lines_out.push(self.formatter.blank().repeat(w as usize));
        }
        for line in lines {
            let mut line_format = String::new();
            for _ in 0..frame.left {
                line_format += self.formatter.blank();
            }
            for rgba in line {
                line_format += self.formatter.to_framedot(Some(rgba)).as_str();
            }
            for _ in frame.left + frame.width..w {
                line_format += self.formatter.blank();
            }
            lines_out.push(line_format);
        }
        for _ in frame.top + frame.height..h {
            lines_out.push(self.formatter.blank().repeat(w as usize));
        }

        lines_out
    }
}

impl FrameParser for GifFrameParser<'_> {
    fn from_input(
        &self,
        filename: &PathBuf,
        clear_line: bool,
        delay: Option<u16>,
    ) -> Vec<FrameInfo> {
        let file = File::open(filename).unwrap();
        let mut decoder = gif::DecodeOptions::new();
        decoder.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = decoder.read_info(file).unwrap();
        let w = decoder.width();
        let h = decoder.height();
        debug!("dim {}x{}", w, h);

        let mut fn_idx: usize = 1;
        let mut frame_infos: Vec<FrameInfo> = vec![];
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            debug!(
                "frame +{}+{} {}x{} delay {}",
                frame.left, frame.top, frame.width, frame.height, frame.delay
            );

            let fn_names = self.prepare_names(&frame, w, h);
            frame_infos.push(self.prepare_frame(
                self.formatter,
                fn_names,
                &mut fn_idx,
                delay.unwrap_or(frame.delay),
                clear_line,
            ));
        }

        frame_infos
    }
}

impl FrameParser for CustomFrameParser<'_> {
    fn from_input(
        &self,
        _filename: &PathBuf,
        clear_line: bool,
        delay: Option<u16>,
    ) -> Vec<FrameInfo> {
        let mut fn_idx: usize = 1;
        let mut frame_infos: Vec<FrameInfo> = vec![];
        let mut fn_names: Vec<_> = vec![];
        for _ in 0..self.height {
            let mut line = String::new();
            for _ in 0..self.width {
                line += &*self.formatter.to_framedot(None);
            }
            fn_names.push(line);
        }
        frame_infos.push(self.prepare_frame(
            self.formatter,
            fn_names,
            &mut fn_idx,
            delay.unwrap_or(100),
            clear_line,
        ));

        frame_infos
    }
}

const COMPILER_ARGS: &[&str] = &[
    "-fdiagnostics-color=always",
    "-std=gnu99",
    "-O0",
    "-nostdlib",
    "-static",
    "-Wall",
    "-Werror",
];

/// Placeholder address for `.symtab` offsets embedded in `.data` section.
const PLACEHOLDER_SYMTAB_ADDR: u64 = 0x01020304;

/// Placeholder address for `.debug_str` offsets embedded in `.data` section.
const PLACEHOLDER_DEBUGSTR_ADDR: u64 = 0x05060708;

#[derive(Debug)]
pub struct FrameInfo {
    delay: u16,
    first_name: String,
    last_name: String,
    tmp_names: Vec<String>,
    tmp_to_frameline: HashMap<String, String>,
}

#[derive(Debug)]
pub struct SymbolInfo {
    addr: u64,
    offs: Vec<u64>,
}

#[derive(Debug)]
pub struct BinInfo {
    pub build_id_desc_offs: u64,
    pub build_id_desc: Vec<u8>,
    pub name_to_info: HashMap<String, SymbolInfo>,
    pub section_offs: HashMap<String, u64>,
    pub size: u64,
}

pub trait FrameConverter {
    /// `.data` address defined in linker script.
    fn data_section_addr(&self) -> u64 {
        0
    }

    /// `.text` address defined in linker script.
    fn text_section_addr(&self) -> u64 {
        0x401000
    }

    fn parser(&self) -> &dyn FrameParser;

    /// Convert function names to temporary names and frame lines.
    fn parse_input(
        &self,
        filename: &PathBuf,
        clear_line: bool,
        delay: Option<u16>,
    ) -> Vec<FrameInfo> {
        self.parser().from_input(filename, clear_line, delay)
    }

    /// Get C source code with nested function calls for each
    /// frame to render. Functions prototypes use the generated
    /// temporary names.
    fn prepare_src(
        &self,
        frame_infos: &Vec<FrameInfo>,
        start_tmp_name: &str,
        _has_debug_info: bool,
    ) -> String {
        let heads = frame_infos
            .iter()
            .map(|n| format!("{}();", n.first_name))
            .collect::<Vec<String>>()
            .join("\n    ");
        let calls = frame_infos
            .iter()
            .map(|n| {
                let mut o = String::new();
                for (prev, next) in n.tmp_names.iter().tuple_windows() {
                    o = format!(
                        r#"
void {}() {{
    {}();
}}
{}"#,
                        prev, next, o
                    );
                }
                format!(
                    r#"
void {}() {{
    return;
}}
{}"#,
                    n.tmp_names.last().unwrap(),
                    o
                )
            })
            .collect::<Vec<String>>()
            .join("\n");

        format!(
            r#"
{}

void {}() {{
loop:
    {}
    goto loop;
}}"#,
            calls, start_tmp_name, heads
        )
    }

    /// Compile the generated C source code, optionally including
    /// debug info sections.
    fn compile(
        &self,
        src: &str,
        compiler: &str,
        start_tmp_name: &str,
        include_debug_info: bool,
    ) -> Result<(), Box<dyn Error>> {
        let name = std::path::Path::new("a.c");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(name)?;
        file.write_all(src.as_bytes())?;
        spawn(
            Command::new(compiler).args(
                include_debug_info
                    .then_some(&["-g"])
                    .into_iter()
                    .flatten()
                    .chain(COMPILER_ARGS)
                    .chain(&[
                        "-Wl,--build-id",
                        &format!("-Wl,--entry={}", start_tmp_name),
                        name.to_str().unwrap(),
                    ]),
            ),
        )
    }

    fn parse_build_id(&self, file: &mut File, build_id: Option<Section>) -> (u64, Vec<u8>) {
        build_id.map_or((0, vec![]), |section| {
            if section.get_type() != lief::elf::section::Type::NOTE {
                panic!("Unexpected type '{:?}' for build id", section.get_type());
            }

            let mut offs = section.file_offset();
            let mut buf4 = [0; 4];
            file.seek(std::io::SeekFrom::Start(offs))
                .expect(&*format!("Can't seek to 0x{:08x}", offs));
            file.read_exact(&mut buf4).expect("Can't read bin");
            let name_len = u32::from_le_bytes(buf4);
            offs += 4;

            file.seek(std::io::SeekFrom::Start(offs))
                .expect(&*format!("Can't seek to 0x{:08x}", offs));
            file.read_exact(&mut buf4).expect("Can't read bin");
            let desc_len = u32::from_le_bytes(buf4);
            offs += 4 + 4 + name_len as u64; // Skip `type`.

            let mut desc = vec![0; desc_len as usize];
            file.seek(std::io::SeekFrom::Start(offs))
                .expect(&*format!("Can't seek to 0x{:08x}", offs));
            file.read_exact(&mut desc).expect("Can't read bin");

            (offs, desc)
        })
    }

    fn parse_debug_str(&self, debug_str: Option<Section>) -> HashMap<String, u64> {
        let mut name_to_debug_offs = HashMap::new();

        // Find offsets, assuming strings are separated by a single null byte.
        //
        // TODO: A more robust approach would be to parse
        // relocations in .debug_info and .debug_types sections
        // that refer to the .debug_str section.
        debug_str.map(|section| {
            let section_offs = section.file_offset();
            let mut prev_i = 0;
            let haystack = section.content();
            for i in memmem::find_iter(haystack, b"\x00") {
                let name = str::from_utf8(&haystack[prev_i as usize..i])
                    .unwrap()
                    .to_string();
                name_to_debug_offs.insert(name, section_offs + prev_i);
                prev_i = i as u64 + 1;
            }
        });

        name_to_debug_offs
    }

    fn parse_bin(&self, file: &str) -> BinInfo {
        let mut name_to_info = HashMap::new();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(file)
            .expect("Can't open output file");
        match lief::Binary::from(&mut file) {
            Some(lief::Binary::ELF(elf)) => {
                let section_offs = [".data", ".strtab", ".text"]
                    .iter()
                    .map(|name| {
                        (
                            String::from(name.to_owned()),
                            elf.section_by_name(name)
                                .map_or(0, |section| section.file_offset()),
                        )
                    })
                    .collect();

                let symtab = elf.section_by_name(".symtab").unwrap();
                let symtab_content = symtab.content();

                let strtab = elf.section_by_name(".strtab").unwrap();
                let strtab_offs = strtab.file_offset();

                let (build_id_desc_offs, build_id_desc) =
                    self.parse_build_id(&mut file, elf.section_by_name(".note.gnu.build-id"));

                let name_to_debug_offs = self.parse_debug_str(elf.section_by_name(".debug_str"));

                for (i, sym) in elf.symtab_symbols().enumerate() {
                    if sym.get_type() != lief::elf::symbol::Type::FUNC {
                        continue;
                    }

                    // Symbol name file offset is not provided,
                    // we have to parse it manually from
                    // relative offset in `.symtab` entry, then
                    // read bytes from `.strtab`.
                    let strtab_sym_offs = symtab.entry_size() as usize * i;
                    let mut buf4 = [0; 4];
                    buf4.copy_from_slice(&symtab_content[strtab_sym_offs..strtab_sym_offs + 4]);
                    let offs = strtab_offs + u32::from_le_bytes(buf4) as u64;

                    let addr = sym.value();
                    let name = sym.demangled_name();
                    debug!("symtab i={} @ {:08x} name={}", i, offs, &name);

                    let mut all_offs = vec![offs];
                    name_to_debug_offs
                        .get(&name)
                        .map(|debug_offs| all_offs.push(*debug_offs));
                    name_to_info.insert(
                        name,
                        SymbolInfo {
                            addr,
                            offs: all_offs,
                        },
                    );
                }

                let size = file
                    .seek(std::io::SeekFrom::End(0))
                    .expect("Can't seek to end");

                BinInfo {
                    build_id_desc_offs,
                    build_id_desc,
                    name_to_info,
                    section_offs,
                    size,
                }
            }
            _ => panic!("Can't parse bin."),
        }
    }

    fn patch_syms(
        &self,
        name_to_info: &HashMap<String, SymbolInfo>,
        frame_infos: &Vec<FrameInfo>,
        start_tmp_name: &str,
        start_name: &str,
    ) {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("a.out")
            .expect("Can't open bin");

        for frame_info in frame_infos {
            for name in &frame_info.tmp_names {
                let frameline = frame_info.tmp_to_frameline.get(name).unwrap();
                name_to_info
                    .get(name)
                    .unwrap()
                    .offs
                    .iter()
                    .for_each(|offs| {
                        file.seek(std::io::SeekFrom::Start(*offs))
                            .expect(&*format!("Can't seek to 0x{:08x}", *offs));
                        file.write(frameline.as_bytes()).expect("Can't write bin");
                    });
            }
        }

        name_to_info
            .get(start_tmp_name)
            .unwrap()
            .offs
            .iter()
            .for_each(|offs| {
                file.seek(std::io::SeekFrom::Start(*offs))
                    .expect(&*format!("Can't seek to 0x{:08x}", *offs));
                file.write(start_name.as_bytes()).expect("Can't write bin");
            });
    }

    /// Patch temporary names with frame lines.
    fn patch_bin(
        &self,
        frame_infos: &Vec<FrameInfo>,
        name_to_info: &HashMap<String, SymbolInfo>,
        start_tmp_name: &str,
        start_name: &str,
        _build_id_offs: u64,
    ) {
        self.patch_syms(name_to_info, frame_infos, start_tmp_name, start_name);
    }

    /// Output commands for debugging patched binary.
    fn write_dbg_script(
        &self,
        frame_infos: &Vec<FrameInfo>,
        name_to_info: &HashMap<String, SymbolInfo>,
        size: u64,
        is_updated: bool,
        bin: &str,
    );
}

pub struct GdbFrameConverter<'a> {
    pub parser: &'a dyn FrameParser,
}

pub struct LldbFrameConverter<'a> {
    pub parser: &'a dyn FrameParser,
}

pub struct CustomFrameConverter<'a> {
    pub inner: &'a dyn FrameConverter,
    pub file: &'a PathBuf,
    pub height: u16,
    pub width: u16,
}

impl CustomFrameConverter<'_> {
    fn patch_addrs(
        &self,
        name_to_info: &HashMap<String, SymbolInfo>,
        frame_infos: &Vec<FrameInfo>,
        text_offs: &u64,
        start_addr: u64,
    ) {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("a2.out")
            .expect("Can't open bin");

        file.seek(std::io::SeekFrom::Start(0))
            .expect("Can't seek bin");
        let mut contents = vec![];
        file.read_to_end(&mut contents).expect("Can't read bin");

        let start_offs = start_addr - self.text_section_addr() + text_offs;
        let contents_at_text_section = &contents[start_offs as usize..];
        let mut decoder = Decoder::with_ip(
            64,
            contents_at_text_section,
            start_offs,
            DecoderOptions::NONE,
        );
        let mut instr = Instruction::default();
        let mut info_factory = InstructionInfoFactory::new();
        let placeholder_addrs = [PLACEHOLDER_SYMTAB_ADDR, PLACEHOLDER_DEBUGSTR_ADDR];
        for frame_info in frame_infos {
            for name in &frame_info.tmp_names {
                for (i, offs) in name_to_info.get(name).unwrap().offs.iter().enumerate() {
                    debug!(
                        "{} for {} {:08x} {:08x}",
                        name, i, offs, placeholder_addrs[i]
                    );
                    let mut target_offs = None;
                    while decoder.can_decode() {
                        decoder.decode_out(&mut instr);
                        debug!(
                            "@ {:08x} => {:?} {:?}",
                            instr.ip(),
                            instr.code(),
                            instr.op_kinds().collect::<Vec<OpKind>>()
                        );

                        // bf 04 03 02 01    mov   edi,0x01020304
                        // e8 0e fe ff ff    call  0x4011fd <draw_line>
                        let info = info_factory.info(&instr);
                        if instr.op_count() == 2
                            && info.used_registers().len() == 1
                                && info.used_registers().first().unwrap().access() == OpAccess::Write
                                && instr.op0_kind() == OpKind::Register
                                && instr.op1_kind() == OpKind::Immediate32
                                // Assumes instruction order is preserved between calls.
                                && instr.try_immediate(1).unwrap() == placeholder_addrs[i]
                        {
                            target_offs = Some(instr.ip() + 1);
                        } else if instr.op_count() == 1
                            && instr.op0_kind() == OpKind::NearBranch64
                            && instr.mnemonic() == Mnemonic::Call
                            && target_offs.is_some()
                        {
                            break;
                        }
                    }
                    if target_offs.is_none() {
                        panic!("Compiler generated unhandled instructions?");
                    }

                    debug!("sym @ {:08x} => patch @ {:08x}", offs, target_offs.unwrap());
                    file.seek(std::io::SeekFrom::Start(target_offs.unwrap()))
                        .expect(&*format!("Can't seek to 0x{:08x}", target_offs.unwrap()));
                    file.write(&(offs + self.inner.data_section_addr()).to_le_bytes()[..4])
                        .expect("Can't write bin");
                }
            }
        }
    }

    fn patch_build_id(&self, offs: u64, desc: Vec<u8>) {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("a2.out")
            .expect("Can't open bin");

        debug!("Patching build id @ 0x{:08x} = {:x?}.", offs, &desc);
        file.seek(std::io::SeekFrom::Start(offs))
            .expect(&*format!("Can't seek to 0x{:08x}", offs));
        file.write(&desc).expect("Can't write build id");
    }
}

impl FrameConverter for CustomFrameConverter<'_> {
    fn parser(&self) -> &dyn FrameParser {
        self.inner.parser()
    }

    fn prepare_src(
        &self,
        frame_infos: &Vec<FrameInfo>,
        start_tmp_name: &str,
        has_debug_info: bool,
    ) -> String {
        let input_src = std::fs::read_to_string(self.file).unwrap();
        let draw_line_calls = frame_infos
            .iter()
            .map(|_| {
                let mut o = String::new();
                for i in 0..self.height {
                    let prefix_offset = if i == self.height - 1 {
                        10 // \x1b[1;1H\x1b[2K
                    } else {
                        9 // \x1b[2K\x1b[99D
                    };
                    o = format!(
                        r#"{}
    draw_line((uint8_t*){}UL, {}, {});"#,
                        o,
                        format!("0x{:08x}", PLACEHOLDER_SYMTAB_ADDR),
                        prefix_offset,
                        self.height - 1 - i
                    );
                    if has_debug_info {
                        o = format!(
                            r#"{}
    draw_line((uint8_t*){}UL, {}, {});"#,
                            o,
                            format!("0x{:08x}", PLACEHOLDER_DEBUGSTR_ADDR),
                            prefix_offset,
                            self.height - 1 - i
                        );
                    }
                }
                o
            })
            .collect::<Vec<String>>()
            .join("\n");

        let heads = frame_infos
            .iter()
            .map(|n| format!("{}();", n.first_name))
            .collect::<Vec<String>>()
            .join("\n    ");
        let calls = frame_infos
            .iter()
            .map(|n| {
                let mut o = String::new();
                for (prev, next) in n.tmp_names.iter().tuple_windows() {
                    o = format!(
                        r#"
void {}() {{
    {}();
}}
{}"#,
                        prev, next, o
                    );
                }
                format!(
                    r#"
void {}() {{
    return;
}}
{}"#,
                    n.tmp_names.last().unwrap(),
                    o
                )
            })
            .collect::<Vec<String>>()
            .join("\n");

        format!(
            r#"
{}

{}

void {}() {{
    init(123, {}, {});
loop:
    update_frame();
    {}
    {}
    goto loop;
}}"#,
            calls, input_src, start_tmp_name, self.width, self.height, draw_line_calls, heads
        )
    }

    fn compile(
        &self,
        src: &str,
        compiler: &str,
        start_tmp_name: &str,
        include_debug_info: bool,
    ) -> Result<(), Box<dyn Error>> {
        let name = std::path::Path::new("a.c");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(name)?;
        file.write_all(src.as_bytes())?;
        spawn(
            Command::new(compiler).args(
                include_debug_info
                    .then_some(&["-g"])
                    .into_iter()
                    .flatten()
                    .chain(COMPILER_ARGS)
                    .chain(&["-c", "-o", "a.o", name.to_str().unwrap()]),
            ),
        )?;

        spawn(Command::new("ld").args(&[
            "--build-id",
            "-e",
            &start_tmp_name,
            "-o",
            "a.out",
            "a.o",
            "-T",
            "a.ld",
        ]))
    }

    fn patch_bin(
        &self,
        frame_infos: &Vec<FrameInfo>,
        name_to_info: &HashMap<String, SymbolInfo>,
        start_tmp_name: &str,
        start_name: &str,
        build_id_desc_offs: u64,
    ) {
        // Since CustomFrameConverters have the program code itself
        // patching symbols with binary-coded decimals, these
        // symbols have placeholder framelines with zeros on each
        // "r:g:b" component.
        FrameConverter::patch_syms(self, name_to_info, frame_infos, start_tmp_name, start_name);

        // We have to convince debuggers to reload these symbols.
        // However, sections such as `.symtab` are not loaded
        // in memory when a process is executed. But debuggers still
        // expect a valid ELF file to load symbols from.
        //
        // The trick we do here is to embed the previously compiled
        // binary into a custom section (`.data`) that we explicitly
        // load as writable memory.
        spawn(Command::new("ld").args(&[
            "--build-id",
            "-e",
            &start_tmp_name,
            "-o",
            "a2.out",
            "a.o",
            "-T",
            format!("a2.0x{:04x}.ld", self.inner.data_section_addr()).as_str(),
        ]))
        .unwrap();

        // We now modify placeholder addresses in the compiled code
        // to instead reference the symbols in the `.symtab` section
        // embedded in the `data` section.
        //
        // While GDB only needs to parse section `.symtab` to
        // reload symbols, other debuggers require additional setup:
        //
        // * VSCode instead reads debug info, in particular, symbols
        //   from section `.debug_str` (we will treat these as just
        //   additional addresses for writing in-memory);
        // * LLDB only handles files that match loaded modules,
        //   either by CRC, or by Build ID descriptor in section
        //   `.note.gnu.build-id` (which is easier to lie about:
        //   we can just patch it with the second binary's Build ID);
        let bin_info2 = FrameConverter::parse_bin(self, "a2.out");
        CustomFrameConverter::patch_addrs(
            &self,
            &name_to_info,
            &frame_infos,
            bin_info2.section_offs.get(".text").unwrap(),
            bin_info2.name_to_info.get(start_tmp_name).unwrap().addr,
        );
        CustomFrameConverter::patch_build_id(
            &self,
            bin_info2.section_offs.get(".data").unwrap() + build_id_desc_offs,
            bin_info2.build_id_desc,
        );
    }

    fn write_dbg_script(
        &self,
        frame_infos: &Vec<FrameInfo>,
        name_to_info: &HashMap<String, SymbolInfo>,
        size: u64,
        _is_updated: bool,
        _bin: &str,
    ) {
        self.inner
            .write_dbg_script(frame_infos, name_to_info, size, true, "a2.out")
    }
}

impl FrameConverter for GdbFrameConverter<'_> {
    fn parser(&self) -> &dyn FrameParser {
        self.parser
    }

    fn write_dbg_script(
        &self,
        frame_infos: &Vec<FrameInfo>,
        name_to_info: &HashMap<String, SymbolInfo>,
        _size: u64,
        is_updated: bool,
        bin: &str,
    ) {
        let bp_info = frame_infos
            .iter()
            .map(|n| (name_to_info.get(&n.last_name).unwrap().addr, n.delay))
            .collect_vec();
        println!(
            "\n{}",
            "Render automatically with debugger script:".purple().bold()
        );
        println!("{}", format!("gdb ./{bin} --command a_gdb.py").bold());
        println!(
            "\n{}",
            "Render manually with software breakpoints:".purple().bold()
        );
        println!(
            "{}",
            format!(
                r#"gdb ./{bin} \
    -ex 'set pagination off' \
    -ex 'set style enabled off' \
    -ex 'set startup-with-shell off' \"#
            )
            .bold()
        );
        println!(
            "{}",
            [String::from("    -ex 'starti'")]
                .into_iter()
                .chain(
                    bp_info
                        .iter()
                        .map(|(addr, _)| format!("    -ex 'b *0x{:08x}'", addr))
                )
                .join(" \\\n")
                .bold()
        );

        let breakpoints = bp_info
            .iter()
            .circular_tuple_windows::<(_, _)>()
            .map(|(prev, next)| {
                format!(
                    "{}[0x{:08x}, 0x{:08x}, {}],",
                    " ".repeat(4),
                    prev.0,
                    next.0,
                    prev.1 * 10
                )
            })
            .collect::<Vec<String>>()
            .join("\n");

        let symbol_reload = is_updated
            .then(|| {
                String::from(
                    r#"
        gdb.execute(f"symbol-file a2.out")
        gdb.execute(f"symbol-file /proc/{gdb.selected_inferior().pid}/mem")"#,
                )
            })
            .unwrap_or_else(|| String::new());

        let o = format!(
            r#"
#!/usr/bin/env python3

import gdb
import time

class B(gdb.Breakpoint):
    def __init__(self, offset, next_offset, delay):
        self.delay = delay
        gdb.Breakpoint.__init__(self, f"*{{offset}}", gdb.BP_HARDWARE_BREAKPOINT)

    def stop(self):
        {}

        gdb.execute("delete breakpoints")
        global bp_i
        bp_i = (bp_i + 1) % {}
        B(*bps[bp_i])

        gdb.execute("bt")
        time.sleep(self.delay / 1000)
        return False

gdb.execute("set pagination off")
gdb.execute("set style enabled off")
gdb.execute("set startup-with-shell off")

gdb.execute("starti")
bp_i = 0
bps = [
{}
]
B(*bps[bp_i])
gdb.execute("c")
"#,
            symbol_reload,
            bp_info.len(),
            breakpoints
        );
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open("a_gdb.py")
            .unwrap();
        file.write(o.as_bytes()).expect("Can't write GDB script");
    }
}

impl FrameConverter for LldbFrameConverter<'_> {
    fn data_section_addr(&self) -> u64 {
        0x1000
    }

    fn parser(&self) -> &dyn FrameParser {
        self.parser
    }

    fn write_dbg_script(
        &self,
        frame_infos: &Vec<FrameInfo>,
        name_to_info: &HashMap<String, SymbolInfo>,
        size: u64,
        is_updated: bool,
        bin: &str,
    ) {
        let bp_info = frame_infos
            .iter()
            .map(|n| (name_to_info.get(&n.last_name).unwrap().addr, n.delay))
            .collect_vec();
        println!(
            "\n{}",
            "Render automatically with debugger script:".purple().bold()
        );
        println!(
            "{}",
            format!("lldb ./{bin} --one-line 'command script import a_lldb.py'").bold()
        );
        println!(
            "\n{}",
            "Render manually with software breakpoints:".purple().bold()
        );
        println!(
            "{}",
            format!(
                r#"lldb ./{bin} \
    --one-line 'settings set use-color false' \
    --one-line 'settings set show-statusline false' \
    --one-line 'process launch --disable-aslr true --no-stdio --stop-at-entry' \"#
            )
            .bold()
        );
        println!(
            "{}",
            &bp_info
                .iter()
                .map(|(addr, _)| format!("    --one-line 'b *0x{:08x}'", addr))
                .join(" \\\n")
                .bold()
        );

        let breakpoints = bp_info
            .iter()
            .circular_tuple_windows::<(_, _)>()
            .map(|(prev, next)| {
                format!(
                    "{}[0x{:08x}, 0x{:08x}, {}],",
                    " ".repeat(4),
                    prev.0,
                    next.0,
                    prev.1 * 10
                )
            })
            .collect::<Vec<String>>()
            .join("\n");

        // Due to llvm-project issue #153772, the `.data` section
        // is instead mapped at address 0x1000, as it needs to be
        // after the zero page. This also implies that the written
        // memory map cannot be read as-is, as LLDB starts reading
        // from offset 0, and gets an EIO (Input/output error).
        //
        // As a workaround, this memory must be dumped to a
        // temporary file on each displayed frame.
        let symbol_reload = is_updated
            .then(|| {
                format!(
                    r#"
    debugger.HandleCommand("target symbols add a2.out")
    debugger.HandleCommand("memory read --binary --outfile /tmp/mem --count 0x{:08x} 0x{:08x}")
    debugger.HandleCommand("target symbols add /tmp/mem")
    "#,
                    size,
                    self.data_section_addr()
                )
            })
            .unwrap_or_else(|| String::new());

        let o = format!(
            r#"
#!/usr/bin/env python3

import lldb
import os
import sys
import time

def b(frame, bp_loc, extra_args, dict):
    debugger = frame.GetThread().GetProcess().GetTarget().GetDebugger()
    {}
    debugger.HandleCommand("bt")

    delay = extra_args.GetValueForKey("delay").GetIntegerValue()
    time.sleep(delay / 1000)

def a(debugger, command, ctx, result, dict):
    # https://github.com/llvm/llvm-project/blob/6e3c7b8244e9067721ccd0d786755f2ae9c96a87/lldb/include/lldb/lldb-enumerations.h#L99
    flags = lldb.eLaunchFlagDisableASLR | lldb.eLaunchFlagDisableSTDIO | lldb.eLaunchFlagDebug
    process = ctx.GetTarget().Launch(debugger.GetListener(), None, None, "/dev/null", None, None, os.getcwd(), flags, True, lldb.SBError())
    if not process:
        raise RuntimeError("Process not launched.")
    if process.GetState() != lldb.eStateStopped:
        raise RuntimeError("Process not stopped.")

    target = process.GetTarget()
    for addr, next_addr, delay in [
{}
    ]:
        extra_args = lldb.SBStructuredData()
        stream = lldb.SBStream()
        stream.Print(f'{{{{"delay" : {{delay}}}}}}')
        extra_args.SetFromJSON(stream)

        bp = target.BreakpointCreateByAddress(addr)
        bp.SetAutoContinue(True)
        bp.SetScriptCallbackFunction("a_lldb.b", extra_args)
        # FIXME: Unimplemented for Linux x86_64 targets
        # err = bp.SetIsHardware(True)
        # if not bp.IsHardware():
        #     raise RuntimeError(err.value)

    debugger.SetAsync(True)
    process.Continue()


def __lldb_init_module(debugger, dict):
    debugger.HandleCommand("settings set use-color false")
    debugger.HandleCommand("settings set show-statusline false")
    debugger.HandleCommand("command script add -f a_lldb.a a")
    debugger.HandleCommand("a")
    "#,
            symbol_reload, breakpoints
        );
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open("a_lldb.py")
            .unwrap();
        file.write(o.as_bytes()).expect("Can't write LLDB script");
    }
}

fn spawn(cmd: &mut Command) -> Result<(), Box<dyn Error>> {
    println!(
        "Running `{} {}`.",
        cmd.get_program().to_str().unwrap(),
        cmd.get_args().map(|a| a.to_str().unwrap()).join(" ")
    );
    let child = cmd.stderr(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        let raw_output = String::from_utf8(output.stdout)?;
        if !raw_output.is_empty() {
            println!("{raw_output}");
        }
        Ok(())
    } else {
        let raw_err = String::from_utf8(output.stderr)?;
        eprintln!("{raw_err}");
        Err("Compile error".into())
    }
}
