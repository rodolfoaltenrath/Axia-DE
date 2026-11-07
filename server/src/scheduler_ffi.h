#ifndef AXIA_SCHEDULER_FFI_H
#define AXIA_SCHEDULER_FFI_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
typedef struct AxiaScheduler AxiaScheduler;
AxiaScheduler *axia_scheduler_new(void);
void axia_scheduler_free(AxiaScheduler *);
uint64_t axia_scheduler_add(AxiaScheduler *, double now, double delay, bool repeating);
void axia_scheduler_cancel(AxiaScheduler *, uint64_t id);
size_t axia_scheduler_due(const AxiaScheduler *, double now, uint64_t *out, size_t capacity);
int32_t axia_scheduler_fire(AxiaScheduler *, uint64_t id, double now);
double axia_scheduler_wait(const AxiaScheduler *, double now);
uint64_t axia_scheduler_frame(AxiaScheduler *);
void axia_scheduler_cancel_frame(AxiaScheduler *, uint64_t id);
size_t axia_scheduler_frame_count(const AxiaScheduler *);
size_t axia_scheduler_take_frames(AxiaScheduler *, uint64_t *out, size_t capacity);
double axia_scheduler_frame_wait(const AxiaScheduler *, double now, bool waiting, uint32_t presentation);
bool axia_scheduler_frame_ready(AxiaScheduler *, double now, bool waiting, uint32_t presentation);
void axia_scheduler_reset(AxiaScheduler *);
bool axia_scheduler_changed(AxiaScheduler *);
#endif
