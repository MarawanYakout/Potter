"""Potter Plugin — /explain

Explains a concept, code snippet, or error message in plain language.

Usage:
    /explain <concept or code>
    /explain like:5yo <concept>       — explain like I'm 5
    /explain like:expert <concept>    — expert-level explanation

Example:
    /explain the borrow checker in Rust
    /explain like:5yo what is an API
"""

COMMAND = "/explain"
DESCRIPTION = "Explain a concept or code in plain language"
VERSION = "1.0.0"


def run(args: str) -> str:
    args = args.strip()
    level = "clear, plain language suitable for a developer"

    if args.lower().startswith("like:"):
        parts = args.split(None, 1)
        modifier = parts[0][5:].lower()  # everything after "like:"
        args = parts[1].strip() if len(parts) > 1 else ""

        if modifier in ("5yo", "kid", "simple"):
            level = "simple terms a 5-year-old could understand, using analogies"
        elif modifier in ("expert", "advanced", "phd"):
            level = "expert-level technical depth, assuming deep domain knowledge"
        else:
            level = f"the perspective of a {modifier}"

    if not args:
        return "Please provide a concept or code snippet to explain."

    return (
        f"Explain the following in {level}. "
        f"Be concise and accurate.\n\n"
        f"{args}"
    )
