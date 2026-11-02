/* fido_e2e.c — the CTAP2 client end to end against the software authenticator
 * (../tests/fido-soft/softfido.c, over uhid). tests/fido-soft/run-pw-116.sh runs it with sudo.
 * It only tests against softfido: a real key (YubiKey, SoloKey, Nitrokey) is NOT tested here. */
#include "vault/ctap2.h"
#include "vault/fidodev.h"

#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static int fails, total;
static void check(int ok, const char *what) { total++; printf("%s %s\n", ok ? "ok   " : "FAIL ", what); if (!ok) fails++; }

static int touches;
static void on_status(void *ud, int st) { (void)ud; if (st == FIDO_ST_TOUCH) touches++; }

static long long now_ms(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return (long long)t.tv_sec * 1000 + t.tv_nsec / 1000000; }

static int set_mode(int fd, uint8_t m) {
    uint8_t r[8];
    size_t n = 0;
    return ctaphid_command(fd, 0xc1, &m, 1, r, sizeof r, &n, 2000);
}

static int cancel_fd = -1;
static void *canceller(void *a) { (void)a; usleep(500000); fido_cancel(cancel_fd); return NULL; }

int main(void) {
    FidoDev devs[8];
    int n = fidodev_list(devs, 8), soft = -1;
    for (int i = 0; i < n; i++) {
        printf("key: %s «%s» %04x:%04x\n", devs[i].path, devs[i].name, devs[i].vid, devs[i].pid);
        if (strstr(devs[i].name, "LordOS Soft FIDO2")) soft = i;
    }
    check(soft >= 0, "fidodev_list finds the software authenticator");
    if (soft < 0) return 1;
    check(devs[soft].vid == 0x1209 && devs[soft].pid == 0x5f1d, "fidodev: vid:pid from uevent");
    int fd = fidodev_open(devs[soft].path);
    check(fd >= 0, "fidodev_open");
    if (fd < 0) return 1;

    FidoInfo info;
    check(fido_get_info(fd, &info) == 0, "getInfo");
    check(info.hmac_secret == 1 && info.client_pin == 0, "getInfo: hmac-secret, no PIN");

    uint8_t cred[FIDO_CRED_MAX], cred2[FIDO_CRED_MAX];
    size_t cl = 0, cl2 = 0;
    touches = 0;
    int rc = fido_make_credential(fd, NULL, cred, &cl, 10000, on_status, NULL);
    check(rc == 0 && cl > 0, "makeCredential with hmac-secret");
    check(touches > 0, "makeCredential: asked to «touch the key»");

    uint8_t saltA[32], saltB[32], o1[32], o2[32], o3[32];
    for (int i = 0; i < 32; i++) { saltA[i] = (uint8_t)i; saltB[i] = (uint8_t)(255 - i); }
    long long t0 = now_ms();
    rc = fido_hmac_secret(fd, NULL, cred, cl, saltA, o1, 10000, on_status, NULL);
    printf("hmac-secret: %lld ms (with 3 simulated 100 ms «touches»)\n", now_ms() - t0);
    check(rc == 0, "hmac-secret salt A");
    check(fido_hmac_secret(fd, NULL, cred, cl, saltA, o2, 10000, NULL, NULL) == 0 && !memcmp(o1, o2, 32), "hmac-secret: same salt → same secret");
    check(fido_hmac_secret(fd, NULL, cred, cl, saltB, o3, 10000, NULL, NULL) == 0 && memcmp(o1, o3, 32), "hmac-secret: another salt → another secret");

    check(fido_make_credential(fd, NULL, cred2, &cl2, 10000, NULL, NULL) == 0, "second credential");
    check(fido_hmac_secret(fd, NULL, cred2, cl2, saltA, o2, 10000, NULL, NULL) == 0 && memcmp(o1, o2, 32), "another credential → another secret");

    uint8_t bad[FIDO_CRED_MAX];
    memcpy(bad, cred, cl);
    bad[cl / 2] ^= 0x40;
    rc = fido_hmac_secret(fd, NULL, bad, cl, saltA, o2, 10000, NULL, NULL);
    check(rc == FIDO_ERR_NO_CREDENTIAL, "foreign credential → FIDO_ERR_NO_CREDENTIAL");

    check(set_mode(fd, 1) == 0, "mode: nobody touches");
    t0 = now_ms();
    rc = fido_hmac_secret(fd, NULL, cred, cl, saltA, o2, 1200, NULL, NULL);
    long long dt = now_ms() - t0;
    check(rc == FIDO_ERR_TIMEOUT && dt >= 1100 && dt < 3500, "no touch → FIDO_ERR_TIMEOUT on time");
    printf("  (took %lld ms with a 1200 ms deadline)\n", dt);

    cancel_fd = fd;
    pthread_t th;
    pthread_create(&th, NULL, canceller, NULL);
    t0 = now_ms();
    rc = fido_hmac_secret(fd, NULL, cred, cl, saltA, o2, 10000, NULL, NULL);
    pthread_join(th, NULL);
    check(rc == FIDO_ERR_CANCELLED && now_ms() - t0 < 3000, "fido_cancel from another thread → FIDO_ERR_CANCELLED");

    rc = set_mode(fd, 2);
    if (rc) printf("  set_mode: %s\n", fido_strerror(rc));
    check(rc == 0, "mode: the user declines");
    check(fido_hmac_secret(fd, NULL, cred, cl, saltA, o2, 5000, NULL, NULL) == FIDO_ERR_DENIED, "declined → FIDO_ERR_DENIED");
    set_mode(fd, 0);
    check(fido_hmac_secret(fd, NULL, cred, cl, saltA, o2, 5000, NULL, NULL) == 0 && !memcmp(o1, o2, 32), "after all that, it gives the same secret again");

    /* PIN */
    check(fido_set_pin(fd, "12") == FIDO_ERR_PIN_INVALID, "setPIN: too short is rejected (client)");
    check(fido_set_pin(fd, "1234") == 0, "setPIN 1234");
    check(fido_get_info(fd, &info) == 0 && info.client_pin == 1 && info.pin_retries == 8, "getInfo: with PIN, 8 retries");
    check(fido_set_pin(fd, "5678") == FIDO_ERR_DENIED, "setPIN again → denied (already set)");
    check(fido_make_credential(fd, NULL, cred2, &cl2, 5000, NULL, NULL) == FIDO_ERR_PIN_REQUIRED, "makeCredential without PIN → FIDO_ERR_PIN_REQUIRED");
    check(fido_make_credential(fd, "0000", cred2, &cl2, 5000, NULL, NULL) == FIDO_ERR_PIN_INVALID, "wrong PIN → FIDO_ERR_PIN_INVALID");
    check(fido_get_info(fd, &info) == 0 && info.pin_retries == 7, "one retry fewer");
    check(fido_make_credential(fd, "1234", cred2, &cl2, 5000, NULL, NULL) == 0, "makeCredential with PIN");
    check(fido_get_info(fd, &info) == 0 && info.pin_retries == 8, "correct PIN: retries back to 8");
    check(fido_hmac_secret(fd, "1234", cred2, cl2, saltA, o2, 5000, NULL, NULL) == 0, "hmac-secret with PIN (UV)");
    check(fido_hmac_secret(fd, NULL, cred2, cl2, saltA, o3, 5000, NULL, NULL) == 0, "hmac-secret of the same credential without PIN (touch only)");
    printf("  (softfido is CTAP 2.0: with and without UV they give %s)\n", memcmp(o2, o3, 32) ? "different results" : "the same");
    check(fido_hmac_secret(fd, NULL, cred, cl, saltA, o2, 5000, NULL, NULL) == 0 && !memcmp(o1, o2, 32), "the credential from before the PIN still gives the same");
    check(fido_hmac_secret(fd, "9999", cred, cl, saltA, o2, 5000, NULL, NULL) == FIDO_ERR_PIN_INVALID, "hmac-secret with a wrong PIN → FIDO_ERR_PIN_INVALID");

    close(fd);
    printf("fido-e2e: %d/%d\n", total - fails, total);
    return fails ? 1 : 0;
}
