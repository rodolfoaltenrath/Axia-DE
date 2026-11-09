/* The C/Rust contract of the window manager's pure rules (src/ui/winmodel_ffi.h). The rules are
 * tested in Rust (rust/axia-shell/src/winmodel/tests.rs); this checks the call and its buffers. */
#include "../src/ui/winmodel_ffi.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static int run(const char *op, const char *json, char **out) { return axia_winmodel_call(op, json, strlen(json), out); }

int main(void) {
    char *out = NULL;
    assert(run("desk.add", "[{\"list\":[1,3],\"active\":1}]", &out) == 0);
    assert(strstr(out, "\"id\":4"));
    axia_winmodel_free(out);
    assert(run("desk.neighbor", "[[1,2],2,1]", &out) == 0 && !strcmp(out, "null"));
    axia_winmodel_free(out);
    assert(run("nothing", "[]", &out) == -1 && out && strstr(out, "unknown"));
    axia_winmodel_free(out);
    assert(run("desk.add", "{broken", &out) == -1 && out);
    axia_winmodel_free(out);
    assert(axia_winmodel_call(NULL, "[]", 2, &out) == -1);
    axia_winmodel_free(out);
    assert(axia_winmodel_call("desk.add", "[]", 2, NULL) == -1);
    axia_winmodel_free(NULL);
    puts("winmodel C/Rust ABI: ok");
    return 0;
}
