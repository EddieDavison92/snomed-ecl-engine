#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unicode/ucol.h>
#include <unicode/usearch.h>
#include <unicode/ubrk.h>

static const UChar EMPTY_TARGET[] = {0x61};

typedef struct {
    UChar *pattern;
    UCollator *collator;
    UStringSearch *search;
    UBreakIterator *words;
} SnomedSearch;

void snomed_search_close(SnomedSearch *state) {
    if (!state) return;
    if (state->search) usearch_close(state->search);
    if (state->words) ubrk_close(state->words);
    if (state->collator) ucol_close(state->collator);
    free(state->pattern);
    free(state);
}

SnomedSearch *snomed_search_open(const uint16_t *pattern, int32_t length,
                                const char *locale, int32_t *result) {
    UErrorCode error = U_ZERO_ERROR;
    SnomedSearch *state = calloc(1, sizeof(*state));
    if (!state) { *result = U_MEMORY_ALLOCATION_ERROR; return NULL; }
    if (length <= 0) { *result = U_ILLEGAL_ARGUMENT_ERROR; snomed_search_close(state); return NULL; }
    state->pattern = malloc((size_t)length * sizeof(UChar));
    if (!state->pattern) { *result = U_MEMORY_ALLOCATION_ERROR; snomed_search_close(state); return NULL; }
    memcpy(state->pattern, pattern, (size_t)length * sizeof(UChar));
    state->collator = ucol_open(locale, &error);
    if (U_SUCCESS(error)) {
        ucol_setStrength(state->collator, UCOL_SECONDARY);
        ucol_setAttribute(state->collator, UCOL_NORMALIZATION_MODE, UCOL_ON, &error);
        state->search = usearch_openFromCollator(state->pattern, length, EMPTY_TARGET, 1,
                                               state->collator, NULL, &error);
    }
    if (U_SUCCESS(error)) {
        usearch_setAttribute(state->search, USEARCH_ELEMENT_COMPARISON,
                             USEARCH_PATTERN_BASE_WEIGHT_IS_WILDCARD, &error);
        usearch_setAttribute(state->search, USEARCH_OVERLAP, USEARCH_ON, &error);
        state->words = ubrk_open(UBRK_WORD, locale, EMPTY_TARGET, 1, &error);
    }
    *result = error;
    if (U_FAILURE(error)) { snomed_search_close(state); return NULL; }
    return state;
}

/* The caller's text is borrowed only for this call. Flags select start, end or word-start anchoring. */
int32_t snomed_search_find(SnomedSearch *state, const uint16_t *text, int32_t length,
                          int32_t minimum, int32_t flags, int32_t *limit, int32_t *result) {
    UErrorCode error = U_ZERO_ERROR;
    int32_t found = USEARCH_DONE;
    if (!state || length <= 0 || minimum < 0 || minimum > length) {
        *result = length == 0 ? U_ZERO_ERROR : U_ILLEGAL_ARGUMENT_ERROR;
        return found;
    }
    usearch_setText(state->search, text, length, &error);
    if (flags & 4) ubrk_setText(state->words, text, length, &error);
    for (int32_t start = usearch_following(state->search, minimum, &error);
         U_SUCCESS(error) && start != USEARCH_DONE;
         start = usearch_next(state->search, &error)) {
        int32_t end = start + usearch_getMatchedLength(state->search);
        if ((flags & 1) && start != 0) break;
        if ((flags & 2) && end != length) continue;
        if (flags & 4) {
            if (!ubrk_isBoundary(state->words, start)) continue;
            ubrk_following(state->words, start);
            if (ubrk_getRuleStatus(state->words) < UBRK_WORD_NUMBER) continue;
        }
        found = start;
        *limit = end;
        break;
    }
    *result = error;
    /* Neither ICU object may retain the caller's buffer after return. */
    UErrorCode reset = U_ZERO_ERROR;
    usearch_setText(state->search, EMPTY_TARGET, 1, &reset);
    ubrk_setText(state->words, EMPTY_TARGET, 1, &reset);
    if (U_FAILURE(reset)) *result = reset;
    return found;
}
