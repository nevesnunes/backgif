#include "stdint.h"

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

static inline uint64_t rotl(uint64_t x, int k) {
    return (x << k) | (x >> (-k & 0x3f));
}

static uint16_t width = 0;
static uint16_t height = 0;

static uint64_t state[2];

uint64_t next(void) {
    uint64_t s0 = state[0], s1 = state[1];
    uint64_t result = rotl((s0 + s1) * 9, 29) + s0;

    state[0] = s0 ^ rotl(s1, 29);
    state[1] = s0 ^ (s1 << 9);

    return result;
}

void init(uint64_t seed, uint16_t w, uint16_t h) {
    width = w;
    height = h;

    for (int i = 0; i < 2; i++) {
        state[i] = seed = seed * 6364136223846793005 + 1442695040888963407;
    }
}

void update_frame() {
    /* Nothing. */
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

        uint64_t value = next();

        uint16_t r = bcd((value >> 0) % 256);
        *(addr + (dot_len * i) + 8) = ((r >> 8) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 9) = ((r >> 4) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 10) = ((r >> 0) & 0xf) | 0x30;

        uint16_t g = bcd((value >> 12) % 256);
        *(addr + (dot_len * i) + 12) = ((g >> 8) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 13) = ((g >> 4) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 14) = ((g >> 0) & 0xf) | 0x30;

        uint16_t b = bcd((value >> 24) % 256);
        *(addr + (dot_len * i) + 16) = ((b >> 8) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 17) = ((b >> 4) & 0xf) | 0x30;
        *(addr + (dot_len * i) + 18) = ((b >> 0) & 0xf) | 0x30;
    }
}
