/* Exercise the real hardware tick + sysfs reader with no kernel uevents. */
#include "../src/hw/power.h"
static int fake_power_read(const char *, PowerState *);
#define power_read fake_power_read
#include "../src/hw/hw.c"
#undef power_read

#include <assert.h>
#include <sys/stat.h>
#include <unistd.h>

static char root[] = "/tmp/axia-battery-XXXXXX";
static int reads, events;
static char latest[4096];
static int fake_power_read(const char *ignored, PowerState *ps) {
    (void)ignored; reads++;
    return power_read(root, ps);
}
static void event(const char *kind, const char *json) {
    assert(!strcmp(kind, "power"));
    events++;
    snprintf(latest, sizeof latest, "%s", json);
}
static void put(const char *name, const char *value) {
    char path[256];
    snprintf(path, sizeof path, "%s/BAT0/%s", root, name);
    FILE *f = fopen(path, "w"); assert(f);
    assert(fputs(value, f) >= 0); assert(!fclose(f));
}
int main(void) {
    assert(mkdtemp(root));
    char path[256];
    snprintf(path, sizeof path, "%s/BAT0", root);
    assert(!mkdir(path, 0700));
    put("type", "Battery\n"); put("status", "Discharging\n"); put("capacity", "70\n");
    DisplayHost fake = { .event = event };
    host = &fake; started = 1;
    power_changed();
    assert(reads == 1 && events == 1 && hw_power()->percent == 70);
    assert(axia_battery_due(0, true, true));
    assert(hw_timeout(0) == 10000);
    put("capacity", "69\n");
    for (int ms = 1; ms < 10000; ms++) hw_tick(ms);
    assert(reads == 1 && hw_power()->percent == 70);
    hw_tick(10000);
    assert(reads == 2 && events == 2 && hw_power()->percent == 69);
    assert(strstr(latest, "\"percent\":69"));
    assert(hw_timeout(10000) == 10000);
    hw_tick(20000);
    assert(reads == 3 && events == 2); // no redraw/event for an unchanged snapshot
    put("capacity", "71\n"); put("status", "Charging\n");
    axia_battery_due(20001, true, true); power_changed();
    assert(hw_power()->charging && hw_power()->percent == 71 && events == 3);
    hw_tick(20002); assert(reads == 4);
    const char *files[] = { "type", "status", "capacity" };
    for (unsigned i = 0; i < sizeof files / sizeof files[0]; i++) {
        snprintf(path, sizeof path, "%s/BAT0/%s", root, files[i]); assert(!unlink(path));
    }
    snprintf(path, sizeof path, "%s/BAT0", root); assert(!rmdir(path)); assert(!rmdir(root));
    puts("battery-refresh: real sysfs percentage changes without uevents, immediate charger refresh and deduplication passed");
}
