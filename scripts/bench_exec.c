/* Linux RSS sampler. A fresh native parent avoids inheriting the Python
 * harness's peak RSS before exec, which masks small improvements in rich. */
#define _DEFAULT_SOURCE
#include <errno.h>
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 3) return 125;
    pid_t child = fork();
    if (child < 0) { perror("fork"); return 125; }
    if (child == 0) {
        execvp(argv[2], argv + 2);
        perror("execvp");
        _exit(127);
    }
    int status;
    struct rusage usage;
    while (wait4(child, &status, 0, &usage) < 0) {
        if (errno != EINTR) { perror("wait4"); return 125; }
    }
    FILE *out = fopen(argv[1], "w");
    if (!out) { perror("RSS output"); return 125; }
    int written = fprintf(out, "%ld\n", usage.ru_maxrss);
    int closed = fclose(out);
    if (written < 0 || closed != 0) return 125;
    if (WIFEXITED(status)) return WEXITSTATUS(status);
    return WIFSIGNALED(status) ? 128 + WTERMSIG(status) : 125;
}
