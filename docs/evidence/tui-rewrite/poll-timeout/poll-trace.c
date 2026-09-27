#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>
static int trace_fd = -1;
static int (*system_poll)(struct pollfd *, nfds_t, int);
__attribute__((constructor)) static void initialize(void) {
    system_poll = dlsym(RTLD_NEXT, "poll");
    const char *path = getenv("HARNESS_PROBE_POLL_TRACE");
    if (path) trace_fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
}
int poll(struct pollfd *fds, nfds_t count, int timeout) {
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);
    int result = system_poll(fds, count, timeout);
    int saved = errno;
    clock_gettime(CLOCK_MONOTONIC, &end);
    long long ns = (end.tv_sec - start.tv_sec) * 1000000000LL + end.tv_nsec - start.tv_nsec;
    if (trace_fd >= 0) dprintf(trace_fd, "%d %d %llu %d %lld\n", gettid(), timeout, (unsigned long long)count, result, ns);
    errno = saved;
    return result;
}
