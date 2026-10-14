#include "lzrw.h"
#include <string.h>

void fast_copy(void *src_adr, void *dst_adr, unsigned long src_len) {
    memcpy(dst_adr, src_adr, src_len);
}
