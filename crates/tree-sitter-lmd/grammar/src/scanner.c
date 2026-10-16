#include <stdio.h>
#include "tree_sitter/alloc.h"
#include "tree_sitter/parser.h"
#include <wctype.h>

enum TokenType {
  BLOCK_COMMENT_START,
  BLOCK_COMMENT_CONTENT,
  BLOCK_COMMENT_END,

  BLOCK_STRING_START,
  BLOCK_STRING_CONTENT,
  BLOCK_STRING_END,

  // lmd extension: markdown fragments, three kinds.
  //   `{=`  … `=}`   unmarked (pandoc.Nodes: context decides)
  //   `{=:` … `:=}`  blocks  (pandoc.Blocks)
  //   `{=[` … `]=}`  inlines (pandoc.Inlines)
  NODES_FRAGMENT_START,
  NODES_FRAGMENT_CONTENT,
  NODES_FRAGMENT_END,
  BLOCKS_FRAGMENT_START,
  BLOCKS_FRAGMENT_CONTENT,
  BLOCKS_FRAGMENT_END,
  INLINES_FRAGMENT_START,
  INLINES_FRAGMENT_CONTENT,
  INLINES_FRAGMENT_END,
};

static inline void consume(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

static inline bool consume_char(char c, TSLexer *lexer) {
  if (lexer->lookahead != c) {
    return false;
  }

  consume(lexer);
  return true;
}

static inline uint8_t consume_and_count_char(char c, TSLexer *lexer) {
  uint8_t count = 0;
  while (lexer->lookahead == c) {
    ++count;
    consume(lexer);
  }
  return count;
}

static inline void skip_whitespaces(TSLexer *lexer) {
  while (iswspace(lexer->lookahead)) {
    skip(lexer);
  }
}

typedef struct {
  char ending_char;
  uint8_t level_count;
  // lmd: level of the fragment whose START was just scanned (1 for `{=`,
  // 2 for `{==`, …), read back when scanning its CONTENT and END.
  uint8_t fragment_level;
} Scanner;

static inline void reset_state(Scanner *scanner) {
  scanner->ending_char = 0;
  scanner->level_count = 0;
}

void *tree_sitter_lmd_external_scanner_create() {
  Scanner *scanner = ts_calloc(1, sizeof(Scanner));
  return scanner;
}

void tree_sitter_lmd_external_scanner_destroy(void *payload) {
  Scanner *scanner = (Scanner *)payload;
  ts_free(scanner);
}

unsigned tree_sitter_lmd_external_scanner_serialize(void *payload, char *buffer) {
  Scanner *scanner = (Scanner *)payload;
  buffer[0] = scanner->ending_char;
  buffer[1] = (char)scanner->level_count;
  buffer[2] = (char)scanner->fragment_level;
  return 3;
}

void tree_sitter_lmd_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  Scanner *scanner = (Scanner *)payload;
  scanner->ending_char = 0;
  scanner->level_count = 0;
  scanner->fragment_level = 0;
  if (length == 0) return;
  scanner->ending_char = buffer[0];
  if (length == 1) return;
  scanner->level_count = buffer[1];
  if (length == 2) return;
  scanner->fragment_level = (uint8_t)buffer[2];
}

static bool scan_block_start(Scanner *scanner, TSLexer *lexer) {
  if (consume_char('[', lexer)) {
    uint8_t level = consume_and_count_char('=', lexer);

    if (consume_char('[', lexer)) {
      scanner->level_count = level;
      return true;
    }
  }

  return false;
}

static bool scan_block_end(Scanner *scanner, TSLexer *lexer) {
  if (consume_char(']', lexer)) {
    uint8_t level = consume_and_count_char('=', lexer);

    if (scanner->level_count == level && consume_char(']', lexer)) {
      return true;
    }
  }

  return false;
}

static bool scan_block_content(Scanner *scanner, TSLexer *lexer) {
  while (lexer->lookahead != 0) {
    if (lexer->lookahead == ']') {
      lexer->mark_end(lexer);

      if (scan_block_end(scanner, lexer)) {
        return true;
      }
    } else {
      consume(lexer);
    }
  }

  return false;
}

static bool scan_comment_start(Scanner *scanner, TSLexer *lexer) {
  if (consume_char('-', lexer) && consume_char('-', lexer)) {
    lexer->mark_end(lexer);

    if (scan_block_start(scanner, lexer)) {
      lexer->mark_end(lexer);
      lexer->result_symbol = BLOCK_COMMENT_START;
      return true;
    }
  }

  return false;
}

static bool scan_comment_content(Scanner *scanner, TSLexer *lexer) {
  if (scanner->ending_char == 0) { // block comment
    if (scan_block_content(scanner, lexer)) {
      lexer->result_symbol = BLOCK_COMMENT_CONTENT;
      return true;
    }

    return false;
  }

  while (lexer->lookahead != 0) {
    if (lexer->lookahead == scanner->ending_char) {
      reset_state(scanner);
      lexer->result_symbol = BLOCK_COMMENT_CONTENT;
      return true;
    }

    consume(lexer);
  }

  return false;
}

// lmd fragment closers: `=}` for the unmarked form, `<close_char>=}` for
// the marked forms (`:=}` for blocks, `]=}` for inlines), with as many `=`
// as the opener had (`:==}` closes `{==:`). `*consumed` reports how many
// characters a failed attempt ate, so the content scan that follows can
// count them as content (the token spans from the scan's start anyway).
static bool scan_fragment_end(TSLexer *lexer, char close_char, uint8_t level, uint32_t *consumed) {
  *consumed = 0;
  if (close_char != 0) {
    if (!consume_char(close_char, lexer)) {
      return false;
    }
    (*consumed)++;
  }
  for (uint8_t i = 0; i < level; i++) {
    if (!consume_char('=', lexer)) {
      return false;
    }
    (*consumed)++;
  }
  return consume_char('}', lexer);
}

// lmd fragment content at level 1: everything up to the closer of this
// fragment's kind, balancing nested fragments of any kind so a fragment
// whose markdown contains inline lmd code with its own fragment still ends
// in the right place. The rule is simple: every `{=` opens (its context
// marker and any extra `=`, if any, are ordinary content), every `=}`
// closes when nested, and at depth zero only this kind's closer ends the
// token. A backslash makes the character after it content, so `\{=` and
// `\=}` neither open nor close (markdown then reads `\{` and `\=` as the
// literal characters). The token stops just before the closer (mark_end),
// which the parser then lexes as the END token. Returns false on an empty
// fragment so `{==}` parses as start + end with no content node.
static bool scan_fragment_content_level1(TSLexer *lexer, char close_char, bool consumed_any) {
  uint32_t depth = 0;
  while (lexer->lookahead != 0) {
    if (lexer->lookahead == '\\') {
      consume(lexer);
      if (lexer->lookahead != 0) {
        consume(lexer);
      }
    } else if (lexer->lookahead == '{') {
      consume(lexer);
      if (lexer->lookahead == '=') {
        consume(lexer);
        depth++;
      }
    } else if (close_char != 0 && lexer->lookahead == close_char) {
      // Possible `<close_char>=}` closer: remember the position before it.
      lexer->mark_end(lexer);
      consume(lexer);
      if (lexer->lookahead == '=') {
        consume(lexer);
        if (lexer->lookahead == '}') {
          if (depth == 0) {
            return consumed_any;
          }
          depth--;
          consume(lexer);
        }
      }
    } else if (lexer->lookahead == '=') {
      lexer->mark_end(lexer);
      consume(lexer);
      if (lexer->lookahead == '}') {
        if (depth == 0) {
          if (close_char == 0) {
            return consumed_any;
          }
          // A bare `=}` at depth zero inside a marked fragment is content.
          consume(lexer);
        } else {
          depth--;
          consume(lexer);
        }
      }
    } else {
      consume(lexer);
    }
    consumed_any = true;
    lexer->mark_end(lexer);
  }
  return false;
}

// lmd fragment content at level 2 and above (`{==: … :==}`): raw text up to
// the first closer of this kind with exactly `level` `=`, like a Lua long
// string. Nothing inside is balanced or escaped; that is what the higher
// level is for (a body that quotes fragment syntax).
static bool scan_fragment_content_leveled(TSLexer *lexer, char close_char, uint8_t level, bool consumed_any) {
  char first = close_char != 0 ? close_char : '=';
  while (lexer->lookahead != 0) {
    if (lexer->lookahead == first) {
      lexer->mark_end(lexer);
      if (close_char != 0) {
        consume(lexer);
      }
      uint8_t run = consume_and_count_char('=', lexer);
      if (run == level && lexer->lookahead == '}') {
        return consumed_any;
      }
      // Not a closer: what was consumed is content; scanning continues
      // from here (a closer cannot start inside a run of `=`).
    } else {
      consume(lexer);
    }
    consumed_any = true;
    lexer->mark_end(lexer);
  }
  return false;
}

static bool scan_fragment_content(TSLexer *lexer, char close_char, uint8_t level, bool consumed_any) {
  if (level <= 1) {
    return scan_fragment_content_level1(lexer, close_char, consumed_any);
  }
  return scan_fragment_content_leveled(lexer, close_char, level, consumed_any);
}

bool tree_sitter_lmd_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *scanner = (Scanner *)payload;

  // lmd: inside a fragment, END wins over CONTENT at the closer; CONTENT
  // is raw (no whitespace skipping) so the markdown keeps its layout. Only
  // one kind's CONTENT/END symbols are valid at a time (the one whose
  // START the parser just consumed), so the kind is read off valid_symbols
  // and no scanner state is needed.
  static const struct {
    enum TokenType start, content, end;
    char close_char;
  } fragment_kinds[] = {
    {NODES_FRAGMENT_START, NODES_FRAGMENT_CONTENT, NODES_FRAGMENT_END, 0},
    {BLOCKS_FRAGMENT_START, BLOCKS_FRAGMENT_CONTENT, BLOCKS_FRAGMENT_END, ':'},
    {INLINES_FRAGMENT_START, INLINES_FRAGMENT_CONTENT, INLINES_FRAGMENT_END, ']'},
  };
  const size_t fragment_kind_count = sizeof(fragment_kinds) / sizeof(fragment_kinds[0]);
  const uint8_t level = scanner->fragment_level == 0 ? 1 : scanner->fragment_level;

  for (size_t i = 0; i < fragment_kind_count; i++) {
    uint32_t consumed = 0;
    if (valid_symbols[fragment_kinds[i].end] && scan_fragment_end(lexer, fragment_kinds[i].close_char, level, &consumed)) {
      lexer->result_symbol = fragment_kinds[i].end;
      scanner->fragment_level = 0;
      return true;
    }
    if (valid_symbols[fragment_kinds[i].content] && scan_fragment_content(lexer, fragment_kinds[i].close_char, level, consumed > 0)) {
      lexer->result_symbol = fragment_kinds[i].content;
      return true;
    }
  }

  if (valid_symbols[BLOCK_STRING_END] && scan_block_end(scanner, lexer)) {
    reset_state(scanner);
    lexer->result_symbol = BLOCK_STRING_END;
    return true;
  }

  if (valid_symbols[BLOCK_STRING_CONTENT] && scan_block_content(scanner, lexer)) {
    lexer->result_symbol = BLOCK_STRING_CONTENT;
    return true;
  }

  if (valid_symbols[BLOCK_COMMENT_END] && scanner->ending_char == 0 && scan_block_end(scanner, lexer)) {
    reset_state(scanner);
    lexer->result_symbol = BLOCK_COMMENT_END;
    return true;
  }

  if (valid_symbols[BLOCK_COMMENT_CONTENT] && scan_comment_content(scanner, lexer)) {
    return true;
  }

  skip_whitespaces(lexer);

  // lmd: the marked openers are longer than the unmarked one, so they are
  // tried first; on failure the lexer position is not reset between
  // attempts, so each attempt re-scans from a fresh `{`.
  if (valid_symbols[BLOCKS_FRAGMENT_START] || valid_symbols[INLINES_FRAGMENT_START] || valid_symbols[NODES_FRAGMENT_START]) {
    // Peek without committing: all three openers start with `{=`. More
    // `=` raise the level (`{==:` … `:==}`, after Lua's long brackets),
    // except that `{==}` is the empty unmarked fragment at level 1, not
    // a level-2 opener: the opener's `=` run followed directly by `}`
    // backs off to `{=`, and the rest is content and closer.
    if (lexer->lookahead == '{') {
      consume(lexer);
      if (lexer->lookahead == '=') {
        consume(lexer);
        lexer->mark_end(lexer);
        uint8_t level = 1;
        while (lexer->lookahead == '=') {
          consume(lexer);
          level++;
        }
        if (level > 1) {
          if (lexer->lookahead == '}') {
            level = 1;
          } else {
            lexer->mark_end(lexer);
          }
        }
        scanner->fragment_level = level;
        if (lexer->lookahead == ':') {
          if (!valid_symbols[BLOCKS_FRAGMENT_START]) return false;
          consume(lexer);
          lexer->mark_end(lexer);
          lexer->result_symbol = BLOCKS_FRAGMENT_START;
          return true;
        }
        if (lexer->lookahead == '[') {
          if (!valid_symbols[INLINES_FRAGMENT_START]) return false;
          consume(lexer);
          lexer->mark_end(lexer);
          lexer->result_symbol = INLINES_FRAGMENT_START;
          return true;
        }
        if (!valid_symbols[NODES_FRAGMENT_START]) return false;
        lexer->result_symbol = NODES_FRAGMENT_START;
        return true;
      }
      // A lone `{`: not ours. Fall through; the block-string / comment
      // scans below start with other characters and will not match the
      // already-consumed `{`, so this returns false for the internal lexer.
      return false;
    }
  }

  if (valid_symbols[BLOCK_STRING_START] && scan_block_start(scanner, lexer)) {
    lexer->result_symbol = BLOCK_STRING_START;
    return true;
  }

  if (valid_symbols[BLOCK_COMMENT_START]) {
    if (scan_comment_start(scanner, lexer)) {
      return true;
    }
  }

  return false;
}
