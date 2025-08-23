//! `backgif` command line binary.

mod conv;

use clap::{Parser, ValueEnum};
use colored::Colorize;
use conv::fmtr::{EmojiFrameFormatter, FrameFormatter, TrueColorFrameFormatter};
use conv::{
    CustomFrameConverter, CustomFrameParser, FrameConverter, FrameParser, GdbFrameConverter,
    GifFrameParser, LldbFrameConverter,
};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about, long_about = None)]
struct Args {
    /// Input file used to parse frames
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Input file format
    #[arg(short, long, value_enum, default_value_t=InputFormat::GIF)]
    format: InputFormat,

    /// Frame renderer format
    #[arg(short, long, value_enum, default_value_t=RenderFormat::TrueColor)]
    renderer: RenderFormat,

    /// Target debugger to generate commands and automation script
    #[arg(short, long, value_enum, default_value_t=Debugger::GDB)]
    debugger: Debugger,

    /// Pass this argument to only clear each line being rendered,
    /// but can leave artifacts on screen; Omit this argument to
    /// clear all lines on the screen when rendering a new frame,
    /// but can cause flickering with large frames (> 20x20)
    #[arg(long, action)]
    clear_line: bool,

    /// Pass this argument to include debug info when compiling
    #[arg(long, action)]
    debug_info: bool,

    /// Custom frame delay in units of 10 ms
    #[arg(long)]
    delay: Option<u16>,

    /// Custom frame height in number of dots
    #[arg(long)]
    height: Option<u16>,

    /// Custom frame width in number of dots
    #[arg(long)]
    width: Option<u16>,
}

#[derive(ValueEnum, Clone, Debug)]
enum Debugger {
    GDB,
    LLDB,
}

#[derive(ValueEnum, Clone, Debug)]
enum InputFormat {
    /// C source file with functions for building custom frames
    ///
    /// ```c
    /// // Called at the beginning of start function, supplying a
    /// // `seed` for the initial state of PRNGs, along with the
    /// // configured frame width `w` and height `h`.
    /// void init(uint64_t seed, uint16_t w, uint16_t h);
    ///
    /// // Called at the beginning of each frame.
    /// void update_frame();
    ///
    /// // Renders frame line `n` containing up to
    /// // `width` dots, updating the corresponding symbol at `addr`.
    /// // First dot is after frame line prefix `offs`.
    /// void draw_line(uint8_t *addr, uint8_t offs, uint16_t n);
    /// ```
    C,

    /// GIF binary file
    GIF,
}

#[derive(ValueEnum, Clone, Debug)]
enum RenderFormat {
    /// UTF-8 emoji codepoints
    Emoji,

    /// 24-bit truecolor for virtual terminal emulators
    TrueColor,
}

fn main() {
    let args = Args::parse();

    let formatter: &dyn FrameFormatter = match args.renderer {
        RenderFormat::Emoji => &EmojiFrameFormatter::new(),
        RenderFormat::TrueColor => &TrueColorFrameFormatter,
    };
    let parser: &dyn FrameParser = match args.format {
        InputFormat::C => &CustomFrameParser {
            formatter,
            height: args.height.expect("Custom parser requires passing height"),
            width: args.width.expect("Custom parser requires passing width"),
        },
        InputFormat::GIF => &GifFrameParser { formatter },
    };
    let compiler: &str = match args.debugger {
        Debugger::GDB => "gcc",
        Debugger::LLDB => "clang",
    };
    let inner: &dyn FrameConverter = match args.debugger {
        Debugger::GDB => &GdbFrameConverter { parser },
        Debugger::LLDB => &LldbFrameConverter { parser },
    };
    let converter: &dyn FrameConverter = match args.format {
        InputFormat::C => {
            let min_addr = std::fs::read_to_string("/proc/sys/vm/mmap_min_addr")
                .unwrap()
                .trim()
                .parse::<u64>()
                .unwrap();
            if min_addr > 0 {
                eprintln!(
                    "{}\n",
                    format!(
                        "[!] Custom input expects `/proc/sys/vm/mmap_min_addr = 0`, got `{}`.",
                        min_addr
                    )
                    .red()
                    .bold()
                );
            }

            if matches!(args.debugger, Debugger::LLDB) {
                eprintln!("{}\n","[!] Workaround for llvm-project issue #153772: each frame dumps memory to a temporary file, mind your SSD lifespan!".red().bold());
                if !args.debug_info {
                    eprintln!("{}\n","[!] LLDB does not reload .symtab symbols, consider passing `--debug-info` to instead use .debug_str entries.".red().bold());
                }
            }

            if matches!(args.renderer, RenderFormat::Emoji) {
                panic!("Custom input not supported with emoji formatter 😞.");
            }

            &CustomFrameConverter {
                inner,
                file: &args.file,
                height: args.height.expect("Custom input requires passing height"),
                width: args.width.expect("Custom input requires passing width"),
            }
        }
        InputFormat::GIF => inner,
    };

    let frame_infos = converter.parse_input(&args.file, args.clear_line, args.delay);
    let (start_name, start_tmp_name) = parser.to_frameline_names(
        formatter,
        // Entrypoint symbol (overrides default symbol `_start`)
        // is not used as frame line, so it can be filled with
        // "Zero Width No-Break Space" (ZWNBSP).
        &String::from_utf8(b"\xef\xbb\xbf".repeat(4)).unwrap(),
        0,
        false,
        args.clear_line,
    );

    let src = converter.prepare_src(&frame_infos, &start_tmp_name, args.debug_info);
    converter
        .compile(&src, &compiler, &start_tmp_name, args.debug_info)
        .unwrap();

    let bin_info = converter.parse_bin("a.out");
    converter.patch_bin(
        &frame_infos,
        &bin_info.name_to_info,
        &start_tmp_name,
        &start_name,
        bin_info.build_id_desc_offs,
    );

    converter.write_dbg_script(&frame_infos, &bin_info.name_to_info, bin_info.size, false, "a.out");
}
