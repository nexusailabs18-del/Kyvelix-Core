#ifndef KYJIT_CORE_PIN_H
#define KYJIT_CORE_PIN_H

int kyjit_pin_thread(int core_id);
int kyjit_pin_thread_to_list(const char *core_list);
int kyjit_get_core_count(void);
int kyjit_pin_p_cores(void);
int kyjit_pin_e_cores(void);

#endif
