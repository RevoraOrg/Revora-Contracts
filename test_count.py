def count_args_in_call(text, start_idx):
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

# Test with multiline call
test = "register_offering(\n        &issuer,\n        &symbol_short!(\"ns\"),\n        &offering_token,\n        &10_000,\n        &payment_token,\n        &0,\n    )"
count, end = count_args_in_call(test, test.index('('))
print("multiline 6-arg count:", count, "end_idx:", end)

# Already-updated 8-arg
test2 = "register_offering(&issuer, &Vec::new(&env), &1u32, &ns, &token, &1_000, &payout_asset, &0)"
count2, end2 = count_args_in_call(test2, test2.index('('))
print("8-arg count:", count2)

# The issue: trailing comma after last arg
test3 = "register_offering(\n        &issuer,\n        &symbol_short!(\"ns\"),\n        &token,\n        &1_000,\n        &payout,\n        &0,\n    )"
count3, end3 = count_args_in_call(test3, test3.index('('))
print("multiline trailing-comma count:", count3)
