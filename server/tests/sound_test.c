/* sound_test.c — PulseAudio protocol serialization (sound/pulse.c) without a server:
 * round trip of each type, property lists, descriptors and truncated data. */
#include "../src/sound/pulse.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #c); fails++; } } while (0)

static int props_seen;
static void on_prop(void *ud, const char *key, const char *val, uint32_t len) {
    (void)ud;
    if (!strcmp(key, "application.name")) CHECK(!strcmp(val, "LordOS") && len == 6);
    if (!strcmp(key, "media.role")) CHECK(!strcmp(val, "event"));
    props_seen++;
}

int main(void) {
    TsOut o;
    ts_init(&o);
    ts_u32(&o, 0xdeadbeef);
    ts_u8(&o, 7);
    ts_u64(&o, 0x0102030405060708ull);
    ts_bool(&o, 1);
    ts_bool(&o, 0);
    ts_str(&o, "hola");
    ts_str(&o, NULL);
    ts_arb(&o, "\x01\x02\x03", 3);
    ts_ss(&o, PA_SAMPLE_FLOAT32LE, 2, 48000);
    uint8_t map[2] = { 1, 2 };
    ts_cmap(&o, 2, map);
    uint32_t vol[2] = { PA_VOLUME_NORM, PA_VOLUME_NORM / 2 };
    ts_cvol(&o, 2, vol);
    const char *kv[] = { "application.name", "LordOS", "media.role", "event" };
    ts_proplist(&o, kv, 2);
    ts_format_pcm(&o);
    size_t n = ts_finish(&o);
    CHECK(n == o.len && !o.err);

    uint32_t len, ch, flags;
    uint64_t off;
    pa_descriptor_read(o.d, &len, &ch, &off, &flags);
    CHECK(len == n - PA_DESCRIPTOR_SIZE);
    CHECK(ch == 0xffffffffu && off == 0 && flags == 0);

    TsIn t;
    ts_in(&t, o.d + PA_DESCRIPTOR_SIZE, len);
    CHECK(ts_get_u32(&t) == 0xdeadbeef);
    CHECK(ts_get_u8(&t) == 7);
    CHECK(ts_get_u64(&t) == 0x0102030405060708ull);
    CHECK(ts_get_bool(&t) == 1);
    CHECK(ts_get_bool(&t) == 0);
    const char *s = ts_get_str(&t);
    CHECK(s && !strcmp(s, "hola"));
    CHECK(ts_get_str(&t) == NULL && !t.err);
    uint32_t an;
    const uint8_t *a = ts_get_arb(&t, &an);
    CHECK(an == 3 && a && a[2] == 3);
    uint8_t f, c; uint32_t r;
    ts_get_ss(&t, &f, &c, &r);
    CHECK(f == PA_SAMPLE_FLOAT32LE && c == 2 && r == 48000);
    uint8_t m2[PA_CHANNELS_MAX];
    CHECK(ts_get_cmap(&t, m2) == 2 && m2[1] == 2);
    uint32_t v2[PA_CHANNELS_MAX];
    CHECK(ts_get_cvol(&t, v2) == 2 && v2[0] == PA_VOLUME_NORM && v2[1] == PA_VOLUME_NORM / 2);
    ts_get_proplist(&t, on_prop, NULL);
    CHECK(props_seen == 2 && !t.err);
    ts_skip_format(&t);
    CHECK(!t.err && ts_eof(&t));

    /* ts_skip walks everything the same way as the typed read */
    ts_in(&t, o.d + PA_DESCRIPTOR_SIZE, len);
    int k = 0;
    while (!ts_eof(&t) && !t.err) { ts_skip(&t); k++; }
    CHECK(!t.err && k == 13);

    /* truncated: every prefix fails without reading outside the buffer */
    int errs = 0;
    for (size_t cut = 0; cut < len; cut++) {
        ts_in(&t, o.d + PA_DESCRIPTOR_SIZE, cut);
        while (!ts_eof(&t) && !t.err) ts_skip(&t);
        if (t.err) errs++;
    }
    CHECK(errs > 0);

    /* tipo equivocado */
    ts_in(&t, o.d + PA_DESCRIPTOR_SIZE, len);
    ts_get_str(&t);
    CHECK(t.err);

    /* data packet */
    size_t total;
    uint8_t *p = ts_data_packet(5, "abcd", 4, &total);
    pa_descriptor_read(p, &len, &ch, &off, &flags);
    CHECK(total == PA_DESCRIPTOR_SIZE + 4 && len == 4 && ch == 5 && !memcmp(p + PA_DESCRIPTOR_SIZE, "abcd", 4));
    free(p);
    ts_free(&o);

    printf("%s (%d failures)\n", fails ? "sound-test: FAIL" : "sound-test: ok", fails);
    return fails != 0;
}
