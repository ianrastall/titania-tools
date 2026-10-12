/* titania_core.dll - the native core of the Titania chess tools.
 *
 * Everything goes through tt_run: a JSON request in, a JSON document out. All text is UTF-8.
 *
 * Requests
 *   {"op":"report","input":PATH}
 *   {"op":"join",  "inputs":[PATH...],"output":PATH}
 *   {"op":"merge", "inputs":[PATH...],"output":PATH,"remove_duplicates":BOOL,"keys":[KEY...]}
 *   {"op":"sort",  "input":PATH,"output":PATH,"keys":[KEY...]}
 *   {"op":"split", "input":PATH,"output_dir":PATH,"mode":MODE,"count":N}
 *
 *   KEY   {"field":FIELD,"descending":BOOL}
 *   FIELD "date" "event" "site" "round" "white" "black" "result" "eco"
 *         "white_elo" "black_elo" "average_elo" "moves"
 *   MODE  "games" (N games per file)  "parts" (N files)
 *         "year" "event" "eco" "result" "white" "black"
 *
 * Results are described by the Serialize structs in core/titania-pgn/src.
 * A failed call returns {"error":"..."} instead.
 */
#ifndef TITANIA_CORE_H
#define TITANIA_CORE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TT_ABI_VERSION 1

/* Return values of tt_run. */
#define TT_OK          0
#define TT_CANCELLED   1
#define TT_ERROR       2
#define TT_BAD_REQUEST 3
#define TT_PANIC       4

/* Events passed to the callback. */
#define TT_EVENT_PROGRESS 0 /* done, total, games are set; total == 0 means "unknown amount" */
#define TT_EVENT_PHASE    1 /* text names the phase that is starting */
#define TT_EVENT_LOG      2 /* text is a line for the log */

/* Called on the thread that called tt_run. text is not NUL-terminated and is only valid
 * during the call. Return non-zero to cancel the operation. */
typedef int32_t (*tt_callback)(void *context, int32_t event, uint64_t done, uint64_t total,
                               uint64_t games, const uint8_t *text, size_t text_len);

uint32_t tt_abi_version(void);

/* NUL-terminated, valid for as long as the library is loaded. */
const uint8_t *tt_version(void);

/* Runs a request. *output receives a JSON document of *output_len bytes, which must be
 * released with tt_free whatever the return value. callback may be NULL. */
int32_t tt_run(const uint8_t *request, size_t request_len, tt_callback callback, void *context,
               uint8_t **output, size_t *output_len);

void tt_free(uint8_t *data, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* TITANIA_CORE_H */
