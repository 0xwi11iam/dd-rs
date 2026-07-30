#ifndef EXARID_CONV_HELPERS_H
#define EXARID_CONV_HELPERS_H

#include <stddef.h>
#include <stdint.h>

/// Convert a buffer of EBCDIC bytes to ASCII in-place using CP037 table.
/// Returns the number of bytes converted.
size_t ebcdic_to_ascii(uint8_t *buf, size_t len);

/// Convert a buffer of ASCII bytes to EBCDIC in-place using CP037 table.
size_t ascii_to_ebcdic(uint8_t *buf, size_t len);

/// Convert a buffer of IBM1047 EBCDIC bytes to ASCII in-place.
size_t ibm_ebcdic_to_ascii(uint8_t *buf, size_t len);

/// Convert a buffer of ASCII bytes to IBM1047 EBCDIC in-place.
size_t ascii_to_ibm_ebcdic(uint8_t *buf, size_t len);

/// Swap every pair of bytes in-place. If len is odd, the last byte is unchanged.
size_t swab_bytes(uint8_t *buf, size_t len);

/// Map uppercase A-Z to lowercase a-z in-place. Returns the buffer length.
size_t map_lcase(uint8_t *buf, size_t len);

/// Map lowercase a-z to uppercase A-Z in-place. Returns the buffer length.
size_t map_ucase(uint8_t *buf, size_t len);

#endif // EXARID_CONV_HELPERS_H
