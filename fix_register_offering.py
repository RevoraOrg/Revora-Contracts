#!/usr/bin/env python3
"""
Fix register_offering call sites: add co_issuers and quorum parameters.

Old signature (6 args): register_offering(issuer, namespace, token, bps, payout, cap)
New signature (8 args): register_offering(primary_issuer, co_issuers, quorum, namespace, token, bps, payout, cap)

Rust multiline calls have trailing commas, so actual arg count:
- single-line 6-arg: count == 6
- multiline 6-arg with trailing comma: count == 7

The env variable is always named `env` in all test call sites.
"""

import re
import os


SRC_DIR = r"c:\Users\EMMA\Desktop\Revora-Contracts\src"


def count_args_in_call(text, start_idx):
    """
    Count top-level comma-separated args in a function call.
    Returns (arg_count, end_idx) where end_idx is index of the closing ')'.
    """
    depth = 0
    args = 0
    i = start_idx
    while i < len(text):
        c = text[i]
        if c == '(' and i == start_idx:
            depth = 1
            args = 1
        elif c == '(':
            depth += 1
        elif c == ')':
            if depth == 1:
                return args, i
            depth -= 1
        elif c == ',' and depth == 1:
            args += 1
        i += 1
    return args, -1


def is_trailing_comma_call(text, paren_start, end_idx):
    """Check if the call has a trailing comma (Rust multiline style)."""
    # Look at the last non-whitespace char before the closing ')'
    before_close = text[paren_start:end_idx]
    stripped = before_close.rstrip()
    return stripped.endswith(',')


def transform_single_line(call_text, env_name="env"):
    """Transform a single-line 6-arg register_offering call to 8-arg."""
    paren_pos = call_text.index('(')
    depth = 0
    first_comma = -1
    for i in range(paren_pos, len(call_text)):
        c = call_text[i]
        if c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
        elif c == ',' and depth == 1:
            first_comma = i
            break
    if first_comma == -1:
        return call_text
    
    return (call_text[:first_comma + 1] +
            f" &Vec::new(&{env_name}), &1u32," +
            call_text[first_comma + 1:])


def transform_multiline(call_text, env_name="env"):
    """
    Transform a multiline 6-arg register_offering call to 8-arg.
    Inserts two new arg lines after the first argument.
    """
    paren_pos = call_text.index('(')
    depth = 0
    first_comma = -1
    for i in range(paren_pos, len(call_text)):
        c = call_text[i]
        if c == '(':
            depth += 1
        elif c == ')':
            if depth == 1:
                break
            depth -= 1
        elif c == ',' and depth == 1:
            first_comma = i
            break
    
    if first_comma == -1:
        return call_text
    
    # Find indentation of the second argument line
    rest_after_comma = call_text[first_comma + 1:]
    nl_idx = rest_after_comma.find('\n')
    if nl_idx >= 0:
        line_after = rest_after_comma[nl_idx + 1:]
        indent_m = re.match(r'(\s*)', line_after)
        line_indent = indent_m.group(1) if indent_m else '        '
    else:
        line_indent = '        '
    
    # Build insertion after the first comma
    insertion = f"\n{line_indent}&Vec::new(&{env_name}),\n{line_indent}&1u32,"
    
    return call_text[:first_comma + 1] + insertion + call_text[first_comma + 1:]


def process_content(content, env_name="env"):
    """Process file content and return modified content."""
    pattern = re.compile(r'(?:try_)?register_offering\(')
    
    result = []
    last_end = 0
    
    for m in pattern.finditer(content):
        start = m.start()
        paren_start = m.end() - 1  # index of '('
        
        rest = content[paren_start:]
        arg_count, end_offset = count_args_in_call(rest, 0)
        
        if end_offset == -1:
            continue
        
        call_text = content[m.start(): paren_start + end_offset + 1]
        is_multiline = '\n' in call_text
        
        # Determine effective arg count (subtract 1 for trailing comma in multiline)
        has_trailing = is_trailing_comma_call(rest, 0, end_offset)
        effective_args = arg_count - (1 if has_trailing else 0)
        
        if effective_args != 6:
            # Already updated (8 args) or some other call, skip
            continue
        
        if is_multiline:
            transformed = transform_multiline(call_text, env_name)
        else:
            transformed = transform_single_line(call_text, env_name)
        
        result.append(content[last_end:m.start()])
        result.append(transformed)
        last_end = paren_start + end_offset + 1
    
    result.append(content[last_end:])
    return ''.join(result)


def process_file(filepath):
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()
    
    original = content
    new_content = process_content(content)
    
    if new_content != original:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(new_content)
        print(f"Updated: {filepath}")
        return True
    else:
        print(f"No changes: {filepath}")
        return False


def main():
    src_dir = SRC_DIR
    changed = 0
    total = 0
    
    for root, dirs, files in os.walk(src_dir):
        for fname in files:
            if fname.endswith('.rs'):
                fpath = os.path.join(root, fname)
                total += 1
                if process_file(fpath):
                    changed += 1
    
    print(f"\nProcessed {total} files, changed {changed}")


if __name__ == '__main__':
    main()
