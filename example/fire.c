#include "stdint.h"

static const uint32_t palette[256] = {
/* Jare's original FirePal. */
#define C(r,g,b) ((((r) * 4) << 16) | ((g) * 4 << 8) | ((b) * 4))
C( 0,   0,   0), C( 0,   1,   1), C( 0,   4,   5), C( 0,   7,   9),
C( 0,   8,  11), C( 0,   9,  12), C(15,   6,   8), C(25,   4,   4),
C(33,   3,   3), C(40,   2,   2), C(48,   2,   2), C(55,   1,   1),
C(63,   0,   0), C(63,   0,   0), C(63,   3,   0), C(63,   7,   0),
C(63,  10,   0), C(63,  13,   0), C(63,  16,   0), C(63,  20,   0),
C(63,  23,   0), C(63,  26,   0), C(63,  29,   0), C(63,  33,   0),
C(63,  36,   0), C(63,  39,   0), C(63,  39,   0), C(63,  40,   0),
C(63,  40,   0), C(63,  41,   0), C(63,  42,   0), C(63,  42,   0),
C(63,  43,   0), C(63,  44,   0), C(63,  44,   0), C(63,  45,   0),
C(63,  45,   0), C(63,  46,   0), C(63,  47,   0), C(63,  47,   0),
C(63,  48,   0), C(63,  49,   0), C(63,  49,   0), C(63,  50,   0),
C(63,  51,   0), C(63,  51,   0), C(63,  52,   0), C(63,  53,   0),
C(63,  53,   0), C(63,  54,   0), C(63,  55,   0), C(63,  55,   0),
C(63,  56,   0), C(63,  57,   0), C(63,  57,   0), C(63,  58,   0),
C(63,  58,   0), C(63,  59,   0), C(63,  60,   0), C(63,  60,   0),
C(63,  61,   0), C(63,  62,   0), C(63,  62,   0), C(63,  63,   0),
/* Followed by "white heat". */
#define W C(63,63,63)
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W,
W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W, W
#undef W
#undef C
};

static uint16_t width = 0;
static uint16_t height = 0;

static uint8_t arena[0x1f0000];

static uint8_t *fire;
static uint8_t *prev_fire;
static uint32_t *framebuf;

static inline uint16_t bcd(uint16_t v) {
    uint16_t acc = 0;
    uint16_t base = 0;
    while (v > 0) {
        acc |= (v % 10) << base;
        base += 4;
        v /= 10;
    }

    return acc;
}

void init(uint64_t seed, uint16_t w, uint16_t h) {
    width = w;
    /* Skip rendering framebuffer's first 2 lines (always zeros). */
    height = h + 2;

    /* Worry-free bump allocator. */
    fire = arena;
    prev_fire = &arena[width * height];
    framebuf = (uint32_t *) &arena[2 * width * height];
}

void update_frame() {
    int i;
    uint32_t sum;
    uint8_t avg;
    for (i = width + 1; i < (height - 1) * width - 1; i++) {
        /* Average the eight neighbours. */
        sum = prev_fire[i - width - 1] + prev_fire[i - width] +
              prev_fire[i - width + 1] + prev_fire[i - 1] + prev_fire[i + 1] +
              prev_fire[i + width - 1] + prev_fire[i + width] +
              prev_fire[i + width + 1];
        avg = (uint8_t)(sum / 8);

        /* "Cool" the pixel if the two bottom bits of the
           sum are clear (somewhat random). For the bottom
           rows, cooling can overflow, causing "sparks". */
        if (!(sum & 3) && (avg > 0 || i >= (height - 4) * width)) {
            avg--;
        }
        fire[i] = avg;
    }

    /* Copy back and scroll up one row.
       The bottom row is all zeros, so it can be skipped. */
    for (i = 0; i < (height - 2) * width; i++) {
        prev_fire[i] = fire[i + width];
    }

    /* Remove dark pixels from the bottom rows (except again the
       bottom row which is all zeros). */
    for (i = (height - 7) * width; i < (height - 1) * width; i++) {
        if (fire[i] < 15) {
            fire[i] = 22 - fire[i];
        }
    }

    /* Copy to framebuffer and map to RGBA, scrolling up one row. */
    for (i = 0; i < (height - 2) * width; i++) {
        framebuf[i] = palette[fire[i + width]];
    }
}

void draw_line(uint8_t *addr, uint8_t offs, uint16_t n) {
    /* Line starting at addr is assumed to be already filled, we
       just compute and update rgb decimal values inplace. ANSI
       color codes support leading zeros, so we don't have to adjust
       the line to decimal values with different lengths. */
    addr += offs;
    for (int i = 0; i < width; i++) {
        /* \x1b[48:2::000:000:000m  \x1b[49m */
        uint64_t dot_len = 27;

        uint64_t value = framebuf[n * width + i];

        uint16_t r = bcd((value >> 16) % 256);
        *(addr + (dot_len * i) + 8) = ((r >> 8) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 9) = ((r >> 4) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 10) = ((r >> 0) & 0xf) | 0x30;

        uint16_t g = bcd((value >> 8) % 256);
        *(addr + (dot_len * i) + 12) = ((g >> 8) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 13) = ((g >> 4) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 14) = ((g >> 0) & 0xf) | 0x30;

        uint16_t b = bcd((value >> 0) % 256);
        *(addr + (dot_len * i) + 16) = ((b >> 8) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 17) = ((b >> 4) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 18) = ((b >> 0) & 0xf) | 0x30;
    }
}
