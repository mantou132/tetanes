use crate::{
    common::{Kind, Reset},
    ppu::{
        vram::{SYSTEM_PALETTE, SYSTEM_PALETTE_SIZE},
        RENDER_CHANNELS, RENDER_HEIGHT, RENDER_SIZE, RENDER_WIDTH,
    },
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{f64::consts::PI, fmt};

#[derive(Clone, Serialize, Deserialize)]
#[must_use]
pub struct Frame {
    pub num: u32,
    pub shift_lo: u16,
    pub shift_hi: u16,
    // Shift registers
    // Tile data - stored in cycles 0 mod 8
    pub tile_addr: u16,
    pub tile_lo: u8,
    pub tile_hi: u8,
    pub prev_palette: u8,
    pub curr_palette: u8,
    pub palette: u8,
    pub prev_pixel: u32,
    pub last_updated_pixel: u32,
    front_buffer: Vec<u16>,
    back_buffer: Vec<u16>,
    output_buffer: Vec<u8>,
}

impl Frame {
    pub fn new() -> Self {
        let mut frame = Self {
            num: 0,
            shift_lo: 0x0000,
            shift_hi: 0x0000,
            tile_addr: 0x0000,
            tile_lo: 0x00,
            tile_hi: 0x00,
            prev_palette: 0x00,
            curr_palette: 0x00,
            palette: 0x00,
            prev_pixel: 0xFFFF_FFFF,
            last_updated_pixel: 0x0000_0000,
            front_buffer: vec![0; (RENDER_WIDTH * RENDER_HEIGHT) as usize],
            back_buffer: vec![0; (RENDER_WIDTH * RENDER_HEIGHT) as usize],
            output_buffer: vec![0; RENDER_SIZE],
        };
        frame.reset(Kind::Hard);
        frame
    }

    #[inline]
    pub fn increment(&mut self) {
        self.num += 1;
    }

    #[inline]
    pub fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.front_buffer, &mut self.back_buffer);
    }

    #[inline]
    #[must_use]
    pub fn get_color(&self, x: u32, y: u32) -> u16 {
        self.back_buffer[(x + (y << 8)) as usize]
    }

    #[inline]
    pub fn put_pixel(&mut self, x: u32, y: u32, color: u16) {
        self.back_buffer[(x + (y << 8)) as usize] = color;
    }

    pub fn decode_buffer(&mut self) -> &[u8] {
        assert!(self.front_buffer.len() * 4 == self.output_buffer.len());
        for (pixel, colors) in self
            .front_buffer
            .iter()
            .zip(self.output_buffer.chunks_exact_mut(4))
        {
            assert!(colors.len() > 2);
            let (red, green, blue) = SYSTEM_PALETTE[(*pixel as usize) & (SYSTEM_PALETTE_SIZE - 1)];
            colors[0] = red;
            colors[1] = green;
            colors[2] = blue;
            // Alpha should always be 255
        }
        &self.output_buffer
    }

    // Amazing implementation Bisqwit! Much faster than my original, but boy what a pain
    // to translate it to Rust
    // Source: https://bisqwit.iki.fi/jutut/kuvat/programming_examples/nesemu1/nesemu1.cc
    // http://wiki.nesdev.com/w/index.php/NTSC_video
    pub fn apply_ntsc_filter(&mut self) -> &[u8] {
        assert!(self.front_buffer.len() * 4 == self.output_buffer.len());
        for (idx, (pixel, colors)) in self
            .front_buffer
            .iter()
            .zip(self.output_buffer.chunks_exact_mut(4))
            .enumerate()
        {
            let x = idx % 256;
            let color = if x == 0 {
                // Remove pixel 0 artifact from not having a valid previous pixel
                0
            } else {
                let y = idx / 256;
                let even_phase = if self.num & 0x01 == 0x01 { 0 } else { 1 };
                let phase = (2 + y * 341 + x + even_phase) % 3;
                NTSC_PALETTE
                    [phase + ((self.prev_pixel & 0x3F) as usize) * 3 + (*pixel as usize) * 3 * 64]
            };
            self.prev_pixel = u32::from(*pixel);
            assert!(colors.len() > 2);
            colors[0] = (color >> 16 & 0xFF) as u8;
            colors[1] = (color >> 8 & 0xFF) as u8;
            colors[2] = (color & 0xFF) as u8;
            // Alpha should always be 255
        }
        &self.output_buffer
    }
}

impl Reset for Frame {
    fn reset(&mut self, _kind: Kind) {
        self.num = 0;
        self.front_buffer.fill(0);
        self.back_buffer.fill(0);
        self.output_buffer.fill(0);
        if RENDER_CHANNELS == 4 {
            // Force alpha to 255.
            for p in self
                .output_buffer
                .iter_mut()
                .skip(RENDER_CHANNELS - 1)
                .step_by(RENDER_CHANNELS)
            {
                *p = 255;
            }
        }
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("num", &self.num)
            .field("shift_lo", &format_args!("${:04X}", &self.shift_lo))
            .field("shift_hi", &format_args!("${:04X}", &self.shift_hi))
            .field("tile_addr", &format_args!("${:04X}", &self.tile_addr))
            .field("tile_lo", &format_args!("${:02X}", &self.tile_lo))
            .field("tile_hi", &format_args!("${:02X}", &self.tile_hi))
            .field("prev_palette", &format_args!("${:02X}", &self.prev_palette))
            .field("curr_palette", &format_args!("${:02X}", &self.curr_palette))
            .field("palette", &format_args!("${:02X}", &self.palette))
            .field("prev_pixel", &self.prev_pixel)
            .field("last_updated_pixel", &self.last_updated_pixel)
            .finish()
    }
}

pub static NTSC_PALETTE: Lazy<Vec<u32>> = Lazy::new(|| {
    // NOTE: There's lot's to clean up here -- too many magic numbers and duplication but
    // I'm afraid to touch it now that it works
    // Source: https://bisqwit.iki.fi/jutut/kuvat/programming_examples/nesemu1/nesemu1.cc
    // http://wiki.nesdev.com/w/index.php/NTSC_video

    // Calculate the luma and chroma by emulating the relevant circuits:
    const VOLTAGES: [i32; 16] = [
        -6, -69, 26, -59, 29, -55, 73, -40, 68, -17, 125, 11, 68, 33, 125, 78,
    ];

    let mut ntsc_palette = vec![0; 512 * 64 * 3];

    // Helper functions for converting YIQ to RGB
    let gamma = 2.0; // Assumed display gamma
    let gammafix = |color: f64| {
        if color <= 0.0 {
            0.0
        } else {
            color.powf(2.2 / gamma)
        }
    };
    let yiq_divider = f64::from(9 * 10u32.pow(6));
    for palette_offset in 0..3 {
        for channel in 0..3 {
            for color0_offset in 0..512 {
                let emphasis = color0_offset / 64;

                for color1_offset in 0..64 {
                    let mut y = 0;
                    let mut i = 0;
                    let mut q = 0;
                    // 12 samples of NTSC signal constitute a color.
                    for sample in 0..12 {
                        let noise = (sample + palette_offset * 4) % 12;
                        // Sample either the previous or the current pixel.
                        // Use pixel=color0 to disable artifacts.
                        let pixel = if noise < 6 - channel * 2 {
                            color0_offset
                        } else {
                            color1_offset
                        };

                        // Decode the color index.
                        let chroma = pixel & 0x0F;
                        // Forces luma to 0, 4, 8, or 12 for easy lookup
                        let luma = if chroma < 0x0E { (pixel / 4) & 12 } else { 4 };
                        // NES NTSC modulator (square wave between up to four voltage levels):
                        let limit = if (chroma + 8 + sample) % 12 < 6 {
                            12
                        } else {
                            0
                        };
                        let high = if chroma > limit { 1 } else { 0 };
                        let emp_effect = if (152_278 >> (sample / 2 * 3)) & emphasis > 0 {
                            0
                        } else {
                            2
                        };
                        let level = 40 + VOLTAGES[high + emp_effect + luma];
                        // Ideal TV NTSC demodulator:
                        let (sin, cos) = (PI * sample as f64 / 6.0).sin_cos();
                        y += level;
                        i += level * (cos * 5909.0) as i32;
                        q += level * (sin * 5909.0) as i32;
                    }
                    // Store color at subpixel precision
                    let y = f64::from(y) / 1980.0;
                    let i = f64::from(i) / yiq_divider;
                    let q = f64::from(q) / yiq_divider;
                    let idx = palette_offset + color0_offset * 3 * 64 + color1_offset * 3;
                    match channel {
                        2 => {
                            let rgb =
                                255.95 * gammafix(q.mul_add(0.623_557, i.mul_add(0.946_882, y)));
                            ntsc_palette[idx] += 0x10000 * rgb.clamp(0.0, 255.0) as u32;
                        }
                        1 => {
                            let rgb =
                                255.95 * gammafix(q.mul_add(-0.635_691, i.mul_add(-0.274_788, y)));
                            ntsc_palette[idx] += 0x00100 * rgb.clamp(0.0, 255.0) as u32;
                        }
                        0 => {
                            let rgb =
                                255.95 * gammafix(q.mul_add(1.709_007, i.mul_add(-1.108_545, y)));
                            ntsc_palette[idx] += rgb.clamp(0.0, 255.0) as u32;
                        }
                        _ => (), // invalid channel
                    }
                }
            }
        }
    }

    ntsc_palette
});
