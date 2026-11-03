#include "../src/ui/switcher_ffi.h"
#include <assert.h>
#include <stdio.h>
int main(void) {
    AxiaSwitcher *s = axia_switcher_new();
    assert(s);
    const char *keys[] = { "files", "browser", "terminal" };
    axia_switcher_focus(s, "files");
    axia_switcher_focus(s, "terminal");
    axia_switcher_focus(s, "browser");
    axia_switcher_begin(s, keys, 3, "browser", false);
    assert(axia_switcher_len(s) == 3 && axia_switcher_index(s) == 1);
    assert(axia_switcher_source(s, 1) == 2);
    axia_switcher_step(s, -1);
    assert(axia_switcher_index(s) == 0);
    axia_switcher_step(s, -1);
    assert(axia_switcher_index(s) == 2);
    assert(axia_switcher_finish(s, false) == -1);
    assert(axia_switcher_index(s) == -1);
    axia_switcher_begin(s, keys, 3, "browser", false);
    axia_switcher_select(s, 2);
    assert(axia_switcher_finish(s, true) == 2);
    axia_switcher_begin(s, keys, 3, "browser", false);
    axia_switcher_remove(s, "terminal");
    assert(axia_switcher_index(s) == 1 && axia_switcher_len(s) == 2);
    axia_switcher_select(s, 9999);
    assert(axia_switcher_index(s) == 1);
    axia_switcher_begin(s, NULL, 0, "", false);
    assert(axia_switcher_finish(s, true) == -1);
    axia_switcher_free(s);
    axia_switcher_free(NULL);
    puts("switcher C/Rust ABI: ok");
    return 0;
}
