#include "tree_sitter/parser.h"
#include <stdbool.h>
#include <stdlib.h>

enum TokenType {
  NARRATIVE_KW,
  META_KW,
  GIVEN_STORIES_KW,
  SCENARIO_KW,
  LIFECYCLE_KW,
  BEFORE_KW,
  AFTER_KW,
  SCOPE_KW,
  GIVEN_KW,
  WHEN_KW,
  THEN_KW,
  AND_KW,
  EXAMPLES_KW,
  EOL,
};

typedef struct {
  const char *text;
  enum TokenType type;
  bool needs_colon;
} Keyword;

// Keyword text and required-colon flag come directly from jbehave-core
// 3.10's bundled `i18n/keywords_*.properties` (one row per locale variant
// that differs from the others; identical words across locales are listed
// once). JBehave doesn't tag which locale a given .story file uses (the
// `!-- language: xx` comment is a hint for the *Java* story loader, not
// something this scanner reads), so all locales' keywords are recognized
// simultaneously — verified there are no cross-category collisions (e.g. no
// word means GIVEN in one language and THEN in another).
//
// needs_colon is per-row, not per-category, because it isn't always
// consistent within a category: Norwegian's GivenStories ("Gitt stories")
// has no trailing colon while every other locale's does, and conversely
// Turkish's Given/When/And *do* have trailing colons even though no other
// locale's step keywords do. When needs_colon is true the scanner only
// peeks at the colon (doesn't consume it) so the grammar's own literal ":"
// (for section keywords) still matches it; for the Turkish step-keyword
// case there's no separate ":" in the `step` rule, so that colon is simply
// left for `step_text` to absorb as ordinary text.
static const Keyword KEYWORDS[] = {
  // Narrative
  {"ANLATI", NARRATIVE_KW, true},
  {"Erzählung", NARRATIVE_KW, true},
  {"Fortelling", NARRATIVE_KW, true},
  {"Narracja", NARRATIVE_KW, true},
  {"Narrativa", NARRATIVE_KW, true},
  {"Narrative", NARRATIVE_KW, true},
  {"Sammanhang", NARRATIVE_KW, true},
  {"Tarina", NARRATIVE_KW, true},
  {"Összefoglalás", NARRATIVE_KW, true},
  {"故事", NARRATIVE_KW, true},
  // Story/Context/Sammanfattning — not jbehave-core keywords, but some
  // real-world stories use them as narrative-block aliases (each behaves
  // exactly like a
  // `Narrative:`/`Sammanhang:` block, often several in one file and often
  // with an inline story id). They're inert wherever narrative isn't valid.
  {"Context", NARRATIVE_KW, true},
  {"Sammanfattning", NARRATIVE_KW, true},
  {"Story", NARRATIVE_KW, true},
  // Meta
  {"Meta", META_KW, true},
  {"Metadatos", META_KW, true},
  // GivenStories
  {"Belirli Öyküler", GIVEN_STORIES_KW, true},
  {"Dadas las historias", GIVEN_STORIES_KW, true},
  {"Dados as Histórias", GIVEN_STORIES_KW, true},
  {"Date le Storie", GIVEN_STORIES_KW, true},
  {"ElőfeltételSztorik", GIVEN_STORIES_KW, true},
  {"Etant donné les Histoires", GIVEN_STORIES_KW, true},
  {"Gitt stories", GIVEN_STORIES_KW, false},
  {"GivenStories", GIVEN_STORIES_KW, true},
  {"GivnaBerättelser", GIVEN_STORIES_KW, true},
  {"Käyttäjätarinat", GIVEN_STORIES_KW, true},
  {"VorgegebeneStories", GIVEN_STORIES_KW, true},
  {"由于故事", GIVEN_STORIES_KW, true},
  // Scenario
  {"Cenário", SCENARIO_KW, true},
  {"Escenario", SCENARIO_KW, true},
  {"Forgatókönyv", SCENARIO_KW, true},
  {"SENARYO", SCENARIO_KW, true},
  {"Scenario", SCENARIO_KW, true},
  {"Scenariusz", SCENARIO_KW, true},
  {"Scénario", SCENARIO_KW, true},
  {"Skenaario", SCENARIO_KW, true},
  {"Szenario", SCENARIO_KW, true},
  {"場景", SCENARIO_KW, true},
  // Lifecycle
  {"Lifecycle", LIFECYCLE_KW, true},
  {"Livscykel", LIFECYCLE_KW, true},
  // Before
  {"Before", BEFORE_KW, true},
  {"Innan", BEFORE_KW, true},
  // After
  {"After", AFTER_KW, true},
  {"Efter", AFTER_KW, true},
  // Scope
  {"Scope", SCOPE_KW, true},
  {"Omfattning", SCOPE_KW, true},
  // Examples (ExamplesTable)
  {"Beispiele", EXAMPLES_KW, true},
  {"Ejemplos", EXAMPLES_KW, true},
  {"Eksempler", EXAMPLES_KW, true},
  {"Esempi", EXAMPLES_KW, true},
  {"Esimerkit", EXAMPLES_KW, true},
  {"Examples", EXAMPLES_KW, true},
  {"Exempel", EXAMPLES_KW, true},
  {"Exemples", EXAMPLES_KW, true},
  {"Exemplos", EXAMPLES_KW, true},
  {"Przykłady", EXAMPLES_KW, true},
  {"Példák", EXAMPLES_KW, true},
  {"Örnekler", EXAMPLES_KW, true},
  {"例子", EXAMPLES_KW, true},
  // Given
  {"Adott", GIVEN_KW, false},
  {"Dado", GIVEN_KW, false},
  {"Dado que", GIVEN_KW, false},
  {"Dato che", GIVEN_KW, false},
  {"Etant donné que", GIVEN_KW, false},
  {"Gegeben", GIVEN_KW, false},
  {"Gitt", GIVEN_KW, false},
  {"Given", GIVEN_KW, false},
  {"Givet", GIVEN_KW, false},
  {"Oletetaan", GIVEN_KW, false},
  {"Zakładając, że", GIVEN_KW, false},
  {"Ön Koşul", GIVEN_KW, true},
  {"假如", GIVEN_KW, false},
  // When
  {"Cuando", WHEN_KW, false},
  {"Gdy", WHEN_KW, false},
  {"Ha", WHEN_KW, false},
  {"Koşul", WHEN_KW, true},
  {"Kun", WHEN_KW, false},
  {"När", WHEN_KW, false},
  {"Når", WHEN_KW, false},
  {"Quand", WHEN_KW, false},
  {"Quando", WHEN_KW, false},
  {"Wenn", WHEN_KW, false},
  {"When", WHEN_KW, false},
  {"當", WHEN_KW, false},
  // Then
  {"Akkor", THEN_KW, false},
  {"Allora", THEN_KW, false},
  {"Alors", THEN_KW, false},
  {"Beklenen", THEN_KW, true},
  {"Dann", THEN_KW, false},
  {"Entonces", THEN_KW, false},
  {"Então", THEN_KW, false},
  {"Niin", THEN_KW, false},
  {"Så", THEN_KW, false},
  {"Then", THEN_KW, false},
  {"Wtedy", THEN_KW, false},
  {"那麼", THEN_KW, false},
  // And
  {"And", AND_KW, false},
  {"E", AND_KW, false},
  {"Et", AND_KW, false},
  {"I", AND_KW, false},
  {"Ja", AND_KW, false},
  {"Och", AND_KW, false},
  {"Og", AND_KW, false},
  {"Und", AND_KW, false},
  {"Ve", AND_KW, true},
  {"Y", AND_KW, false},
  {"És", AND_KW, false},
  {"並且", AND_KW, false},
};

static const size_t KEYWORD_COUNT = sizeof(KEYWORDS) / sizeof(KEYWORDS[0]);

static bool is_word_char(int32_t c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
         (c >= '0' && c <= '9') || c == '_' ||
         // Treat any non-ASCII codepoint as a word character too, so a
         // keyword immediately followed by more letters in another language
         // (accented Latin, CJK, etc.) isn't mistaken for a valid boundary.
         c >= 0x80;
}

// Decodes one UTF-8 codepoint starting at *p, advancing *p past its encoded
// bytes. Returns -1 on malformed input (shouldn't happen for the keyword
// table below, which is well-formed UTF-8 source text).
static int32_t decode_utf8(const unsigned char **p) {
  const unsigned char *s = *p;
  if (s[0] < 0x80) {
    *p += 1;
    return s[0];
  } else if ((s[0] & 0xE0) == 0xC0) {
    if (s[1] == '\0' || (s[1] & 0xC0) != 0x80) return -1;
    *p += 2;
    return ((int32_t)(s[0] & 0x1F) << 6) | (s[1] & 0x3F);
  } else if ((s[0] & 0xF0) == 0xE0) {
    if (s[1] == '\0' || s[2] == '\0' || (s[1] & 0xC0) != 0x80 || (s[2] & 0xC0) != 0x80) return -1;
    *p += 3;
    return ((int32_t)(s[0] & 0x0F) << 12) | ((int32_t)(s[1] & 0x3F) << 6) |
           (s[2] & 0x3F);
  } else if ((s[0] & 0xF8) == 0xF0) {
    if (s[1] == '\0' || s[2] == '\0' || s[3] == '\0' || (s[1] & 0xC0) != 0x80 ||
        (s[2] & 0xC0) != 0x80 || (s[3] & 0xC0) != 0x80) return -1;
    *p += 4;
    return ((int32_t)(s[0] & 0x07) << 18) | ((int32_t)(s[1] & 0x3F) << 12) |
           ((int32_t)(s[2] & 0x3F) << 6) | (s[3] & 0x3F);
  }
  return -1;
}

typedef struct {
  const Keyword *kw;
  const unsigned char *cursor; // position within kw->text's UTF-8 bytes
} Candidate;

// First-match-wins among prefix-sharing keywords (e.g. "Dado" vs "Dado que":
// "Dado que ..." matches "Dado", leaving "que" as step text). Both map to
// the same token type, so longest-match would only change consumed length —
// deferred until a real story shows it matters.
//
// Tries every keyword whose type is currently valid *simultaneously*,
// advancing the lexer exactly once per matched character rather than
// retrying each candidate's full text one at a time. This matters because
// TSLexer only moves forward (there's no rewind): once localized keywords
// were added, entries of the same type can share a prefix (e.g. Polish
// "Narracja" vs English "Narrative" both start "Narra"), and a naive
// try-fully-then-retry-next-candidate loop would leave the lexer stranded
// mid-input after the first candidate's partial match, causing every
// subsequent candidate — including the one that should have matched — to
// compare against the wrong position.
static bool try_match_keywords(TSLexer *lexer, const bool *valid_symbols,
                                enum TokenType *out_type) {
  Candidate candidates[KEYWORD_COUNT];
  size_t alive = 0;
  for (size_t i = 0; i < KEYWORD_COUNT; i++) {
    if (valid_symbols[KEYWORDS[i].type]) {
      candidates[alive].kw = &KEYWORDS[i];
      candidates[alive].cursor = (const unsigned char *)KEYWORDS[i].text;
      alive++;
    }
  }
  if (alive == 0) return false;

  for (;;) {
    // Any candidate whose text is fully consumed completes here — check its
    // boundary condition against the lexer position as it stands right now,
    // before consuming anything further.
    for (size_t i = 0; i < alive; i++) {
      if (*candidates[i].cursor != '\0') continue;
      const Keyword *kw = candidates[i].kw;
      bool boundary_ok = kw->needs_colon
        ? lexer->lookahead == ':' // peek only; the grammar matches ":" itself
        : !is_word_char(lexer->lookahead);
      if (boundary_ok) {
        lexer->mark_end(lexer);
        *out_type = kw->type;
        return true;
      }
    }

    // Advance every still-viable candidate by one codepoint, dropping any
    // whose next expected character doesn't match the actual input (and
    // dropping already-complete-but-boundary-failed candidates from above).
    int32_t current = lexer->lookahead;
    size_t next_alive = 0;
    for (size_t i = 0; i < alive; i++) {
      if (*candidates[i].cursor == '\0') continue;
      const unsigned char *peek = candidates[i].cursor;
      int32_t expected = decode_utf8(&peek);
      if (expected == current) {
        candidates[next_alive].kw = candidates[i].kw;
        candidates[next_alive].cursor = peek;
        next_alive++;
      }
    }
    alive = next_alive;
    if (alive == 0) return false;
    lexer->advance(lexer, false);
  }
}

typedef struct {
  // Whether the last external keyword this scanner emitted was a section
  // *header* keyword (Narrative:, Scenario:, Meta:, Lifecycle:, ...) rather
  // than a step keyword or plain content. Used by the EOL comment-swallow
  // rule below.
  bool last_is_header;
} ScannerState;

static bool is_header_keyword(enum TokenType type) {
  switch (type) {
    case NARRATIVE_KW:
    case META_KW:
    case GIVEN_STORIES_KW:
    case SCENARIO_KW:
    case LIFECYCLE_KW:
    case BEFORE_KW:
    case AFTER_KW:
    case SCOPE_KW:
    case EXAMPLES_KW:
      return true;
    default:
      return false;
  }
}

// Verifies that an EXAMPLES keyword is genuinely introducing an Examples
// table rather than appearing in prose (real-world "Exempel:" is frequently
// followed by a bare story-path reference or inline text instead of a
// table). When no table row follows, the keyword is *not* emitted: it falls
// through to be lexed as ordinary text, which stops the GLR error-recovery
// blowup that a committed-but-unsatisfiable examples rule otherwise hits.
//
// The lexer is positioned on the ":" that terminates the keyword (mark_end
// already sits right before it, so any bytes consumed here beyond the colon
// are re-lexed — they're never part of this token). We skip the colon, the
// line terminator, and any fully-blank or `!--` comment lines, and accept
// only when the next content line actually starts a table row ("|").
static bool examples_table_follows(TSLexer *lexer) {
  lexer->advance(lexer, false); // ":"
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
    lexer->advance(lexer, false);
  }
  if (lexer->lookahead == '\r') {
    lexer->advance(lexer, false);
  }
  if (lexer->lookahead != '\n') {
    return false; // text follows on the same line — not an Examples header
  }
  lexer->advance(lexer, false);
  for (;;) {
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
      lexer->advance(lexer, false);
    }
    if (lexer->lookahead == '\r') {
      lexer->advance(lexer, false);
    }
    if (lexer->lookahead == '\n') {
      lexer->advance(lexer, false); // fully-blank line — keep looking
      continue;
    }
    if (lexer->eof(lexer)) {
      return false;
    }
    if (lexer->lookahead == '!') {
      // Possible `!--` comment line; only treat it as such on a confirmed
      // "--" prefix, otherwise bail.
      lexer->advance(lexer, false);
      if (lexer->lookahead == '-') {
        lexer->advance(lexer, false);
        if (lexer->lookahead == '-') {
          while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
            lexer->advance(lexer, false);
          }
          if (lexer->eof(lexer)) {
            return false;
          }
          continue; // consume the newline at the top of the loop
        }
      }
      return false;
    }
    return lexer->lookahead == '|';
  }
}

void *tree_sitter_jbehave_external_scanner_create(void) {
  ScannerState *state = calloc(1, sizeof(ScannerState));
  return state;
}
void tree_sitter_jbehave_external_scanner_destroy(void *payload) { free(payload); }
unsigned tree_sitter_jbehave_external_scanner_serialize(void *payload, char *buffer) {
  ScannerState *state = payload;
  buffer[0] = state->last_is_header ? 1 : 0;
  return 1;
}
void tree_sitter_jbehave_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  ScannerState *state = payload;
  state->last_is_header = length > 0 && buffer[0] != 0;
}

bool tree_sitter_jbehave_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  ScannerState *state = payload;
  if (valid_symbols[EOL]) {
    // Match one required line terminator, then greedily absorb any further
    // fully-blank lines (optional inline spaces/tabs then another
    // terminator) into the same token. This keeps blank-line skipping a
    // bounded lexical concern instead of a grammar-level repetition, which
    // previously caused catastrophic GLR ambiguity when blank lines were
    // also modeled as an `extras` token.
    //
    // A full-line comment (`!-- ...`) that interrupts the run is absorbed
    // into the EOL token when either (a) the run has already crossed a
    // fully-blank line, or (b) the run starts right after non-header
    // content (a step or table body). (a) keeps the common "divider
    // comment" pattern — step/table, blank line, comment, next step — from
    // stranding the parser on the newline that follows the comment, since
    // no token is valid there. (b) covers comments sandwiched directly
    // between a table row and the next line with no blank in between. A
    // comment that directly follows a section header (Narrative:,
    // Scenario:, Meta:, ...) is deliberately left in place so it stays a
    // visible (comment) node — see test/corpus/comments.txt.
    bool saw_newline = false;
    bool saw_blank = false;
    bool first_newline = true;
    for (;;) {
      while (lexer->lookahead == ' ' || lexer->lookahead == '\t') {
        lexer->advance(lexer, false);
      }
      if (lexer->lookahead == '\r') {
        lexer->advance(lexer, false);
      }
      if (lexer->lookahead == '\n') {
        lexer->advance(lexer, false);
        saw_newline = true;
        // The first newline consumed always terminates the *preceding*
        // content line; only later newlines (those preceded by nothing but
        // inline spaces/tabs) end genuinely blank lines.
        if (!first_newline) saw_blank = true;
        first_newline = false;
        lexer->mark_end(lexer);
        continue;
      }
      if (lexer->eof(lexer)) {
        if (!saw_newline) return false;
        lexer->mark_end(lexer);
        lexer->result_symbol = EOL;
        state->last_is_header = false;
        return true;
      }
      if (saw_newline && (saw_blank || !state->last_is_header) &&
          lexer->lookahead == '!') {
        // Peek "--" to confirm this is actually a comment line. On failure
        // we return false and the lexer rewinds to the token start, leaving
        // the previous EOL token in effect — the same outcome as today.
        lexer->advance(lexer, false);
        if (lexer->lookahead == '-') {
          lexer->advance(lexer, false);
          if (lexer->lookahead == '-') {
            // Consume the rest of the comment line as part of the EOL run.
            while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
              lexer->advance(lexer, false);
            }
            if (lexer->eof(lexer)) {
              if (!saw_newline) return false;
              lexer->mark_end(lexer);
              lexer->result_symbol = EOL;
              state->last_is_header = false;
              return true;
            }
            lexer->advance(lexer, false);
            saw_newline = true;
            lexer->mark_end(lexer);
            continue;
          }
        }
        return false;
      }
      break;
    }
    if (saw_newline) {
      lexer->result_symbol = EOL;
      state->last_is_header = false;
      return true;
    }
  }

  enum TokenType matched_type;
  if (try_match_keywords(lexer, valid_symbols, &matched_type)) {
    if (matched_type == EXAMPLES_KW && !examples_table_follows(lexer)) {
      return false;
    }
    lexer->result_symbol = matched_type;
    state->last_is_header = is_header_keyword(matched_type);
    return true;
  }

  return false;
}
