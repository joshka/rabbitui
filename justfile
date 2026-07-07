betamax := env_var_or_default("BETAMAX", "betamax")

# Run every example tape (captures GIFs, screenshots, and state under target/betamax/)
tapes:
    {{betamax}} validate 'tapes/*.tape'
    for t in tapes/*.tape; do {{betamax}} run "$t"; done

# Validate tape syntax without running
tapes-check:
    {{betamax}} validate 'tapes/*.tape'

# Render the gallery tapes (one per theme) and copy the final-frame PNGs into
# docs/images/ under stable names for the README and style guide. The PNGs are
# human-reviewed acceptance artifacts, committed when they change meaningfully —
# no pixel-diffing in CI (betamax rendering is not pixel-stable across hosts).
screenshots:
    {{betamax}} validate 'tapes/gallery-*.tape'
    for t in tapes/gallery-*.tape; do {{betamax}} run "$t"; done
    mkdir -p docs/images
    for png in target/betamax/gallery-*-top.png target/betamax/gallery-*-roles.png; do \
        [ -f "$png" ] && cp "$png" "docs/images/$(basename "$png")"; \
    done
    echo "Refreshed docs/images/gallery-*.png"
