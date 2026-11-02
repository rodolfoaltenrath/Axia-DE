/* js_vault.h — lord.vault: Contraseñas y el llavero por el ayudante lord-vault (ver js_vault.c). */
#ifndef LORD_JS_VAULT_H
#define LORD_JS_VAULT_H

#include "quickjs.h"

void js_vault_register(JSContext *c, JSValue lord);

/* App process (lord-server --app): lord.vault on the app's own channel to lord-vault (LORD_VAULT_FD). */
void js_vault_register_app(JSContext *c, JSValue lord);

#endif
