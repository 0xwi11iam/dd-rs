/// Lightweight C conversion helpers.
/// These are compiled to a static library and linked into the Rust binary.
/// The table-driven conversions live here so they benefit from C-level
/// optimizations; the orchestration lives in Rust.

#include "conv_helpers.h"
#include "ebcdic_tables.h"

size_t ebcdic_to_ascii(uint8_t *buf, size_t len) {
    for (size_t i = 0; i < len; i++) {
        buf[i] = EBCDIC_TO_ASCII[buf[i]];
    }
    return len;
}

size_t ascii_to_ebcdic(uint8_t *buf, size_t len) {
    for (size_t i = 0; i < len; i++) {
        buf[i] = ASCII_TO_EBCDIC[buf[i]];
    }
    return len;
}

size_t ibm_ebcdic_to_ascii(uint8_t *buf, size_t len) {
    for (size_t i = 0; i < len; i++) {
        buf[i] = IBM_EBCDIC_TO_ASCII[buf[i]];
    }
    return len;
}

size_t ascii_to_ibm_ebcdic(uint8_t *buf, size_t len) {
    for (size_t i = 0; i < len; i++) {
        buf[i] = ASCII_TO_IBM_EBCDIC[buf[i]];
    }
    return len;
}

size_t swab_bytes(uint8_t *buf, size_t len) {
    size_t end = len & ~(size_t)1; // round down to even
    for (size_t i = 0; i < end; i += 2) {
        uint8_t tmp = buf[i];
        buf[i] = buf[i + 1];
        buf[i + 1] = tmp;
    }
    return len;
}

size_t map_lcase(uint8_t *buf, size_t len) {
    for (size_t i = 0; i < len; i++) {
        if (buf[i] >= 'A' && buf[i] <= 'Z') {
            buf[i] += 32; // 'a' - 'A'
        }
    }
    return len;
}

size_t map_ucase(uint8_t *buf, size_t len) {
    for (size_t i = 0; i < len; i++) {
        if (buf[i] >= 'a' && buf[i] <= 'z') {
            buf[i] -= 32; // 'A' - 'a'
        }
    }
    return len;
}
