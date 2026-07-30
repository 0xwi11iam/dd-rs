#ifndef EXARID_EBCDIC_TABLES_H
#define EXARID_EBCDIC_TABLES_H

#include <stdint.h>

/// Standard EBCDIC (CP037 / IBM037) to ASCII (ISO 8859-1) translation table.
/// Indexed by EBCDIC byte value (0x00–0xFF), yields the ASCII equivalent.
extern const uint8_t EBCDIC_TO_ASCII[256];

/// ASCII to standard EBCDIC (CP037) translation table.
/// Indexed by ASCII byte value (0x00–0xFF), yields the EBCDIC equivalent.
extern const uint8_t ASCII_TO_EBCDIC[256];

/// Alternate EBCDIC (IBM1047, used by `conv=ibm`) to ASCII table.
/// Differs from CP037 in the mapping of ~, [, ], ^, etc.
extern const uint8_t IBM_EBCDIC_TO_ASCII[256];

/// ASCII to alternate EBCDIC (IBM1047) table.
extern const uint8_t ASCII_TO_IBM_EBCDIC[256];

#endif // EXARID_EBCDIC_TABLES_H
