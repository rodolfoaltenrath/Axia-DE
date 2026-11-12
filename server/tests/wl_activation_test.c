/* Actual activation handlers, token lifetime and event delivery, with no desktop/GPU. */
#include "../src/wl/wl.c"
#include <assert.h>
#include <sys/socket.h>
int main(void) {
    int fd[2];
    assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, fd) == 0);
    dpy = wl_display_create();
    struct wl_client *client = wl_client_create(dpy, fd[0]);
    int target_fd[2];
    assert(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, target_fd) == 0);
    struct wl_client *receiver = wl_client_create(dpy, target_fd[0]);
    Surf source = {.id=1,.client=client,.role=R_TOPLEVEL,.mapped=1};
    Surf target = {.id=2,.client=receiver,.role=R_TOPLEVEL,.mapped=1};
    surfs[0]=&source; surfs[1]=&target; key_focus=&source;
    struct wl_resource *surface = wl_resource_create(receiver, &wl_surface_interface, 6, 0);
    wl_resource_set_user_data(surface, &target);
    struct wl_resource *res = wl_resource_create(client, &xdg_activation_token_v1_interface, 1, 0);
    Activation *a = calloc(1,sizeof *a);
    wl_resource_set_implementation(res, &activation_token_impl, a, activation_gone);
    activation_commit(client,res);
    char token[65]; memcpy(token,activation_tokens[0].token,sizeof token);
    assert(strlen(token)==64);
    wl_resource_destroy(res); /* exported token survives its protocol object */
    activation_activate(receiver,NULL,token,surface);
    assert(nev==1 && !strcmp(evq[0].type,"activate") && evq[0].id==2);
    activation_activate(receiver,NULL,token,surface);
    assert(nev==1); /* cannot replay */
    strcpy(activation_tokens[1].token,"expired");
    activation_tokens[1].created=now_ms()-ACTIVATION_TTL-1;
    activation_activate(client,NULL,"expired",surface);
    activation_activate(client,NULL,"unknown",surface);
    assert(nev==1);
    strcpy(activation_tokens[2].token,"unmapped");activation_tokens[2].created=now_ms();
    target.mapped=0;
    activation_activate(client,NULL,"unmapped",surface);
    assert(target.activation_pending && nev==1);
    key_focus=NULL;
    res=wl_resource_create(client,&xdg_activation_token_v1_interface,1,0);
    a=calloc(1,sizeof *a);
    wl_resource_set_implementation(res,&activation_token_impl,a,activation_gone);
    unsigned slot=activation_slot;
    activation_commit(client,res);
    assert(activation_slot==slot); /* background client gets no usable token */
    activation_record(client,100,now_ms(),1);
    activation_record(client,101,now_ms(),0);
    assert(activation_serial_valid(client,100));
    assert(activation_serial_valid(client,101)); /* click handlers often use the release serial */
    assert(!activation_serial_valid(receiver,101));
    assert(!activation_serial_valid(client,999));
    activation_input_client=client;activation_input_time=now_ms();
    assert(wl_activation_timestamp_valid(activation_input_time));
    assert(!wl_activation_timestamp_valid(0));
    assert(!wl_activation_timestamp_valid(activation_input_time-1));
    assert(!wl_activation_timestamp_valid(activation_input_time-10001));
    wl_display_destroy_clients(dpy);wl_display_destroy(dpy);close(fd[1]);close(target_fd[1]);
    puts("activation: focus transfer, token destruction/replay/expiry, background denial and timestamps passed");
    return 0;
}
