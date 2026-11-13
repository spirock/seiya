/* Read-only ABI shim using system SDK structs, not guessed Rust layouts. */
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp_fsm.h>
#include <errno.h>
#include <stdint.h>

/* 1 exact own loopback listener; 0 not yet listening; -1 incomplete/error. */
int seiya_owned_listener(int pid, uint16_t port) {
    struct proc_fdinfo fds[1024];
    if (pid <= 0 || port < 49152) return -1;
    int size = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, sizeof(fds));
    if (size <= 0 || size >= (int)sizeof(fds) || size % sizeof(fds[0])) return -1;
    for (unsigned i = 0; i < (unsigned)size / sizeof(fds[0]); ++i) {
        if (fds[i].proc_fdtype != PROX_FDTYPE_SOCKET) continue;
        struct socket_fdinfo info;
        errno = 0;
        int got = proc_pidfdinfo(pid, fds[i].proc_fd, PROC_PIDFDSOCKETINFO, &info, sizeof(info));
        /* A concurrently closed FD is transient, not evidence of ownership. */
        if (got != sizeof(info)) {
            if (errno == EBADF || errno == ENOENT) continue;
            return -1;
        }
        if (info.psi.soi_family != AF_INET || info.psi.soi_kind != SOCKINFO_TCP) continue;
        struct tcp_sockinfo *tcp = &info.psi.soi_proto.pri_tcp;
        if (tcp->tcpsi_state == TCPS_LISTEN &&
            tcp->tcpsi_ini.insi_lport == htons(port) &&
            tcp->tcpsi_ini.insi_laddr.ina_46.i46a_addr4.s_addr == htonl(INADDR_LOOPBACK)) return 1;
    }
    return 0;
}
