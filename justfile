betamax := env_var_or_default("BETAMAX", "betamax")

# Run every example tape (captures GIFs, screenshots, and state under target/betamax/)
tapes:
    {{betamax}} validate 'tapes/*.tape'
    for t in tapes/*.tape; do {{betamax}} run "$t"; done

# Validate tape syntax without running
tapes-check:
    {{betamax}} validate 'tapes/*.tape'
