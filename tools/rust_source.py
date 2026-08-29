"""Where a Rust file's production code is, as against its test modules.

Every mutation screen here needs the same thing: mutate the code under test,
not the tests. All three of them answered it the same wrong way -- stop at the
first `#[cfg(test)]` -- under a docstring that said

    Every test module in this crate is the tail of its file, so the first
    `#[cfg(test)]` is enough.

That was true when it was written. It is not true now. `component_themes.rs`
has six test modules and roughly 2,500 lines of production code *after* the
first one; `render.rs` has eight; `borders.rs`, `widget_state.rs`, `theme.rs`,
`decoration.rs` and `slider_theme.rs` all have more than one. Tick 223 found
this by mutating twenty-seven sites by hand and having a screen report only
sixteen of them.

The comment named its own assumption, which is the only reason the gap was
findable at all. It still cost three ticks of screens quietly reading part of
a file. So: count braces, and let the caller ask about a line.
"""
import re

ATTRIBUTE = re.compile(r'^\s*#\[cfg\(test\)\]\s*$')
MODULE = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{')


def test_spans(text):
    """The [start, end) character spans of every `#[cfg(test)] mod ... { }`.

    The attribute may be followed by blank lines, comments or further
    attributes before the `mod` line; anything else means the attribute is on
    something other than a module (an item, a function) and is skipped.
    """
    spans = []
    lines = text.split('\n')
    starts = []
    offset = 0
    for line in lines:
        starts.append(offset)
        offset += len(line) + 1

    i = 0
    while i < len(lines):
        if not ATTRIBUTE.match(lines[i]):
            i += 1
            continue
        j = i + 1
        while j < len(lines) and (not lines[j].strip()
                                  or lines[j].lstrip().startswith('//')
                                  or lines[j].lstrip().startswith('#[')):
            j += 1
        if j >= len(lines) or not MODULE.match(lines[j]):
            i += 1
            continue

        # Count braces from the `mod` line to its close. String and character
        # literals holding a stray brace would break this; none of the test
        # module headers in this crate do, and a miscount would show up as a
        # span that swallows the rest of the file, which is visible.
        depth = 0
        k = j
        while k < len(lines):
            depth += lines[k].count('{') - lines[k].count('}')
            if depth <= 0 and k > j:
                break
            if depth <= 0 and k == j and lines[k].count('}'):
                break
            k += 1
        end = starts[k] + len(lines[k]) + 1 if k < len(lines) else len(text)
        spans.append((starts[i], end))
        i = k + 1
    return spans


def in_test(spans, position):
    """Is this character offset inside one of the spans?"""
    return any(start <= position < end for start, end in spans)


def production(text):
    """A predicate: is this character offset outside every test module?"""
    spans = test_spans(text)
    return lambda position: not in_test(spans, position)


if __name__ == '__main__':
    import io
    import os
    import sys

    import paths

    root = sys.argv[1] if len(sys.argv) > 1 else paths.SRC
    for name in sorted(os.listdir(root)):
        if not name.endswith('.rs'):
            continue
        text = io.open(os.path.join(root, name), encoding='utf-8').read()
        text = text.replace('\r\n', '\n')
        spans = test_spans(text)
        if len(spans) > 1:
            after = len(text) - max(end for _, end in spans)
            print('%-28s %d test modules, %d chars of code after the first'
                  % (name, len(spans),
                     max(0, len(text) - spans[0][1] - sum(
                         end - start for start, end in spans[1:]))))
            del after
