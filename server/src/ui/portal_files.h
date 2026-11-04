#ifndef AXIA_PORTAL_FILES_H
#define AXIA_PORTAL_FILES_H
#include "../vault/dbus.h"
int portal_files_handle(DConn *conn, const DMsg *message);
void portal_files_result(unsigned id, const char *const *uris, unsigned count);
#endif
