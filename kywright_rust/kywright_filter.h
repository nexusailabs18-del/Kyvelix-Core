#ifndef KYWRIGHT_FILTER_H
#define KYWRIGHT_FILTER_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KywrightSession KywrightSession;

char *filter_html(const char *input);
void kywright_free_string(char *s);
KywrightSession *kywright_session_new(void);
int kywright_session_feed(KywrightSession *handle, const char *chunk);
char *kywright_session_take_output(KywrightSession *handle);
char *kywright_session_finish(KywrightSession *handle);
void kywright_session_free(KywrightSession *handle);

#ifdef __cplusplus
}
#endif

#endif /* KYWRIGHT_FILTER_H */
