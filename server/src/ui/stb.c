/* stb.c — implementation of the vendored stb libraries (compiled without warnings). */
#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"

#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_PNG
#define STBI_ONLY_JPEG
/* on ARM64 (Raspberry Pi) stb's NEON loops for JPEG; on x86-64 stb already uses SSE2 by itself */
#if defined(__aarch64__) && !defined(STBI_NEON) && !defined(STBI_NO_SIMD)
#define STBI_NEON
#endif
#include "stb_image.h"
