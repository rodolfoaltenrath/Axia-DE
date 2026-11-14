# Files → Network (docs/FILES.md, "Network"): axia-lan, the sandboxed helper that reads what the local
# network answers (mDNS, SMB2, ONC RPC; the packet parsers, the address policy and rconnect's checks are
# rust/axia-codecs/src/{lanparse,mdns,lanaddr,rconnect}.rs),
# and axia-server's side (src/lan: one bounded pass on a thread,
# lord.lan in ui/js_lan.c, the share-* actions). Our own code, no libsmbclient or libnfs.
LAN_SRV_SRC := src/lan/lan.c src/lan/rconnect.c src/lan/rconnect_run.c src/ui/js_lan.c
LAN_SRV_OBJ := $(LAN_SRV_SRC:src/%.c=build/obj/%.o)
$(LAN_SRV_OBJ): $(wildcard src/lan/*.h) src/doc/wire.h src/print/mdns.h src/sec/json.h src/ui/js_lan.h src/admin.h src/errcode.h
build/axia-server: $(LAN_SRV_OBJ)
build/obj/ui/js.o: src/ui/js_lan.h

LAN_HELPER_SRC := src/lan/lanhelper.c src/lan/smbauth.c src/pkg/crypto.c src/doc/wire.c src/sec/sandbox.c src/sec/cgroup.c src/sec/json.c
all: build/axia-lan
build/axia-lan: $(LAN_HELPER_SRC) $(wildcard src/lan/*.h) src/pkg/crypto.h src/doc/wire.h src/doc/proto.h src/print/mdns.h src/sec/sandbox.h src/sec/cgroup.h $(CODEC_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $(LAN_HELPER_SRC) $(CODEC_LIB) -lpthread -lm $(CODEC_NATIVE_LIBS)
	chmod g-w $@

# the parsers against crafted and damaged answers, the address policy, the actions' arguments
test: build/lan-test
LAN_TEST_SRC := tests/lan_test.c src/lan/smbauth.c src/pkg/crypto.c
build/lan-test: $(LAN_TEST_SRC) $(wildcard src/lan/*.h) src/print/mdns.h $(CODEC_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) -fsanitize=address,undefined -fno-sanitize-recover=undefined $(WARN) -std=c11 -o $@ $(LAN_TEST_SRC) $(CODEC_LIB) -lm $(CODEC_NATIVE_LIBS)
	./$@ || { rm -f $@; exit 1; }

# the programs rconnect runs: deadline, read-only system, no TCP when offline
test: build/rconnect-run-test
RCONNECT_RUN_SRC := tests/rconnect_run_test.c src/lan/rconnect_run.c src/sec/sandbox.c src/sec/cgroup.c
build/rconnect-run-test: $(RCONNECT_RUN_SRC) src/lan/rconnect.h src/sec/sandbox.h src/sec/cgroup.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $(RCONNECT_RUN_SRC) -lpthread -lm
	./$@ || { rm -f $@; exit 1; }

# one real pass on the network this machine is on, printed as JSON (a tool, not part of make test):
#   build/lan-scan   (not in a LordOS session: there are no sockets there without a connector). Built with
#   LAN_ANY_LINK, so a test server behind a veth on an isolated network namespace counts as a link.
build/lan-scan: tests/lan_scan.c src/lan/lan.c src/doc/wire.c src/sec/json.c src/sec/netconn.c src/sec/cgroup.c build/axia-lan $(CODEC_LIB)
	$(CC) $(CPPFLAGS) -DLAN_ANY_LINK $(CFLAGS) $(WARN) -std=c11 -o $@ tests/lan_scan.c src/lan/lan.c src/doc/wire.c src/sec/json.c src/sec/netconn.c src/sec/cgroup.c src/fw/rules_json.c src/fw/fwmap.c $(CODEC_LIB) -lpthread -lm $(CODEC_NATIVE_LIBS)

# the share-* actions alone, real mounts as root on a test machine (a tool, not part of make test)
build/share-cli: tests/share_cli.c src/lan/share_admin.c src/lan/share_admin.h src/lan/lanaddr.h $(CODEC_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/share_cli.c src/lan/share_admin.c $(CODEC_LIB) -lm $(CODEC_NATIVE_LIBS)

# axia-lan's sign-in against one test server (a tool, not part of make test)
build/lan-list: tests/lan_list.c src/doc/wire.c build/axia-lan
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/lan_list.c src/doc/wire.c

