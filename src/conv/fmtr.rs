//! Frame formatting types.

use palette::color_difference::Ciede2000;
use palette::convert::FromColorUnclamped;
use palette::{Lab, Srgb};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

pub trait FrameFormatter {
    fn blank(&self) -> &str;

    fn placeholder(&self) -> &str;

    fn to_framedot(&self, rgba: Option<Vec<u8>>) -> String;

    fn to_frameline_at_origin(&self, name: &String, clear_line: bool) -> String;

    fn to_frameline(&self, name: &String) -> String;
}

pub struct EmojiFrameFormatter {
    /// RGB hex values to closest UTF-8 emoji codepoint, based on
    /// smallest color difference against pre-computed
    /// color mappings in `bgr_to_emoji.json`
    pub cache: RefCell<HashMap<String, String>>,

    /// RGB hex values to CIE L*a*b*
    pub rgb_to_lab: HashMap<String, Lab>,

    /// RGB hex values to UTF-8 emoji codepoints
    pub rgb_to_emoji: HashMap<String, String>,
}

pub struct TrueColorFrameFormatter;

impl EmojiFrameFormatter {
    pub fn new() -> Self {
        let mut this = Self {
            cache: RefCell::new(HashMap::new()),
            rgb_to_lab: HashMap::new(),
            rgb_to_emoji: HashMap::new(),
        };

        let json: Value = serde_json::from_str(
            std::fs::read_to_string("bgr_to_emoji.json")
                .unwrap()
                .as_str(),
        )
        .unwrap();
        for v in json.as_array().unwrap() {
            let rgb = format!(
                "{:02x}{:02x}{:02x}",
                v[2].as_u64().unwrap(),
                v[1].as_u64().unwrap(),
                v[0].as_u64().unwrap()
            );
            let lab: Lab = Lab::from_color_unclamped(Srgb::new(
                v[2].as_u64().unwrap() as f32 / 255.0,
                v[1].as_u64().unwrap() as f32 / 255.0,
                v[0].as_u64().unwrap() as f32 / 255.0,
            ));
            this.rgb_to_lab.insert(rgb.to_owned(), lab);
            this.rgb_to_emoji
                .insert(rgb, String::from(v[3].as_str().unwrap()));
        }

        this
    }

    pub fn lookup(&self, rgba: Vec<u8>) -> String {
        let candidate_rgb = format!("{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]);
        if self.cache.borrow().contains_key(&candidate_rgb) {
            return self.cache.borrow().get(&candidate_rgb).unwrap().to_owned();
        }

        let candidate_lab: Lab = Lab::from_color_unclamped(Srgb::new(
            rgba[0] as f32 / 255.0,
            rgba[1] as f32 / 255.0,
            rgba[2] as f32 / 255.0,
        ));
        let mut min_diff = f32::MAX;
        let mut best_rgb = &candidate_rgb;
        for (rgb, lab) in self.rgb_to_lab.iter() {
            let diff = lab.difference(candidate_lab);
            if min_diff > diff {
                min_diff = diff;
                best_rgb = rgb;
            }
        }
        let best_emoji = self.rgb_to_emoji.get(best_rgb).unwrap();
        self.cache
            .borrow_mut()
            .insert(candidate_rgb.to_owned(), best_emoji.to_owned());

        best_emoji.to_owned()
    }
}

impl FrameFormatter for EmojiFrameFormatter {
    fn blank(&self) -> &str {
        "🫥"
    }

    fn placeholder(&self) -> &str {
        self.blank()
    }

    /// Convert color value to closest UTF-8 emoji codepoint.
    fn to_framedot(&self, rgba: Option<Vec<u8>>) -> String {
        rgba.map_or(String::from(self.placeholder()), |rgba| match rgba[3] {
            0 => String::from(self.blank()),
            _ => self.lookup(rgba),
        })
    }

    fn to_frameline_at_origin(&self, name: &String, _clear_line: bool) -> String {
        self.to_frameline(name)
    }

    fn to_frameline(&self, name: &String) -> String {
        name.to_owned()
    }
}

impl FrameFormatter for TrueColorFrameFormatter {
    /// Double-width spacing rendered as a square frame dot.
    fn blank(&self) -> &str {
        "  "
    }

    /// Black in 24-bit rgb color code.
    fn placeholder(&self) -> &str {
        "000:000:000"
    }

    /// Convert "r:g:b" hex values to a terminal sequence representing
    /// a single frame dot, encoded in 24-bit truecolor
    /// a.k.a. "888" colors a.k.a. 16 million colors.
    ///
    /// See: <https://tintin.mudhalla.net/info/truecolor/>
    fn to_framedot(&self, rgba: Option<Vec<u8>>) -> String {
        let mut rgb = String::new();
        rgba.map_or(Some(self.placeholder()), |rgba| {
            rgb = rgba[0..3]
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<String>>()
                .join(":");
            let a = rgba[3];
            match a {
                0 => None,
                _ => Some(rgb.as_str()),
            }
        })
        .map_or(String::from(self.blank()), |rgb| {
            // \x1b[48:2::{}m => Background 24-bit rgb color code;
            // \x1b[49m => Default background color;
            format!("\x1b[48:2::{}m{}\x1b[49m", rgb, self.blank())
        })
    }

    fn to_frameline_at_origin(&self, name: &String, clear_line: bool) -> String {
        // \x1b[1;1H => Set cursor position to screen origin [row=1;column=1];
        // \x1b[2K => Erase all in line;
        // \x1b[2J => Erase all in display;
        // \x1b[8m => Character attribute invisible: hides trailing argument parenthesis (gdb) / function offset (lldb);
        // \x1b[?25l => Hide cursor (DECTCEM);
        format!(
            "\x1b[1;1H\x1b[2{}{}\x1b[8m\x1b[?25l",
            if clear_line { "K" } else { "J" },
            name
        )
    }

    fn to_frameline(&self, name: &String) -> String {
        // \x1b[1K => Erase to left of cursor in line;
        // \x1b[99D => Cursor backward 99 times;
        // \x1b[3K => Erase to right of cursor in line;
        // \x1b[8m => Character attribute invisible: hides trailing argument parenthesis (gdb) / function offset (lldb);
        // \x1b[?25l => Hide cursor (DECTCEM);
        format!("\x1b[1K\x1b[99D{}\x1b[3K\x1b[8m\x1b[?25l", name)
    }
}
