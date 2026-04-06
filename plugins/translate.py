"""Potter Plugin — /translate

Translates the given text to English (default) or a specified target language.

Usage:
    /translate <text>
    /translate to:french <text>

Example:
    /translate Bonjour le monde
    /translate to:spanish Hello world
"""

COMMAND = "/translate"
DESCRIPTION = "Translate text to English (or specify target with to:<lang>)"
VERSION = "1.0.0"


def run(args: str) -> str:
    """
    Pre-processes the user's input into a translation prompt.
    The return value replaces the original prompt sent to the LLM.

    :param args: Everything the user typed after /translate
    :return: A fully formed prompt string for the LLM
    """
    args = args.strip()
    target_lang = "English"

    # Check for optional `to:<language>` prefix
    if args.lower().startswith("to:"):
        parts = args.split(None, 1)  # split on first whitespace
        lang_spec = parts[0]  # e.g. "to:french"
        target_lang = lang_spec[3:].capitalize()
        args = parts[1] if len(parts) > 1 else ""

    if not args:
        return "Please provide text to translate."

    return (
        f"Translate the following text to {target_lang}. "
        f"Output only the translation, nothing else.\n\n"
        f"{args}"
    )
