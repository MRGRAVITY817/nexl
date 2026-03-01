#!/usr/bin/env python3
"""Convert defn/def docstrings from regular to triple-quoted strings."""

import re
import sys
from pathlib import Path


def reindent_body(body_lines: list[str]) -> list[str]:
    """Reindent body lines for triple-quoted docstrings.

    Rules:
    - Non-code-block lines: already at 2-space indent; keep as-is.
    - Lines inside ```...``` blocks: add 2 leading spaces to preserve
      relative indentation (after dedent_triple strips the 2-space base,
      the relative nesting is maintained).
    - Blank lines: kept blank everywhere.
    """
    result = []
    in_code_block = False

    for line in body_lines:
        stripped = line.strip()

        # Toggle code block on opening/closing backtick fence
        if stripped.startswith("```"):
            in_code_block = not in_code_block
            # The fence line itself has 2-space indent already; keep it.
            result.append(line)
            continue

        if stripped == "":
            result.append(line)
            continue

        if in_code_block:
            # Add 2 spaces so that after dedent_triple (which strips 2) the
            # original column is restored, while preserving relative indent.
            result.append("  " + line)
        else:
            # Regular text: already at 2-space indent; keep.
            result.append(line)

    return result


def convert_file(path: Path) -> bool:
    """Convert docstrings in a .nx file. Returns True if any changes were made."""
    text = path.read_text(encoding="utf-8")
    lines = text.split("\n")
    result = []
    i = 0
    changed = False

    while i < len(lines):
        line = lines[i]

        # Detect multi-line docstring opening:
        # - line starts with exactly 2 spaces then `"`
        # - has more text after `"` (non-empty)
        # - does NOT end with a bare `"` (which would be a one-liner)
        m = re.match(r'^  "(.+)$', line)
        if m:
            content_first_line = m.group(1)

            # One-liner check: ends with `"` not preceded by `\`
            if (
                content_first_line.endswith('"')
                and not content_first_line.endswith('\\"')
            ):
                result.append(line)
                i += 1
                continue

            # Scan ahead for the closing line: exactly `  "` + optional spaces
            closing_idx = None
            for j in range(i + 1, len(lines)):
                if re.match(r'^  "\s*$', lines[j]):
                    closing_idx = j
                    break

            if closing_idx is None:
                # No closing found — leave as-is
                result.append(line)
                i += 1
                continue

            body_lines = lines[i + 1:closing_idx]
            new_body = reindent_body(body_lines)

            # Emit triple-quoted form
            result.append('  """')
            result.append("  " + content_first_line)
            result.extend(new_body)
            result.append('  """')
            i = closing_idx + 1
            changed = True
            continue

        result.append(line)
        i += 1

    if changed:
        path.write_text("\n".join(result), encoding="utf-8")
    return changed


def main():
    if len(sys.argv) < 2:
        print("Usage: convert_docstrings.py <file.nx> [file2.nx ...]")
        sys.exit(1)

    for arg in sys.argv[1:]:
        p = Path(arg)
        if convert_file(p):
            print(f"  converted: {p}")
        else:
            print(f"  unchanged: {p}")


if __name__ == "__main__":
    main()
