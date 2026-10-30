// TypeThru Virtual Keyboard packet rules, shared by the helper and its fuzz test.
#pragma once
#include <cstdint>

// Byte 0 version 1; byte 1: 0 readiness probe (other bytes ignored), 1 key press;
// byte 2 HID usage 4-56 or 100; byte 3 modifier 0, 2 (left Shift), or 1 (left Control, only
// with usage 5 or 12: Control+B and Control+I for bold and italic);
// bytes 4-5 hold 10-200 ms and 6-7 gap 0-500 ms, little-endian.
inline bool valid(const uint8_t* p) {
    if (p[0] != 1) return false;
    if (p[1] == 0) return true; // readiness probe
    unsigned hold = p[4] | (p[5] << 8), gap = p[6] | (p[7] << 8);
    return p[1] == 1 && ((p[2] >= 4 && p[2] <= 56) || p[2] == 100)
        && (p[3] == 0 || p[3] == 2 || (p[3] == 1 && (p[2] == 5 || p[2] == 12))) && hold >= 10 && hold <= 200 && gap <= 500;
}
