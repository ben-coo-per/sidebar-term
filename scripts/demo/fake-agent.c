// Fake coding agent for sidebar-term detection tests. Behaviour depends on argv[0]'s basename.
#include <stdio.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
  const char *name = strrchr(argv[0], '/'); name = name ? name + 1 : argv[0];
  setvbuf(stdout, NULL, _IONBF, 0);
  if (strcmp(name, "codex") == 0) {
    const char *spin[] = {"⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧"};
    for (int i = 0; i < 30; i++) { printf("\033]0;%s doorstep\007", spin[i % 8]); usleep(200000); }
    printf("\033]0;[ ! ] Action Required\007Allow command? (y/n) ");
  } else {
    for (int i = 0; i < 30; i++) { printf("thinking... step %d\r\n", i); usleep(200000); }
    printf("\007Waiting for your input > ");
  }
  sleep(600);
  return 0;
}
