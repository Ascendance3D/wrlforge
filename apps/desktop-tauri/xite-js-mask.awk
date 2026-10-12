# SPDX-License-Identifier: GPL-3.0-or-later
# STACK-REVIEW-1 follow-up: a small JavaScript lexer for check-xite-adapters.sh.
# It masks every byte that is not executable top-level code, so a declaration
# inside a comment, a string or a template literal can never match:
#   - a comment becomes one space (its newlines stay, so lines keep numbers);
#   - a string literal becomes '_' (or "_"), unless its whole text is an
#     exact version like 16.5.0, which stays as written ('16.5.0');
#   - a template literal, with every ${...} expression in it, becomes `_`.
# Output: one line per source line, "DEPTH<TAB>MASKED", where DEPTH is the
# brace depth of top-level code at the start of that line; then one line
# "TOP<TAB>CODE" with all depth-0 code, whitespace removed.
# It fails closed (exit 3) on anything it cannot lex with certainty: an
# unterminated comment, string or template, a string line continuation,
# unbalanced braces, or a "/" that could start a regular-expression literal.
# POSIX awk only (gawk, mawk and the BSD/macOS awk). Input must have no CR.
#   F=NAME awk -f xite-js-mask.awk FILE   (F: the name used in messages)
function die(m) { printf "%s:%d: %s; refusing\n", (ENVIRON["F"] != "" ? ENVIRON["F"] : FILENAME), line, m > "/dev/stderr"; bad = 1; exit 3 }
function put(s) { if (sp == 1) { buf = buf s; if (depth == 0) { gsub(/[ \t]/, "", s); top = top s } } }
function flush() { printf "%d\t%s\n", ld, buf; buf = ""; ld = depth; line++ }
function idch(ch) { return ch ~ /[A-Za-z0-9_$]/ }
{ src = src $0 "\n" }
END {
  if (bad) exit 3
  split("return typeof instanceof in of new delete void throw case do else yield await", k, " ")
  for (x in k) kw[k[x]] = 1
  n = length(src); i = 1; line = 1; depth = 0; ld = 0; sp = 1; mode[1] = "code"; br[1] = 0
  prev = ""; w = ""; buf = ""; top = ""
  while (i <= n) {
    c = substr(src, i, 1); d = substr(src, i + 1, 1)
    if (c == "\n") { flush(); i++; continue }
    if (mode[sp] == "tmpl") {
      if (c == "\\") { if (d == "\n") flush(); i += 2; continue }
      if (c == "`") { sp--; prev = "`"; w = ""; i++; continue }
      if (c == "$" && d == "{") { sp++; mode[sp] = "code"; br[sp] = 0; prev = "{"; w = ""; i += 2; continue }
      i++; continue
    }
    # code
    if (c == " " || c == "\t") { put(c); i++; continue }
    if (c == "/" && d == "/") { while (i <= n && substr(src, i, 1) != "\n") i++; put(" "); continue }
    if (c == "/" && d == "*") {
      i += 2
      while (i <= n && !(substr(src, i, 1) == "*" && substr(src, i + 1, 1) == "/")) { if (substr(src, i, 1) == "\n") flush(); i++ }
      if (i > n) die("unterminated block comment")
      put(" "); i += 2; continue
    }
    if (c == "'" || c == "\"") {
      j = i + 1; esc = 0
      while (j <= n && substr(src, j, 1) != c) {
        if (substr(src, j, 1) == "\n") die("unterminated string")
        if (substr(src, j, 1) == "\\") { if (substr(src, j + 1, 1) == "\n") die("string line continuation"); esc = 1; j++ }
        j++
      }
      if (j > n) die("unterminated string")
      t = substr(src, i + 1, j - i - 1)
      if (esc || t !~ /^[0-9]+(\.[0-9]+)*$/) t = "_"
      put(c t c); prev = c; w = ""; i = j + 1; continue
    }
    if (c == "`") { put("`_`"); sp++; mode[sp] = "tmpl"; i++; continue }
    if (c == "/") {
      if (!(prev ~ /[])A-Za-z0-9_$'"`]/) || (w in kw)) die("possible regular-expression literal")
    }
    if (c == "{") { if (sp == 1) { put(c); depth++; prev = c; w = ""; i++; continue } br[sp]++ }
    if (c == "}") {
      if (sp > 1) { if (br[sp] == 0) { sp--; i++; continue } br[sp]-- }
      else if (--depth < 0) die("unbalanced }")
    }
    put(c)
    # w: the identifier that ends at prev (for the "/" check after a keyword)
    if (idch(c)) w = (idch(prev) && substr(src, i - 1, 1) == prev ? w c : c); else w = ""
    prev = c; i++
  }
  if (sp != 1) die("unterminated template literal")
  if (depth != 0) die("unbalanced {")
  if (buf != "") flush()
  printf "TOP\t%s\n", top
}
